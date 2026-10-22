//! Os quatro registradores do NV2A que formam a linha de interrupção do vblank.
//!
//! Isto **não** é o NV2A: é o recorte mínimo que o `pbkit` do nxdk lê e escreve para
//! tratar o vblank (ADR-0055). Todo outro registrador do dispositivo continua sem modelo,
//! lido como zero declarado e registrado pela telemetria.
//!
//! | Offset | Registrador | Modelo |
//! |---|---|---|
//! | `0x000100` | `PMC_INTR_0` | derivado: bit 24 (PCRTC) liga quando há vblank pendente **e** habilitado |
//! | `0x000140` | `PMC_INTR_EN_0` | guardado; bit 0 = INTA por hardware |
//! | `0x600100` | `PCRTC_INTR` | bit 0 = vblank pendente; escrita de 1 limpa (write-1-to-clear) |
//! | `0x600140` | `PCRTC_INTR_EN` | guardado; bit 0 = vblank habilitado |
//!
//! O estado de pendência trava independente da habilitação — a habilitação só decide se a
//! pendência chega ao PMC. É por isso que o `pbkit` limpa `PCRTC_INTR` *antes* de ligar
//! `PCRTC_INTR_EN`.
//!
//! **Só acessos de 32 bits** a estes quatro registradores são modelados. Um acesso de
//! outra largura cai na telemetria como qualquer registrador sem modelo, em vez de ganhar
//! uma semântica parcial inventada.

use xbox_core::error::SnapshotError;
use xbox_core::snapshot::{SnapshotReader, SnapshotWriter};

const PMC_INTR_0: u32 = 0x0000_0100;
const PMC_INTR_EN_0: u32 = 0x0000_0140;
const PCRTC_INTR: u32 = 0x0060_0100;
const PCRTC_INTR_EN: u32 = 0x0060_0140;

/// `NV_PMC_INTR_0_PCRTC_PENDING`.
pub const PMC_PCRTC_PENDING: u32 = 0x0100_0000;
/// `PCRTC_INTR_VBLANK`: bit de vblank em `PCRTC_INTR` e `PCRTC_INTR_EN`.
pub const PCRTC_VBLANK: u32 = 1;
/// `NV_PMC_INTR_EN_0_INTA_HARDWARE`.
pub const PMC_INTA_HARDWARE: u32 = 1;

/// Estado dos registradores de interrupção de vblank.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Nv2aIrq {
    pcrtc_intr: u32,
    pcrtc_intr_en: u32,
    pmc_intr_en: u32,
}

impl Nv2aIrq {
    /// Valor de um registrador de 32 bits, ou `None` se este módulo não o modela.
    pub const fn read(&self, offset: u32) -> Option<u32> {
        Some(match offset {
            PMC_INTR_0 => self.pmc_intr_0(),
            PMC_INTR_EN_0 => self.pmc_intr_en,
            PCRTC_INTR => self.pcrtc_intr,
            PCRTC_INTR_EN => self.pcrtc_intr_en,
            _ => return None,
        })
    }

    /// Aplica uma escrita de 32 bits; `false` se o registrador não é modelado aqui.
    pub const fn write(&mut self, offset: u32, value: u32) -> bool {
        match offset {
            PMC_INTR_EN_0 => self.pmc_intr_en = value & 3,
            // Write-1-to-clear.
            PCRTC_INTR => self.pcrtc_intr &= !value,
            PCRTC_INTR_EN => self.pcrtc_intr_en = value & PCRTC_VBLANK,
            _ => return false,
        }
        true
    }

    const fn pmc_intr_0(&self) -> u32 {
        if self.pcrtc_intr & self.pcrtc_intr_en & PCRTC_VBLANK != 0 {
            PMC_PCRTC_PENDING
        } else {
            0
        }
    }

    /// O vblank aconteceu: trava a pendência (vários vblanks sem tratamento viram um só).
    pub const fn raise_vblank(&mut self) {
        self.pcrtc_intr |= PCRTC_VBLANK;
    }

    /// A linha de interrupção para a CPU está ligada?
    pub const fn line_asserted(&self) -> bool {
        self.pmc_intr_en & PMC_INTA_HARDWARE != 0 && self.pmc_intr_0() != 0
    }

    /// Alguém habilitou o vblank até a linha? Sem isso, esperar um vblank não termina.
    pub const fn vblank_armed(&self) -> bool {
        self.pcrtc_intr_en & PCRTC_VBLANK != 0 && self.pmc_intr_en & PMC_INTA_HARDWARE != 0
    }

    pub(crate) fn save(&self, out: &mut SnapshotWriter) {
        out.u32(self.pcrtc_intr);
        out.u32(self.pcrtc_intr_en);
        out.u32(self.pmc_intr_en);
    }

    pub(crate) fn load(&mut self, inp: &mut SnapshotReader<'_>) -> Result<(), SnapshotError> {
        let bad = |what| SnapshotError::BadValue {
            section: "BUS0",
            what,
        };
        let pcrtc_intr = inp.u32()?;
        let pcrtc_intr_en = inp.u32()?;
        let pmc_intr_en = inp.u32()?;
        if pcrtc_intr & !PCRTC_VBLANK != 0
            || pcrtc_intr_en & !PCRTC_VBLANK != 0
            || pmc_intr_en & !3 != 0
        {
            return Err(bad("bits de interrupção do NV2A sem modelo"));
        }
        *self = Self {
            pcrtc_intr,
            pcrtc_intr_en,
            pmc_intr_en,
        };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vblank_so_chega_a_linha_com_habilitacao_nos_dois_niveis() {
        let mut irq = Nv2aIrq::default();
        irq.raise_vblank();
        assert_eq!(
            irq.read(PCRTC_INTR),
            Some(1),
            "pendência trava sem habilitação"
        );
        assert_eq!(irq.read(PMC_INTR_0), Some(0));
        assert!(!irq.line_asserted());

        irq.write(PCRTC_INTR_EN, 1);
        assert_eq!(irq.read(PMC_INTR_0), Some(PMC_PCRTC_PENDING));
        assert!(!irq.line_asserted(), "falta INTA no PMC");
        irq.write(PMC_INTR_EN_0, 1);
        assert!(irq.line_asserted());
    }

    #[test]
    fn escrever_um_limpa_a_pendencia_e_zero_nao_faz_nada() {
        let mut irq = Nv2aIrq::default();
        irq.raise_vblank();
        irq.write(PCRTC_INTR, 0);
        assert_eq!(irq.read(PCRTC_INTR), Some(1));
        irq.write(PCRTC_INTR, 1);
        assert_eq!(irq.read(PCRTC_INTR), Some(0));
    }

    #[test]
    fn desabilitar_inta_apaga_a_linha_sem_perder_a_pendencia() {
        let mut irq = Nv2aIrq::default();
        irq.write(PCRTC_INTR_EN, 1);
        irq.write(PMC_INTR_EN_0, 1);
        irq.raise_vblank();
        assert!(irq.line_asserted());
        irq.write(PMC_INTR_EN_0, 0);
        assert!(!irq.line_asserted());
        assert_eq!(irq.read(PCRTC_INTR), Some(1));
        irq.write(PMC_INTR_EN_0, 1);
        assert!(irq.line_asserted(), "reabilitar religa a mesma pendência");
    }

    #[test]
    fn registrador_fora_do_recorte_nao_e_modelado() {
        let mut irq = Nv2aIrq::default();
        for offset in [0, 0x104, 0x144, 0x600104, 0x681_3C8] {
            assert_eq!(irq.read(offset), None);
            assert!(!irq.write(offset, 1));
        }
        // PMC_INTR_0 é derivado: escrever nele não é modelado.
        assert!(!irq.write(PMC_INTR_0, 1));
    }

    #[test]
    fn snapshot_ida_e_volta_e_recusa_bits_sem_modelo() {
        let mut irq = Nv2aIrq::default();
        irq.write(PCRTC_INTR_EN, 1);
        irq.write(PMC_INTR_EN_0, 1);
        irq.raise_vblank();
        let mut out = SnapshotWriter::new();
        irq.save(&mut out);
        let bytes = out.finish();
        let mut restored = Nv2aIrq::default();
        restored
            .load(&mut SnapshotReader::new(&bytes).unwrap())
            .unwrap();
        assert_eq!(restored, irq);
    }
}
