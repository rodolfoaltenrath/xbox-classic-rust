//! Os espaços de endereçamento, como tipos distintos.
//!
//! Implementa o ADR-0009. Três endereços do guest cabem em `u32` e significam coisas
//! diferentes; confundi-los produz o bug mais caro de diagnosticar num emulador,
//! porque o sintoma aparece fases depois, dentro da GPU, como "textura errada".
//!
//! O caminho real que torna a confusão fatal, e que o jogo verificado na Fase 1 usa:
//!
//! 1. o título chama `MmAllocateContiguousMemory` e recebe um [`GuestVirt`];
//! 2. chama `MmGetPhysicalAddress` e recebe um [`GuestPhys`];
//! 3. escreve esse físico num objeto DMA do NV2A;
//! 4. a GPU lê **por endereço físico**, sem passar pela MMU.
//!
//! Com `u32` em tudo, o passo 3 aceita um virtual em silêncio. Com estes tipos, não
//! compila.
//!
//! ## Nenhuma conversão implícita
//!
//! Não existe `From` entre eles, nem `as`, nem aritmética de janela escondida. As
//! únicas conversões possíveis são:
//!
//! | De | Para | Como |
//! |---|---|---|
//! | [`GuestVirt`] | [`GuestPhys`] | `Mmu::traduzir` |
//! | [`GuestPhys`] | [`ArenaOff`] | `Bus::classificar` |
//! | [`ArenaOff`] | ponteiro do host | `Arena`, interno, `unsafe` |

use std::fmt;

use xbox_core::PAGE_SIZE;

/// Tamanho de uma página grande do x86 (PSE, 4 MiB).
pub const LARGE_PAGE_SIZE: usize = 4 * 1024 * 1024;

/// Máscara dos bits de deslocamento dentro de uma página de 4 KiB.
pub const PAGE_MASK: u32 = PAGE_SIZE as u32 - 1;

/// Máscara dos bits de deslocamento dentro de uma página de 4 MiB.
pub const LARGE_PAGE_MASK: u32 = LARGE_PAGE_SIZE as u32 - 1;

/// Endereço **virtual** do guest: o que o código x86 do título manipula.
///
/// Passa pela MMU. O entry point do NFSU2, `0x0021B1CE`, é um destes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct GuestVirt(pub u32);

impl GuestVirt {
    /// Número da página de 4 KiB que contém este endereço.
    pub const fn pagina(self) -> u32 {
        self.0 >> 12
    }

    /// Índice no diretório de páginas (bits 31..22).
    pub const fn indice_diretorio(self) -> usize {
        (self.0 >> 22) as usize
    }

    /// Índice na tabela de páginas (bits 21..12).
    pub const fn indice_tabela(self) -> usize {
        ((self.0 >> 12) & 0x3FF) as usize
    }

    /// Deslocamento dentro da página de 4 KiB.
    pub const fn deslocamento(self) -> u32 {
        self.0 & PAGE_MASK
    }

    /// Deslocamento dentro da página de 4 MiB.
    pub const fn deslocamento_grande(self) -> u32 {
        self.0 & LARGE_PAGE_MASK
    }

    /// Endereço do início da página que contém este endereço.
    #[must_use]
    pub const fn base_da_pagina(self) -> Self {
        Self(self.0 & !PAGE_MASK)
    }

    /// Avança `n` bytes, saturando no fim do espaço de 32 bits.
    ///
    /// Satura em vez de dar wrap porque wrap silencioso em aritmética de endereço é
    /// fonte de bug: um acesso em `0xFFFFFFFF` deve falhar, não voltar para zero.
    #[must_use]
    pub const fn saturating_add(self, n: u32) -> Self {
        Self(self.0.saturating_add(n))
    }

    /// Soma `n` bytes, ou `None` se transbordar o espaço de 32 bits.
    pub const fn checked_add(self, n: u32) -> Option<Self> {
        match self.0.checked_add(n) {
            Some(v) => Some(Self(v)),
            None => None,
        }
    }
}

impl fmt::Display for GuestVirt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "virt:0x{:08X}", self.0)
    }
}

/// Endereço **físico** do Xbox.
///
/// RAM abaixo de `0x04000000`; dispositivos acima de `0xFD000000`. É o que
/// `MmGetPhysicalAddress` devolve e o que a NV2A usa em DMA.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct GuestPhys(pub u32);

impl GuestPhys {
    /// Número do frame de 4 KiB.
    pub const fn frame(self) -> u32 {
        self.0 >> 12
    }

    /// Endereço físico do início de um frame.
    pub const fn do_frame(frame: u32) -> Self {
        Self(frame << 12)
    }

    /// Deslocamento dentro da página de 4 KiB.
    pub const fn deslocamento(self) -> u32 {
        self.0 & PAGE_MASK
    }

    /// Soma `n` bytes, ou `None` se transbordar.
    pub const fn checked_add(self, n: u32) -> Option<Self> {
        match self.0.checked_add(n) {
            Some(v) => Some(Self(v)),
            None => None,
        }
    }
}

impl fmt::Display for GuestPhys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "fis:0x{:08X}", self.0)
    }
}

/// Deslocamento dentro da arena de RAM do emulador.
///
/// Igual ao [`GuestPhys`] quando o físico aponta para RAM — mas tipado em separado,
/// porque físico também endereça MMIO, que não tem deslocamento na arena, e porque a
/// arena pode ter 128 MiB com a feature `ram128`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct ArenaOff(pub u32);

impl ArenaOff {
    /// O deslocamento como índice de fatia.
    pub const fn indice(self) -> usize {
        self.0 as usize
    }
}

impl fmt::Display for ArenaOff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "arena+0x{:X}", self.0)
    }
}

/// Que tipo de acesso está sendo feito.
///
/// Determina qual bit de permissão é exigido e qual código de erro a falha carrega.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Acesso {
    /// Leitura de dados.
    Leitura,
    /// Escrita de dados.
    Escrita,
    /// Busca de instrução.
    Execucao,
}

impl Acesso {
    /// É escrita? (Afeta o bit W/R do código de erro de `#PF`.)
    pub const fn e_escrita(self) -> bool {
        matches!(self, Self::Escrita)
    }

    /// É busca de instrução? (Afeta o bit I/D do código de erro.)
    pub const fn e_execucao(self) -> bool {
        matches!(self, Self::Execucao)
    }
}

/// Permissões de uma página mapeada.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Perm(pub u8);

impl Perm {
    /// Leitura permitida.
    pub const LER: u8 = 0b001;
    /// Escrita permitida.
    pub const ESCREVER: u8 = 0b010;
    /// Execução permitida.
    pub const EXECUTAR: u8 = 0b100;

    /// Somente leitura.
    pub const RO: Self = Self(Self::LER);
    /// Leitura e escrita.
    pub const RW: Self = Self(Self::LER | Self::ESCREVER);
    /// Leitura e execução.
    pub const RX: Self = Self(Self::LER | Self::EXECUTAR);
    /// Leitura, escrita e execução.
    pub const RWX: Self = Self(Self::LER | Self::ESCREVER | Self::EXECUTAR);

    /// A permissão cobre este acesso?
    pub const fn permite(self, acesso: Acesso) -> bool {
        let bit = match acesso {
            Acesso::Leitura => Self::LER,
            Acesso::Escrita => Self::ESCREVER,
            Acesso::Execucao => Self::EXECUTAR,
        };
        self.0 & bit != 0
    }

    /// Resumo em três caracteres, no estilo `rwx`.
    pub fn rwx(self) -> String {
        let bit = |b: u8, c: char| if self.0 & b != 0 { c } else { '-' };
        format!(
            "{}{}{}",
            bit(Self::LER, 'r'),
            bit(Self::ESCREVER, 'w'),
            bit(Self::EXECUTAR, 'x')
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{Acesso, ArenaOff, GuestPhys, GuestVirt, Perm};

    #[test]
    fn decomposicao_de_endereco_virtual() {
        // 0x0021B1CE é o entry point do jogo verificado na Fase 1.
        let va = GuestVirt(0x0021_B1CE);
        assert_eq!(va.indice_diretorio(), 0);
        assert_eq!(va.indice_tabela(), 0x21B);
        assert_eq!(va.deslocamento(), 0x1CE);
        assert_eq!(va.base_da_pagina(), GuestVirt(0x0021_B000));
        assert_eq!(va.pagina(), 0x21B);
    }

    #[test]
    fn decomposicao_na_janela_contigua() {
        let va = GuestVirt(0x8000_1234);
        assert_eq!(va.indice_diretorio(), 0x200);
        assert_eq!(va.indice_tabela(), 1);
        assert_eq!(va.deslocamento(), 0x234);
    }

    #[test]
    fn aritmetica_nao_da_wrap_silencioso() {
        let va = GuestVirt(0xFFFF_FFFF);
        assert_eq!(va.checked_add(1), None, "wrap silencioso esconde bug");
        assert_eq!(va.saturating_add(1), GuestVirt(0xFFFF_FFFF));
        assert_eq!(GuestVirt(0x1000).checked_add(4), Some(GuestVirt(0x1004)));
        assert_eq!(GuestPhys(0xFFFF_FFFC).checked_add(8), None);
    }

    #[test]
    fn frames_e_deslocamentos_fisicos() {
        let pa = GuestPhys(0x0001_2345);
        assert_eq!(pa.frame(), 0x12);
        assert_eq!(pa.deslocamento(), 0x345);
        assert_eq!(GuestPhys::do_frame(0x12), GuestPhys(0x0001_2000));
    }

    #[test]
    fn permissoes_cobrem_o_acesso_certo() {
        assert!(Perm::RO.permite(Acesso::Leitura));
        assert!(!Perm::RO.permite(Acesso::Escrita));
        assert!(!Perm::RO.permite(Acesso::Execucao));

        assert!(Perm::RW.permite(Acesso::Escrita));
        assert!(!Perm::RW.permite(Acesso::Execucao));

        assert!(Perm::RX.permite(Acesso::Execucao));
        assert!(!Perm::RX.permite(Acesso::Escrita));

        assert_eq!(Perm::RO.rwx(), "r--");
        assert_eq!(Perm::RW.rwx(), "rw-");
        assert_eq!(Perm::RWX.rwx(), "rwx");
    }

    #[test]
    fn tipos_distintos_nao_se_misturam() {
        // Este teste existe para documentar a intenção do ADR-0009: as linhas
        // comentadas abaixo **não compilam**, e é esse o ponto.
        let va = GuestVirt(0x1000);
        let pa = GuestPhys(0x1000);
        let off = ArenaOff(0x1000);

        // let _ = va == pa;        // erro: tipos diferentes
        // let _ = pa + 4;          // erro: sem Add
        // let _: GuestPhys = va;   // erro: sem From

        assert_eq!(va.0, pa.0);
        assert_eq!(off.indice(), 0x1000);
    }

    #[test]
    fn display_diz_de_que_espaco_e_o_endereco() {
        assert_eq!(GuestVirt(0x1234).to_string(), "virt:0x00001234");
        assert_eq!(GuestPhys(0x1234).to_string(), "fis:0x00001234");
        assert_eq!(ArenaOff(0x1234).to_string(), "arena+0x1234");
    }
}
