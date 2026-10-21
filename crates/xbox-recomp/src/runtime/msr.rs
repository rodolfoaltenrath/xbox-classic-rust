//! Perfil de leitura de MSRs P6 guest. Intel SDM vol. 2B (RDMSR) e vol. 4
//! tabela 2-53 (P6); relação usada pelo nxdk em lib/winapi/profiling.c.
//! Sem RDMSR host, tabela universal de zeros ou cópia de dump físico (ADR-0038).

use xbox_core::error::SnapshotError;
use xbox_core::hardware::RetailClockV1;

use super::State;
use crate::{Error, Result};

/// Perfil funcional versionado, não uma reprodução completa do silício/KPCR.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum CpuProfile {
    /// P6 nominal a 733⅓ MHz: bus 133⅓ MHz × 5,5. Demais bits de configuração
    /// do MSR 0x2A são zero por política sintética, não valores medidos no Xbox.
    XboxRetailV1 = 1,
}

impl CpuProfile {
    pub(super) fn from_id(id: u32) -> std::result::Result<Self, SnapshotError> {
        match id {
            1 => Ok(Self::XboxRetailV1),
            _ => Err(SnapshotError::BadValue {
                section: "AOTC",
                what: "perfil de CPU/MSRs AOT desconhecido",
            }),
        }
    }
}

impl State {
    /// Leitura sem efeitos: o emissor aplica EDX:EAX e cobra o passo somente se
    /// esta consulta tiver sucesso. Perfil flat/privilegiado; não há CPL/CR4/IDT.
    pub fn read_msr(&self, index: u32) -> Result<u64> {
        match (self.cpu_profile, index) {
            (CpuProfile::XboxRetailV1, 0x10) => Ok(self.clock.tsc()),
            (CpuProfile::XboxRetailV1, 0x2A) => Ok(RetailClockV1::EBL_CR_POWERON),
            _ => Err(Error::UnsupportedMsr {
                pc: self.eip,
                index,
            }),
        }
    }
}
