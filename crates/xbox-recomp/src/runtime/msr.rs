//! Perfil de leitura de MSRs P6 guest. Intel SDM vol. 2B (RDMSR) e vol. 4
//! tabela 2-53 (P6); relação usada pelo nxdk em lib/winapi/profiling.c.
//! Sem RDMSR host, tabela universal de zeros ou cópia de dump físico (ADR-0038).

use xbox_core::error::SnapshotError;

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

// EBL_CR_POWERON: [19:18]=01 (bus 133 MHz), [25:22]=0100 (ratio 5,5),
// [27]=0. Demais bits são zero neste perfil; não modelamos seus efeitos/RW.
const EBL_CR_POWERON: u64 = 0x0104_0000;
// Bus nominal = 400 MHz/3, ratio = 11/2. Evitar arredondar o bus antes da razão.
// O mesmo domínio nominal deve sustentar Clock, RDTSC e MSR_TSC.
const _: () = assert!(400_000_000u64 * 11 / 6 == xbox_core::time::CPU_HZ);

impl State {
    /// Leitura sem efeitos: o emissor aplica EDX:EAX e cobra o passo somente se
    /// esta consulta tiver sucesso. Perfil flat/privilegiado; não há CPL/CR4/IDT.
    pub fn read_msr(&self, index: u32) -> Result<u64> {
        match (self.cpu_profile, index) {
            (CpuProfile::XboxRetailV1, 0x10) => Ok(self.clock.tsc()),
            (CpuProfile::XboxRetailV1, 0x2A) => Ok(EBL_CR_POWERON),
            _ => Err(Error::UnsupportedMsr {
                pc: self.eip,
                index,
            }),
        }
    }
}
