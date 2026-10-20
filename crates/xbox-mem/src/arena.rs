//! A arena: os 64 MiB de RAM unificada do console.
//!
//! **Este é o único módulo do projeto com `unsafe` na Fase 2.** Toda a API pública é
//! segura: quem usa a arena recebe `&[u8]`/`&mut [u8]` e indexa com checagem, como em
//! qualquer fatia Rust. Os invariantes de cada bloco estão em
//! `docs/arch/memoria-unsafe.md`.
//!
//! ## Por que não é um `Vec<u8>`
//!
//! Porque a Fase 5 precisa importar esta memória no Vulkan. Vértices e texturas que o
//! jogo monta ficam na RAM do guest, e importar memória em Vulkan exige um **objeto
//! compartilhável** do sistema — `memfd` no Linux, objeto de seção no Windows. Com
//! memória anônima, só restaria copiar tudo a cada frame (ADR-0011).
//!
//! A decisão é tomada agora porque mudá-la depois obrigaria a refatorar o subsistema
//! inteiro. Nada de GPU é implementado aqui: só a criação do objeto e o descritor
//! guardado para quem precisar depois.
//!
//! ## Três origens possíveis
//!
//! | Origem | Quando | Importável no Vulkan |
//! |---|---|---|
//! | objeto compartilhável do SO | caminho normal | sim |
//! | alocação comum | o SO recusou o objeto | não |
//! | alocação comum | sob Miri | não se aplica |
//!
//! A reserva para fallback não é defensiva por acaso: sem ela, o emulador não rodaria
//! em ambiente onde `memfd_create` não existe (kernel antigo, sandbox restritivo), e
//! **Miri não conseguiria verificar nada**, porque não executa chamada de sistema.

use std::alloc::Layout;
use std::ptr::NonNull;

use xbox_core::error::SnapshotError;
use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_core::{Error, RAM_SIZE, Result};

use crate::addr::ArenaOff;

/// De onde vieram os bytes, e como devolvê-los.
#[derive(Debug)]
#[cfg_attr(miri, allow(dead_code))]
enum Origem {
    /// Objeto de memória compartilhável do sistema operacional.
    Compartilhada(backing::Objeto),
    /// Alocação comum do processo.
    Alocada(Layout),
}

/// A RAM do console.
///
/// `Send` mas **não** `Sync`: mover a arena entre threads é seguro, acesso concorrente
/// não. A leitura concorrente pela thread de GPU é trabalho da Fase 5, com o invariante
/// do ADR-0011 (a GPU só lê faixa cuja posse o título cedeu pelo pushbuffer).
#[derive(Debug)]
pub struct Arena {
    ponteiro: NonNull<u8>,
    tamanho: usize,
    origem: Origem,
}

// SAFETY: a arena é dona exclusiva da região apontada por `ponteiro`; não há estado
// compartilhado nem thread-local envolvido, então transferir a posse entre threads é
// seguro. `Sync` **não** é implementado de propósito: dois acessos concorrentes a esta
// memória é exatamente o que a Fase 5 terá de tratar com cuidado (ADR-0011).
unsafe impl Send for Arena {}

impl Arena {
    /// Cria a arena com o tamanho da RAM do console (64 MiB, ou 128 com `ram128`).
    pub fn nova() -> Result<Self> {
        Self::com_tamanho(RAM_SIZE)
    }

    /// Cria uma arena de tamanho arbitrário.
    ///
    /// Existe para teste e para o modo de dev kit; o tamanho precisa ser múltiplo de
    /// página, porque a MMU e o alocador de frames trabalham em páginas inteiras.
    pub fn com_tamanho(tamanho: usize) -> Result<Self> {
        if tamanho == 0
            || tamanho > isize::MAX as usize
            || !tamanho.is_multiple_of(xbox_core::PAGE_SIZE)
        {
            return Err(Error::config(format!(
                "tamanho de arena {tamanho} inválido: exige múltiplo de {} bytes e limite isize::MAX",
                xbox_core::PAGE_SIZE
            )));
        }

        // Caminho preferido: objeto compartilhável, para a Fase 5 poder importar.
        // Sob Miri não há chamada de sistema, então vai direto para a alocação.
        #[cfg(not(miri))]
        if let Some((ponteiro, objeto)) = backing::criar(tamanho) {
            tracing::debug!(tamanho, "arena em objeto compartilhável do sistema");
            return Ok(Self {
                ponteiro,
                tamanho,
                origem: Origem::Compartilhada(objeto),
            });
        }

        // Reserva: alocação comum. Perde a importação no Vulkan, mas o emulador roda.
        let layout = Layout::from_size_align(tamanho, xbox_core::PAGE_SIZE)
            .map_err(|e| Error::config(format!("layout de arena inválido: {e}")))?;
        // SAFETY: `layout` tem tamanho não nulo (checado acima) e alinhamento válido
        // (potência de dois, vindo de PAGE_SIZE). `alloc_zeroed` devolve ponteiro
        // alinhado para `tamanho` bytes zerados, ou nulo em falha — que tratamos.
        let bruto = unsafe { std::alloc::alloc_zeroed(layout) };
        let ponteiro = NonNull::new(bruto).ok_or_else(|| {
            Error::config(format!(
                "não foi possível alocar {tamanho} bytes para a arena"
            ))
        })?;
        tracing::debug!(tamanho, "arena em alocação comum (sem compartilhamento)");
        Ok(Self {
            ponteiro,
            tamanho,
            origem: Origem::Alocada(layout),
        })
    }

    /// Tamanho da arena, em bytes.
    pub const fn len(&self) -> usize {
        self.tamanho
    }

    /// Nunca vazia — existe para satisfazer o lint que acompanha `len`.
    pub const fn is_empty(&self) -> bool {
        self.tamanho == 0
    }

    /// A arena está em objeto compartilhável, isto é, pode ser importada no Vulkan?
    pub const fn compartilhavel(&self) -> bool {
        matches!(self.origem, Origem::Compartilhada(_))
    }

    /// Descritor do objeto compartilhável, para a Fase 5 importar no Vulkan.
    ///
    /// `None` quando a arena está em alocação comum. Nenhum código de GPU usa isto
    /// ainda; o descritor é guardado porque criar a arena de outra forma depois seria
    /// refatoração do subsistema inteiro.
    pub fn descritor(&self) -> Option<backing::Descritor> {
        match &self.origem {
            Origem::Compartilhada(o) => Some(o.descritor()),
            Origem::Alocada(_) => None,
        }
    }

    /// A arena como fatia imutável.
    pub fn bytes(&self) -> &[u8] {
        // SAFETY: `ponteiro` aponta para `tamanho` bytes inicializados (zerados na
        // criação) e permanece válido enquanto `self` viver, porque `self` é dono da
        // origem. O empréstimo `&self` garante que ninguém tem `&mut` ao mesmo tempo, e
        // `Arena` não é `Sync`, então não há leitor em outra thread.
        unsafe { std::slice::from_raw_parts(self.ponteiro.as_ptr(), self.tamanho) }
    }

    /// A arena como fatia mutável.
    pub fn bytes_mut(&mut self) -> &mut [u8] {
        // SAFETY: mesmas condições de `bytes`, e `&mut self` garante exclusividade do
        // acesso durante o empréstimo.
        unsafe { std::slice::from_raw_parts_mut(self.ponteiro.as_ptr(), self.tamanho) }
    }

    /// Lê `destino.len()` bytes a partir de um deslocamento.
    ///
    /// Erra sem tocar em memória quando a faixa passa do fim da arena. É a barreira que
    /// garante que endereço inválido do guest não vira acesso indevido no host.
    pub fn ler(&self, off: ArenaOff, destino: &mut [u8]) -> Result<()> {
        let fatia = self.faixa(off, destino.len())?;
        destino.copy_from_slice(fatia);
        Ok(())
    }

    /// Escreve bytes em um deslocamento.
    pub fn escrever(&mut self, off: ArenaOff, origem: &[u8]) -> Result<()> {
        let fim = Self::fim(off, origem.len(), self.tamanho)?;
        self.bytes_mut()[off.indice()..fim].copy_from_slice(origem);
        Ok(())
    }

    /// Fatia imutável de uma faixa da arena.
    pub fn faixa(&self, off: ArenaOff, len: usize) -> Result<&[u8]> {
        let fim = Self::fim(off, len, self.tamanho)?;
        Ok(&self.bytes()[off.indice()..fim])
    }

    /// Fatia mutável de uma faixa da arena.
    pub fn faixa_mut(&mut self, off: ArenaOff, len: usize) -> Result<&mut [u8]> {
        let fim = Self::fim(off, len, self.tamanho)?;
        Ok(&mut self.bytes_mut()[off.indice()..fim])
    }

    /// Calcula o fim de uma faixa, recusando transbordo e estouro de limite.
    fn fim(off: ArenaOff, len: usize, tamanho: usize) -> Result<usize> {
        let fim = off
            .indice()
            .checked_add(len)
            .ok_or_else(|| Error::format(format!("faixa absurda em {off} + {len}")))?;
        if fim > tamanho {
            return Err(Error::format(format!(
                "faixa {off}..+{len} passa do fim da arena (0x{tamanho:X})"
            )));
        }
        Ok(fim)
    }
}

impl Drop for Arena {
    fn drop(&mut self) {
        match &mut self.origem {
            Origem::Compartilhada(objeto) => {
                // SAFETY: `ponteiro` e `tamanho` são exatamente o que `backing::criar`
                // devolveu junto com este objeto, e `drop` só roda uma vez.
                unsafe { backing::destruir(self.ponteiro, self.tamanho, objeto) };
            }
            Origem::Alocada(layout) => {
                // SAFETY: `ponteiro` veio de `alloc_zeroed` com este mesmo `layout`, e
                // `drop` só roda uma vez.
                unsafe { std::alloc::dealloc(self.ponteiro.as_ptr(), *layout) };
            }
        }
    }
}

impl Snapshot for Arena {
    fn save(&self, out: &mut SnapshotWriter) {
        out.section("ARN0", 1);
        out.u64(self.tamanho as u64);
        out.raw(self.bytes());
    }

    fn load(&mut self, inp: &mut SnapshotReader<'_>) -> std::result::Result<(), SnapshotError> {
        let _v = inp.section("ARN0", 1)?;
        let tamanho = inp.u64()? as usize;
        if tamanho != self.tamanho {
            return Err(SnapshotError::BadValue {
                section: "ARN0",
                what: "tamanho de arena diferente do build atual",
            });
        }
        let dados = inp.raw(tamanho)?;
        self.bytes_mut().copy_from_slice(dados);
        Ok(())
    }
}

/// Criação do objeto de memória compartilhável, por sistema operacional.
///
/// As funções do sistema são declaradas à mão em vez de usar um crate de bindings: o
/// que precisamos é um punhado de funções, e no host de desenvolvimento atual isto
/// também contorna o problema de toolchain do ADR-0008.
#[cfg_attr(miri, allow(dead_code))]
pub mod backing {
    use std::ptr::NonNull;

    /// Identificador do objeto compartilhável, para a Fase 5 importar no Vulkan.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Descritor {
        /// Descritor de arquivo de um `memfd` (Linux).
        Fd(i32),
        /// Handle de objeto de seção (Windows).
        Handle(isize),
    }

    #[cfg(target_os = "linux")]
    mod sistema {
        use std::ffi::c_void;
        use std::ptr::NonNull;

        use super::Descritor;

        const MFD_CLOEXEC: u32 = 0x0001;
        const PROT_READ: i32 = 0x1;
        const PROT_WRITE: i32 = 0x2;
        const MAP_SHARED: i32 = 0x01;

        unsafe extern "C" {
            fn memfd_create(nome: *const i8, flags: u32) -> i32;
            fn ftruncate(fd: i32, tamanho: i64) -> i32;
            fn mmap(
                addr: *mut c_void,
                len: usize,
                prot: i32,
                flags: i32,
                fd: i32,
                off: i64,
            ) -> *mut c_void;
            fn munmap(addr: *mut c_void, len: usize) -> i32;
            fn close(fd: i32) -> i32;
        }

        /// Objeto compartilhável: um `memfd` anônimo.
        #[derive(Debug)]
        pub struct Objeto {
            fd: i32,
        }

        impl Objeto {
            pub fn descritor(&self) -> Descritor {
                Descritor::Fd(self.fd)
            }
        }

        pub(super) fn criar(tamanho: usize) -> Option<(NonNull<u8>, Objeto)> {
            let nome = c"obsidian-arena";
            // SAFETY: `nome` é um literal C terminado em NUL e vivo durante a chamada.
            let fd = unsafe { memfd_create(nome.as_ptr(), MFD_CLOEXEC) };
            if fd < 0 {
                return None;
            }
            // SAFETY: `fd` é válido (checado acima); `tamanho` cabe em i64.
            if unsafe { ftruncate(fd, tamanho as i64) } != 0 {
                // SAFETY: fd válido, fechado uma vez só.
                unsafe { close(fd) };
                return None;
            }
            // SAFETY: fd válido e dimensionado; pedimos ao kernel que escolha o
            // endereço (addr nulo). MAP_SHARED é o que torna o objeto importável.
            let p = unsafe {
                mmap(
                    std::ptr::null_mut(),
                    tamanho,
                    PROT_READ | PROT_WRITE,
                    MAP_SHARED,
                    fd,
                    0,
                )
            };
            if p.is_null() || p as isize == -1 {
                // SAFETY: fd válido, fechado uma vez só.
                unsafe { close(fd) };
                return None;
            }
            NonNull::new(p.cast::<u8>()).map(|nn| (nn, Objeto { fd }))
        }

        /// # Safety
        ///
        /// `ponteiro`/`tamanho` têm de ser exatamente o que `criar` devolveu com este
        /// `objeto`, e esta função pode ser chamada no máximo uma vez por objeto.
        pub(super) unsafe fn destruir(ponteiro: NonNull<u8>, tamanho: usize, objeto: &mut Objeto) {
            // SAFETY: garantido pelo contrato da função.
            unsafe {
                munmap(ponteiro.as_ptr().cast::<c_void>(), tamanho);
                close(objeto.fd);
            }
        }
    }

    #[cfg(windows)]
    mod sistema {
        use std::ptr::NonNull;

        use super::Descritor;

        const PAGE_READWRITE: u32 = 0x04;
        const FILE_MAP_ALL_ACCESS: u32 = 0x000F_001F;
        const INVALID_HANDLE_VALUE: isize = -1;

        #[allow(non_snake_case)]
        unsafe extern "system" {
            fn CreateFileMappingW(
                arquivo: isize,
                atributos: *const u8,
                protecao: u32,
                tamanho_alto: u32,
                tamanho_baixo: u32,
                nome: *const u16,
            ) -> isize;
            fn MapViewOfFile(
                mapeamento: isize,
                acesso: u32,
                off_alto: u32,
                off_baixo: u32,
                bytes: usize,
            ) -> *mut u8;
            fn UnmapViewOfFile(base: *const u8) -> i32;
            fn CloseHandle(handle: isize) -> i32;
        }

        /// Objeto compartilhável: um objeto de seção anônimo.
        #[derive(Debug)]
        pub struct Objeto {
            handle: isize,
        }

        impl Objeto {
            pub fn descritor(&self) -> Descritor {
                Descritor::Handle(self.handle)
            }
        }

        pub(super) fn criar(tamanho: usize) -> Option<(NonNull<u8>, Objeto)> {
            let alto = (tamanho >> 32) as u32;
            let baixo = (tamanho & 0xFFFF_FFFF) as u32;
            // SAFETY: seção anônima (sem nome, sem arquivo) do tamanho pedido.
            // Devolve 0 em falha, que tratamos.
            let handle = unsafe {
                CreateFileMappingW(
                    INVALID_HANDLE_VALUE,
                    std::ptr::null(),
                    PAGE_READWRITE,
                    alto,
                    baixo,
                    std::ptr::null(),
                )
            };
            if handle == 0 {
                return None;
            }
            // SAFETY: `handle` é válido e a view cobre o objeto inteiro.
            let p = unsafe { MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, tamanho) };
            if let Some(nn) = NonNull::new(p) {
                Some((nn, Objeto { handle }))
            } else {
                // SAFETY: handle válido, fechado uma vez só.
                unsafe { CloseHandle(handle) };
                None
            }
        }

        /// # Safety
        ///
        /// `ponteiro` tem de ser a view devolvida por `criar` junto com este `objeto`, e
        /// esta função pode ser chamada no máximo uma vez por objeto.
        pub(super) unsafe fn destruir(ponteiro: NonNull<u8>, _tamanho: usize, objeto: &mut Objeto) {
            // SAFETY: garantido pelo contrato da função.
            unsafe {
                UnmapViewOfFile(ponteiro.as_ptr());
                CloseHandle(objeto.handle);
            }
        }
    }

    #[cfg(not(any(target_os = "linux", windows)))]
    mod sistema {
        use std::ptr::NonNull;

        use super::Descritor;

        /// Sistema sem suporte: a arena cai na alocação comum.
        #[derive(Debug)]
        pub struct Objeto;

        impl Objeto {
            pub fn descritor(&self) -> Descritor {
                Descritor::Fd(-1)
            }
        }

        pub(super) fn criar(_tamanho: usize) -> Option<(NonNull<u8>, Objeto)> {
            None
        }

        /// # Safety
        ///
        /// Inalcançável: `criar` nunca devolve objeto nesta plataforma.
        pub(super) unsafe fn destruir(_p: NonNull<u8>, _t: usize, _o: &mut Objeto) {}
    }

    pub use sistema::Objeto;

    pub(super) fn criar(tamanho: usize) -> Option<(NonNull<u8>, Objeto)> {
        sistema::criar(tamanho)
    }

    /// # Safety
    ///
    /// Ver o contrato da implementação por sistema: `ponteiro` e `tamanho` têm de ser
    /// os que `criar` devolveu com este `objeto`, uma única vez.
    pub(super) unsafe fn destruir(ponteiro: NonNull<u8>, tamanho: usize, objeto: &mut Objeto) {
        // SAFETY: repassado do chamador, que tem o mesmo contrato.
        unsafe { sistema::destruir(ponteiro, tamanho, objeto) };
    }
}

#[cfg(test)]
mod tests {
    use super::Arena;
    use crate::addr::ArenaOff;
    use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};

    fn arena_de_teste() -> Arena {
        Arena::com_tamanho(64 * 1024).expect("arena de teste")
    }

    #[test]
    fn nasce_zerada_e_do_tamanho_pedido() {
        let a = arena_de_teste();
        assert_eq!(a.len(), 64 * 1024);
        assert!(a.bytes().iter().all(|&b| b == 0), "arena não nasceu zerada");
    }

    #[test]
    fn tamanho_precisa_ser_multiplo_de_pagina() {
        assert!(Arena::com_tamanho(0).is_err());
        assert!(Arena::com_tamanho(1).is_err());
        assert!(Arena::com_tamanho(4095).is_err());
        assert!(Arena::com_tamanho(4096).is_ok());
    }

    #[test]
    fn ida_e_volta_de_bytes() {
        let mut a = arena_de_teste();
        a.escrever(ArenaOff(0x100), b"obsidian").unwrap();
        let mut buf = [0u8; 8];
        a.ler(ArenaOff(0x100), &mut buf).unwrap();
        assert_eq!(&buf, b"obsidian");
    }

    #[test]
    fn faixa_fora_dos_limites_erra_sem_tocar_memoria() {
        let mut a = arena_de_teste();
        let fim = a.len() as u32;
        assert!(a.faixa(ArenaOff(fim), 1).is_err());
        assert!(
            a.faixa(ArenaOff(fim - 2), 4).is_err(),
            "faixa cruzando o fim"
        );
        assert!(a.faixa(ArenaOff(u32::MAX), 4).is_err());
        assert!(a.escrever(ArenaOff(fim - 1), b"xx").is_err());
        // E o último byte válido continua acessível.
        assert!(a.faixa(ArenaOff(fim - 1), 1).is_ok());
    }

    #[test]
    fn origem_e_coerente_com_o_descritor() {
        let a = arena_de_teste();
        if a.compartilhavel() {
            assert!(a.descritor().is_some(), "compartilhável sem descritor");
        } else {
            assert!(a.descritor().is_none(), "alocada com descritor");
        }
    }

    #[test]
    fn snapshot_ida_e_volta() {
        let mut a = arena_de_teste();
        a.escrever(ArenaOff(0x200), b"estado").unwrap();

        let mut w = SnapshotWriter::new();
        a.save(&mut w);
        let bytes = w.finish();

        let mut b = arena_de_teste();
        let mut r = SnapshotReader::new(&bytes).unwrap();
        b.load(&mut r).unwrap();

        assert_eq!(a.bytes(), b.bytes());
        let mut buf = [0u8; 6];
        b.ler(ArenaOff(0x200), &mut buf).unwrap();
        assert_eq!(&buf, b"estado");
    }

    #[test]
    fn snapshot_de_tamanho_diferente_e_recusado() {
        let a = arena_de_teste();
        let mut w = SnapshotWriter::new();
        a.save(&mut w);
        let bytes = w.finish();

        let mut outra = Arena::com_tamanho(8 * 1024).unwrap();
        let mut r = SnapshotReader::new(&bytes).unwrap();
        assert!(outra.load(&mut r).is_err());
    }

    #[test]
    fn muitas_arenas_nao_vazam() {
        // Criar e destruir em laço é o que acusaria falta de `munmap`/`CloseHandle`.
        for _ in 0..64 {
            let mut a = Arena::com_tamanho(256 * 1024).unwrap();
            a.escrever(ArenaOff(0), b"x").unwrap();
        }
    }
}
