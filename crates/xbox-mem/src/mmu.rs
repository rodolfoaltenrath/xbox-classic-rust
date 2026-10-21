//! A MMU do guest: tradução de [`GuestVirt`] para [`GuestPhys`].
//!
//! ## Por que a tabela é nossa, e não do guest
//!
//! No Xbox real, a MMU percorre tabelas de página que ficam na própria RAM, apontadas
//! por `CR3`. Com kernel HLE (ADR-0001), **nós somos o kernel**: as tabelas seriam
//! escritas por nós e lidas por nós. Emular o percurso custaria dois acessos à memória
//! do guest por tradução, para ler estruturas que nós mesmos montamos.
//!
//! Então a estrutura autoritativa é esta, do lado do host, com o mesmo formato de dois
//! níveis do x86 (diretório de 1024 entradas, tabelas de 1024 entradas, página grande
//! de 4 MiB no diretório). Decisão registrada no ADR-0013.
//!
//! **Limitação conhecida:** um título que leia as tabelas de página direto da RAM não
//! vai encontrá-las. Isso é raro — o caminho normal é `MmGetPhysicalAddress`, que o
//! kernel HLE implementa — e o caminho de correção está escolhido: materializar as
//! tabelas na RAM do guest quando a Fase 3c (virtualização) precisar delas de verdade.
//!
//! ## Bits de acessado e sujo
//!
//! Atualizados na tradução, inclusive em caminho `&self`, via [`std::cell::Cell`]. É
//! por isso que [`Mmu`] não é `Sync` — e não precisa ser: o guest é mono-core.

use std::cell::Cell;

use xbox_core::error::SnapshotError;
use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};

use crate::addr::{Acesso, GuestPhys, GuestVirt, LARGE_PAGE_SIZE, Perm};

/// Entradas em um diretório de páginas.
pub const ENTRADAS_DIRETORIO: usize = 1024;
/// Entradas em uma tabela de páginas.
pub const ENTRADAS_TABELA: usize = 1024;

/// Bit de presença.
const PRESENTE: u8 = 0b0000_1000;
/// Bit de acessado.
const ACESSADO: u8 = 0b0001_0000;
/// Bit de sujo.
const SUJO: u8 = 0b0010_0000;
/// Máscara das permissões (ver [`Perm`]).
const MASCARA_PERM: u8 = 0b0000_0111;

/// Por que uma tradução ou um acesso falhou.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotivoDeFalta {
    /// Não existe mapeamento para este endereço virtual.
    NaoMapeado,
    /// Existe mapeamento, mas ele não permite este acesso.
    SemPermissao(Perm),
    /// O físico resultante não cai em nenhuma região conhecida.
    RegiaoInvalida(GuestPhys),
    /// O físico cai em região reconhecida mas deliberadamente não implementada.
    NaoImplementado {
        /// Endereço físico alcançado.
        fisico: GuestPhys,
        /// O que falta implementar, e em que fase.
        o_que: &'static str,
    },
    /// A faixa pedida transborda o espaço de endereçamento.
    Transbordo,
}

/// Uma falha de acesso à memória do guest.
///
/// Não é erro do emulador: é evento normal de execução, que a CPU traduz em `#PF`
/// quando [`Falta::e_falha_de_pagina`] for verdadeiro.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Falta {
    /// Endereço virtual que falhou. Vai para `CR2`.
    pub endereco: GuestVirt,
    /// Que tipo de acesso foi tentado.
    pub acesso: Acesso,
    /// Por que falhou.
    pub motivo: MotivoDeFalta,
}

impl Falta {
    /// Esta falha corresponde a uma `#PF` que o guest deveria ver?
    ///
    /// `NaoMapeado` e `SemPermissao` são falhas que o hardware real produziria.
    /// As outras são limites do emulador, e a CPU deve tratá-las como erro de execução,
    /// não entregar ao guest uma exceção que o console não daria.
    pub const fn e_falha_de_pagina(&self) -> bool {
        matches!(
            self.motivo,
            MotivoDeFalta::NaoMapeado | MotivoDeFalta::SemPermissao(_)
        )
    }

    /// Código de erro de `#PF`, no formato do x86.
    ///
    /// Bit 0 (P): o mapeamento existia. Bit 1 (W/R): foi escrita. Bit 2 (U/S): modo
    /// usuário — sempre 0, porque o título do Xbox roda em modo supervisor. Bit 4
    /// (I/D): busca de instrução.
    pub const fn codigo_de_erro(&self) -> u32 {
        let mut c = 0;
        if matches!(self.motivo, MotivoDeFalta::SemPermissao(_)) {
            c |= 1 << 0;
        }
        if self.acesso.e_escrita() {
            c |= 1 << 1;
        }
        if self.acesso.e_execucao() {
            c |= 1 << 4;
        }
        c
    }
}

impl std::fmt::Display for Falta {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "falta em {} ({:?}): ", self.endereco, self.acesso)?;
        match self.motivo {
            MotivoDeFalta::NaoMapeado => write!(f, "endereço não mapeado"),
            MotivoDeFalta::SemPermissao(p) => {
                write!(f, "página é {}, acesso não permitido", p.rwx())
            }
            MotivoDeFalta::RegiaoInvalida(pa) => write!(f, "{pa} fora do mapa físico"),
            MotivoDeFalta::NaoImplementado { fisico, o_que } => {
                write!(f, "{fisico} em região não implementada ({o_que})")
            }
            MotivoDeFalta::Transbordo => write!(f, "faixa transborda o espaço de 32 bits"),
        }
    }
}

impl std::error::Error for Falta {}

impl From<Falta> for xbox_core::Error {
    /// Converte uma falta do guest em erro do emulador.
    ///
    /// Existe para que código de fora do laço de execução (loader, kernel HLE,
    /// ferramentas) use `?` normalmente. A CPU **não** usa esta conversão: para ela a
    /// falta é controle de fluxo, que vira `#PF` quando
    /// [`Falta::e_falha_de_pagina`] for verdadeiro.
    fn from(f: Falta) -> Self {
        Self::format(f.to_string())
    }
}

/// Uma entrada de tabela de páginas.
#[derive(Debug, Clone)]
struct Entrada {
    /// Frame físico de 4 KiB.
    frame: u32,
    /// Permissões e bits de estado.
    flags: Cell<u8>,
}

impl Entrada {
    const fn ausente() -> Self {
        Self {
            frame: 0,
            flags: Cell::new(0),
        }
    }

    fn nova(frame: u32, perm: Perm) -> Self {
        Self {
            frame,
            flags: Cell::new(PRESENTE | (perm.0 & MASCARA_PERM)),
        }
    }

    fn presente(&self) -> bool {
        self.flags.get() & PRESENTE != 0
    }

    fn perm(&self) -> Perm {
        Perm(self.flags.get() & MASCARA_PERM)
    }

    fn marcar(&self, acesso: Acesso) {
        let mut f = self.flags.get() | ACESSADO;
        if acesso.e_escrita() {
            f |= SUJO;
        }
        self.flags.set(f);
    }

    fn sujo(&self) -> bool {
        self.flags.get() & SUJO != 0
    }

    fn acessado(&self) -> bool {
        self.flags.get() & ACESSADO != 0
    }
}

/// Uma entrada do diretório.
#[derive(Debug, Clone)]
enum EntradaDeDiretorio {
    /// Nada mapeado nestes 4 MiB.
    Ausente,
    /// Página grande de 4 MiB.
    Grande(Entrada),
    /// Tabela de 1024 páginas de 4 KiB.
    Tabela(Vec<Entrada>),
}

/// A unidade de gerenciamento de memória do guest.
#[derive(Debug, Clone)]
pub struct Mmu {
    diretorio: Vec<EntradaDeDiretorio>,
}

impl Default for Mmu {
    fn default() -> Self {
        Self::nova()
    }
}

impl Mmu {
    /// MMU sem nenhum mapeamento.
    pub fn nova() -> Self {
        Self {
            diretorio: (0..ENTRADAS_DIRETORIO)
                .map(|_| EntradaDeDiretorio::Ausente)
                .collect(),
        }
    }

    /// Mapeia `paginas` páginas de 4 KiB de `va` para `pa`.
    ///
    /// Erra se `va` ou `pa` não estiverem alinhados em página, ou se a faixa
    /// transbordar o espaço de 32 bits.
    pub fn mapear(
        &mut self,
        va: GuestVirt,
        pa: GuestPhys,
        paginas: u32,
        perm: Perm,
    ) -> Result<(), Falta> {
        if va.deslocamento() != 0 || pa.deslocamento() != 0 {
            return Err(Falta {
                endereco: va,
                acesso: Acesso::Escrita,
                motivo: MotivoDeFalta::Transbordo,
            });
        }
        for i in 0..paginas {
            let bytes = i.checked_mul(4096).ok_or(Falta {
                endereco: va,
                acesso: Acesso::Escrita,
                motivo: MotivoDeFalta::Transbordo,
            })?;
            let v = va.checked_add(bytes).ok_or(Falta {
                endereco: va,
                acesso: Acesso::Escrita,
                motivo: MotivoDeFalta::Transbordo,
            })?;
            let p = pa.checked_add(bytes).ok_or(Falta {
                endereco: va,
                acesso: Acesso::Escrita,
                motivo: MotivoDeFalta::Transbordo,
            })?;
            self.mapear_pagina(v, p, perm);
        }
        Ok(())
    }

    fn mapear_pagina(&mut self, va: GuestVirt, pa: GuestPhys, perm: Perm) {
        let d = va.indice_diretorio();
        if !matches!(self.diretorio[d], EntradaDeDiretorio::Tabela(_)) {
            let tabela = (0..ENTRADAS_TABELA).map(|_| Entrada::ausente()).collect();
            self.diretorio[d] = EntradaDeDiretorio::Tabela(tabela);
        }
        if let EntradaDeDiretorio::Tabela(t) = &mut self.diretorio[d] {
            t[va.indice_tabela()] = Entrada::nova(pa.frame(), perm);
        }
    }

    /// Mapeia uma página grande de 4 MiB.
    ///
    /// É assim que a janela contígua `0x80000000` é montada: 16 páginas grandes cobrem
    /// os 64 MiB de RAM, em vez de 16 mil entradas de 4 KiB.
    pub fn mapear_grande(&mut self, va: GuestVirt, pa: GuestPhys, perm: Perm) -> Result<(), Falta> {
        let falta = |motivo| Falta {
            endereco: va,
            acesso: Acesso::Escrita,
            motivo,
        };
        if !va.0.is_multiple_of(LARGE_PAGE_SIZE as u32)
            || !pa.0.is_multiple_of(LARGE_PAGE_SIZE as u32)
        {
            return Err(falta(MotivoDeFalta::Transbordo));
        }
        self.diretorio[va.indice_diretorio()] =
            EntradaDeDiretorio::Grande(Entrada::nova(pa.frame(), perm));
        Ok(())
    }

    /// Remove `paginas` páginas de 4 KiB a partir de `va`.
    pub fn desmapear(&mut self, va: GuestVirt, paginas: u32) {
        for i in 0..paginas {
            let Some(v) = va.checked_add(i.saturating_mul(4096)) else {
                return;
            };
            let d = v.indice_diretorio();
            match &mut self.diretorio[d] {
                EntradaDeDiretorio::Tabela(t) => t[v.indice_tabela()] = Entrada::ausente(),
                EntradaDeDiretorio::Grande(_) => {
                    self.diretorio[d] = EntradaDeDiretorio::Ausente;
                }
                EntradaDeDiretorio::Ausente => {}
            }
        }
    }

    /// Traduz um endereço virtual, verificando a permissão e marcando os bits de
    /// acessado e sujo.
    pub fn traduzir(&self, va: GuestVirt, acesso: Acesso) -> Result<GuestPhys, Falta> {
        let entrada = self.entrada(va).ok_or(Falta {
            endereco: va,
            acesso,
            motivo: MotivoDeFalta::NaoMapeado,
        })?;
        let (e, grande) = entrada;
        if !e.perm().permite(acesso) {
            return Err(Falta {
                endereco: va,
                acesso,
                motivo: MotivoDeFalta::SemPermissao(e.perm()),
            });
        }
        e.marcar(acesso);
        Ok(Self::fisico(e, grande, va))
    }

    /// Traduz sem verificar permissão nem marcar bits.
    ///
    /// Para diagnóstico e para o `MmGetPhysicalAddress` do kernel, que responde o
    /// endereço físico independentemente de o chamador poder escrever ali.
    pub fn consultar(&self, va: GuestVirt) -> Option<(GuestPhys, Perm)> {
        let (e, grande) = self.entrada(va)?;
        Some((Self::fisico(e, grande, va), e.perm()))
    }

    /// Páginas mapeadas, inclusive subdivisões das páginas grandes, sem marcar
    /// Accessed/Dirty. Permite conferir a posse física ao restaurar uma sessão.
    pub fn mapeamentos(&self) -> impl Iterator<Item = (GuestVirt, GuestPhys, Perm)> + '_ {
        self.diretorio
            .iter()
            .enumerate()
            .filter(|(_, entry)| !matches!(entry, EntradaDeDiretorio::Ausente))
            .flat_map(move |(directory, _)| {
                (0..1024u32).filter_map(move |table| {
                    let va = GuestVirt(((directory as u32) << 22) | (table << 12));
                    self.consultar(va).map(|(pa, perm)| (va, pa, perm))
                })
            })
    }

    fn entrada(&self, va: GuestVirt) -> Option<(&Entrada, bool)> {
        match &self.diretorio[va.indice_diretorio()] {
            EntradaDeDiretorio::Grande(e) if e.presente() => Some((e, true)),
            EntradaDeDiretorio::Ausente | EntradaDeDiretorio::Grande(_) => None,
            EntradaDeDiretorio::Tabela(t) => {
                let e = &t[va.indice_tabela()];
                if e.presente() { Some((e, false)) } else { None }
            }
        }
    }

    fn fisico(e: &Entrada, grande: bool, va: GuestVirt) -> GuestPhys {
        if grande {
            GuestPhys(GuestPhys::do_frame(e.frame).0 + va.deslocamento_grande())
        } else {
            GuestPhys(GuestPhys::do_frame(e.frame).0 + va.deslocamento())
        }
    }

    /// A página que contém `va` está suja?
    pub fn sujo(&self, va: GuestVirt) -> bool {
        self.entrada(va).is_some_and(|(e, _)| e.sujo())
    }

    /// A página que contém `va` foi acessada?
    pub fn acessado(&self, va: GuestVirt) -> bool {
        self.entrada(va).is_some_and(|(e, _)| e.acessado())
    }

    /// Limpa o bit de sujo de uma página, devolvendo o valor anterior.
    ///
    /// A GPU (Fase 5) e o JIT (Fase 3b) usam isto para saber o que mudou desde a última
    /// vez que olharam.
    pub fn limpar_sujo(&self, va: GuestVirt) -> bool {
        match self.entrada(va) {
            Some((e, _)) => {
                let antes = e.sujo();
                e.flags.set(e.flags.get() & !SUJO);
                antes
            }
            None => false,
        }
    }

    /// Quantas páginas de 4 KiB estão mapeadas (página grande conta como 1024).
    pub fn paginas_mapeadas(&self) -> usize {
        self.diretorio
            .iter()
            .map(|d| match d {
                EntradaDeDiretorio::Grande(e) if e.presente() => ENTRADAS_TABELA,
                EntradaDeDiretorio::Ausente | EntradaDeDiretorio::Grande(_) => 0,
                EntradaDeDiretorio::Tabela(t) => t.iter().filter(|e| e.presente()).count(),
            })
            .sum()
    }
}

impl Snapshot for Mmu {
    fn save(&self, out: &mut SnapshotWriter) {
        out.section("MMU0", 1);

        // Grandes primeiro, depois as de 4 KiB: ordem fixa, derivada da estrutura, o
        // que mantém o snapshot determinístico (ADR-0004).
        let mut grandes = Vec::new();
        let mut pequenas = Vec::new();
        for (d, entrada) in self.diretorio.iter().enumerate() {
            match entrada {
                EntradaDeDiretorio::Grande(e) if e.presente() => {
                    grandes.push((d as u32, e.frame, e.flags.get()));
                }
                EntradaDeDiretorio::Ausente | EntradaDeDiretorio::Grande(_) => {}
                EntradaDeDiretorio::Tabela(t) => {
                    for (i, e) in t.iter().enumerate() {
                        if e.presente() {
                            pequenas.push((d as u32, i as u32, e.frame, e.flags.get()));
                        }
                    }
                }
            }
        }

        out.u64(grandes.len() as u64);
        for (d, frame, flags) in grandes {
            out.u32(d);
            out.u32(frame);
            out.u8(flags);
        }
        out.u64(pequenas.len() as u64);
        for (d, i, frame, flags) in pequenas {
            out.u32(d);
            out.u32(i);
            out.u32(frame);
            out.u8(flags);
        }
    }

    fn load(&mut self, inp: &mut SnapshotReader<'_>) -> Result<(), SnapshotError> {
        let _v = inp.section("MMU0", 1)?;
        *self = Self::nova();

        let grandes = inp.u64()?;
        for _ in 0..grandes {
            let d = inp.u32()? as usize;
            let frame = inp.u32()?;
            let flags = inp.u8()?;
            if d >= ENTRADAS_DIRETORIO || frame > (u32::MAX >> 12) || !frame.is_multiple_of(1024) {
                return Err(SnapshotError::BadValue {
                    section: "MMU0",
                    what: "índice, frame ou alinhamento de página grande inválido",
                });
            }
            self.diretorio[d] = EntradaDeDiretorio::Grande(Entrada {
                frame,
                flags: Cell::new(flags),
            });
        }

        let pequenas = inp.u64()?;
        for _ in 0..pequenas {
            let d = inp.u32()? as usize;
            let i = inp.u32()? as usize;
            let frame = inp.u32()?;
            let flags = inp.u8()?;
            if d >= ENTRADAS_DIRETORIO || i >= ENTRADAS_TABELA || frame > (u32::MAX >> 12) {
                return Err(SnapshotError::BadValue {
                    section: "MMU0",
                    what: "índice ou frame de página fora da faixa",
                });
            }
            if !matches!(self.diretorio[d], EntradaDeDiretorio::Tabela(_)) {
                let tabela = (0..ENTRADAS_TABELA).map(|_| Entrada::ausente()).collect();
                self.diretorio[d] = EntradaDeDiretorio::Tabela(tabela);
            }
            if let EntradaDeDiretorio::Tabela(t) = &mut self.diretorio[d] {
                t[i] = Entrada {
                    frame,
                    flags: Cell::new(flags),
                };
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Falta, Mmu, MotivoDeFalta};
    use crate::addr::{Acesso, GuestPhys, GuestVirt, Perm};
    use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};

    #[test]
    fn traduz_pagina_simples() {
        let mut m = Mmu::nova();
        m.mapear(GuestVirt(0x0001_0000), GuestPhys(0x0020_0000), 1, Perm::RW)
            .unwrap();

        assert_eq!(
            m.traduzir(GuestVirt(0x0001_0000), Acesso::Leitura),
            Ok(GuestPhys(0x0020_0000))
        );
        // O deslocamento dentro da página é preservado.
        assert_eq!(
            m.traduzir(GuestVirt(0x0001_0ABC), Acesso::Leitura),
            Ok(GuestPhys(0x0020_0ABC))
        );
        // A página seguinte não foi mapeada.
        assert!(m.traduzir(GuestVirt(0x0001_1000), Acesso::Leitura).is_err());
    }

    #[test]
    fn pagina_grande_cobre_quatro_megas() {
        let mut m = Mmu::nova();
        m.mapear_grande(GuestVirt(0x8000_0000), GuestPhys(0), Perm::RW)
            .unwrap();

        assert_eq!(
            m.traduzir(GuestVirt(0x8000_0000), Acesso::Leitura),
            Ok(GuestPhys(0))
        );
        assert_eq!(
            m.traduzir(GuestVirt(0x8030_0000), Acesso::Leitura),
            Ok(GuestPhys(0x0030_0000))
        );
        assert_eq!(
            m.traduzir(GuestVirt(0x803F_FFFF), Acesso::Leitura),
            Ok(GuestPhys(0x003F_FFFF))
        );
        // 4 MiB adiante já é outra entrada de diretório.
        assert!(m.traduzir(GuestVirt(0x8040_0000), Acesso::Leitura).is_err());
        assert_eq!(m.paginas_mapeadas(), 1024);
    }

    #[test]
    fn alinhamento_e_exigido_no_mapeamento() {
        let mut m = Mmu::nova();
        assert!(
            m.mapear(GuestVirt(0x1001), GuestPhys(0), 1, Perm::RW)
                .is_err()
        );
        assert!(
            m.mapear(GuestVirt(0x1000), GuestPhys(1), 1, Perm::RW)
                .is_err()
        );
        assert!(
            m.mapear_grande(GuestVirt(0x8000_1000), GuestPhys(0), Perm::RW)
                .is_err(),
            "página grande exige alinhamento de 4 MiB"
        );
    }

    #[test]
    fn permissao_e_verificada_e_gera_codigo_de_erro_do_x86() {
        let mut m = Mmu::nova();
        m.mapear(GuestVirt(0x2000), GuestPhys(0x5000), 1, Perm::RO)
            .unwrap();

        assert!(m.traduzir(GuestVirt(0x2000), Acesso::Leitura).is_ok());

        let falta = m.traduzir(GuestVirt(0x2000), Acesso::Escrita).unwrap_err();
        assert_eq!(falta.motivo, MotivoDeFalta::SemPermissao(Perm::RO));
        assert!(falta.e_falha_de_pagina());
        // P=1 (existia) e W=1 (escrita) → 0b11
        assert_eq!(falta.codigo_de_erro(), 0b11);

        let falta = m.traduzir(GuestVirt(0x2000), Acesso::Execucao).unwrap_err();
        // P=1 e I/D=1 → 0b10001
        assert_eq!(falta.codigo_de_erro(), 0b1_0001);
    }

    #[test]
    fn nao_mapeado_tem_bit_de_presenca_zero() {
        let m = Mmu::nova();
        let falta = m.traduzir(GuestVirt(0x1234), Acesso::Leitura).unwrap_err();
        assert_eq!(falta.motivo, MotivoDeFalta::NaoMapeado);
        assert_eq!(
            falta.endereco,
            GuestVirt(0x1234),
            "CR2 precisa ser o endereço pedido"
        );
        assert_eq!(falta.codigo_de_erro(), 0, "P=0 em página ausente");
    }

    #[test]
    fn bits_de_acessado_e_sujo() {
        let mut m = Mmu::nova();
        let va = GuestVirt(0x3000);
        m.mapear(va, GuestPhys(0x9000), 1, Perm::RW).unwrap();

        assert!(!m.acessado(va));
        assert!(!m.sujo(va));

        m.traduzir(va, Acesso::Leitura).unwrap();
        assert!(m.acessado(va), "leitura marca acessado");
        assert!(!m.sujo(va), "leitura não marca sujo");

        m.traduzir(va, Acesso::Escrita).unwrap();
        assert!(m.sujo(va), "escrita marca sujo");

        assert!(m.limpar_sujo(va), "limpar devolve o valor anterior");
        assert!(!m.sujo(va));
        assert!(!m.limpar_sujo(va));
    }

    #[test]
    fn consultar_ignora_permissao() {
        let mut m = Mmu::nova();
        m.mapear(GuestVirt(0x4000), GuestPhys(0x7000), 1, Perm::RO)
            .unwrap();
        let (pa, perm) = m.consultar(GuestVirt(0x4010)).unwrap();
        assert_eq!(pa, GuestPhys(0x7010));
        assert_eq!(perm, Perm::RO);
        assert!(m.consultar(GuestVirt(0x5000)).is_none());
    }

    #[test]
    fn desmapear_remove_o_mapeamento() {
        let mut m = Mmu::nova();
        m.mapear(GuestVirt(0x6000), GuestPhys(0x1000), 2, Perm::RW)
            .unwrap();
        assert_eq!(m.paginas_mapeadas(), 2);

        m.desmapear(GuestVirt(0x6000), 1);
        assert_eq!(m.paginas_mapeadas(), 1);
        assert!(m.traduzir(GuestVirt(0x6000), Acesso::Leitura).is_err());
        assert!(m.traduzir(GuestVirt(0x7000), Acesso::Leitura).is_ok());
    }

    #[test]
    fn duas_janelas_virtuais_para_o_mesmo_fisico() {
        // É o arranjo real do Xbox: a imagem do título em 0x00010000 e a janela
        // contígua em 0x80000000 apontando para a mesma RAM.
        let mut m = Mmu::nova();
        m.mapear(GuestVirt(0x0001_0000), GuestPhys(0x0001_0000), 1, Perm::RWX)
            .unwrap();
        m.mapear_grande(GuestVirt(0x8000_0000), GuestPhys(0), Perm::RW)
            .unwrap();

        let a = m.traduzir(GuestVirt(0x0001_0100), Acesso::Leitura).unwrap();
        let b = m.traduzir(GuestVirt(0x8001_0100), Acesso::Leitura).unwrap();
        assert_eq!(a, b, "as duas janelas têm de chegar no mesmo físico");
    }

    #[test]
    fn mapeamento_que_transborda_e_recusado() {
        let mut m = Mmu::nova();
        let r = m.mapear(GuestVirt(0xFFFF_F000), GuestPhys(0), 2, Perm::RW);
        assert!(matches!(
            r,
            Err(Falta {
                motivo: MotivoDeFalta::Transbordo,
                ..
            })
        ));
    }

    #[test]
    fn snapshot_ida_e_volta_e_deterministico() {
        let mut m = Mmu::nova();
        m.mapear(GuestVirt(0x0001_0000), GuestPhys(0x0010_0000), 4, Perm::RX)
            .unwrap();
        m.mapear(GuestVirt(0x0002_0000), GuestPhys(0x0020_0000), 2, Perm::RW)
            .unwrap();
        m.mapear_grande(GuestVirt(0x8000_0000), GuestPhys(0), Perm::RW)
            .unwrap();
        m.traduzir(GuestVirt(0x0002_0000), Acesso::Escrita).unwrap();

        let mut w1 = SnapshotWriter::new();
        m.save(&mut w1);
        let bytes = w1.finish();

        let mut restaurada = Mmu::nova();
        let mut r = SnapshotReader::new(&bytes).unwrap();
        restaurada.load(&mut r).unwrap();

        assert_eq!(restaurada.paginas_mapeadas(), m.paginas_mapeadas());
        assert_eq!(
            restaurada.consultar(GuestVirt(0x0001_2345)),
            m.consultar(GuestVirt(0x0001_2345))
        );
        assert!(
            restaurada.sujo(GuestVirt(0x0002_0000)),
            "bit sujo precisa sobreviver"
        );

        let mut w2 = SnapshotWriter::new();
        restaurada.save(&mut w2);
        assert_eq!(
            bytes,
            w2.finish(),
            "snapshot não é estável após ida e volta"
        );
    }

    #[test]
    fn snapshot_com_indice_ou_frame_invalido_e_recusado() {
        let mut w = SnapshotWriter::new();
        w.section("MMU0", 1);
        w.u64(1);
        w.u32(9999); // índice de diretório absurdo
        w.u32(0);
        w.u8(0b1000);
        let bytes = w.finish();

        let mut m = Mmu::nova();
        let mut r = SnapshotReader::new(&bytes).unwrap();
        assert!(m.load(&mut r).is_err());
        // Sem estes limites, consultar uma página grande corrompida poderia
        // transbordar ao enumerar seus endereços físicos durante a restauração.
        for (large, frame) in [(true, 1u32), (true, u32::MAX), (false, u32::MAX)] {
            let mut w = SnapshotWriter::new();
            w.section("MMU0", 1);
            if large {
                w.u64(1);
                w.u32(0);
                w.u32(frame);
                w.u8(0b1000);
                w.u64(0);
            } else {
                w.u64(0);
                w.u64(1);
                w.u32(0);
                w.u32(0);
                w.u32(frame);
                w.u8(0b1000);
            }
            assert!(
                m.load(&mut SnapshotReader::new(&w.finish()).unwrap())
                    .is_err()
            );
        }
    }
}
