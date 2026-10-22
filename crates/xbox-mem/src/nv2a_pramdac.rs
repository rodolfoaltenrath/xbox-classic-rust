//! Recorte PRAMDAC: coeficiente NVPLL e frequência nominal derivada (ADR-0057).
//!
//! O reset é um perfil de compatibilidade com referência pública fixada no ADR,
//! não um dump de console. Não há oscilador, lock de PLL ou relógio independente.
use xbox_core::{
    error::SnapshotError,
    snapshot::{SnapshotReader, SnapshotWriter},
};

/// Offset na janela MMIO do NV2A.
pub const NVPLL_COEFF: u32 = 0x0068_0500;
pub const NVPLL_RESET: u32 = 0x0001_1C01;
const CRYSTAL_HZ: u64 = 16_666_666;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PramdacProfile {
    /// BUS0 v1/v2 não modelava este registrador: preserva zero com telemetria.
    LegacyUnmodeled,
    /// Fórmula nxdk e coeficiente inicial da referência xemu fixada no ADR-0056.
    PublicReferenceV1,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nv2aPramdac {
    profile: PramdacProfile,
    coefficient: u32,
}
impl Default for Nv2aPramdac {
    fn default() -> Self {
        Self {
            profile: PramdacProfile::PublicReferenceV1,
            coefficient: NVPLL_RESET,
        }
    }
}
impl Nv2aPramdac {
    pub(crate) const fn legacy() -> Self {
        Self {
            profile: PramdacProfile::LegacyUnmodeled,
            coefficient: 0,
        }
    }
    pub const fn profile(&self) -> PramdacProfile {
        self.profile
    }
    pub const fn coefficient(&self) -> Option<u32> {
        match self.profile {
            PramdacProfile::LegacyUnmodeled => None,
            PramdacProfile::PublicReferenceV1 => Some(self.coefficient),
        }
    }
    /// Frequência nominal calculada, sem avançar ou duplicar Clock da sessão.
    pub fn core_clock_hz(&self) -> Option<u64> {
        let value = self.coefficient()?;
        let m = (value & 0xFF) as u64;
        let n = ((value >> 8) & 0xFF) as u64;
        let p = (value >> 16) & 7;
        Some((CRYSTAL_HZ * n / (1u64 << p)).checked_div(m).unwrap_or(0))
    }
    /// Acessos que se sobrepõem ao registrador exigem exatamente 32 bits alinhados.
    /// False indica região sem modelo, que deve continuar indo à telemetria.
    pub(crate) fn validate_access(&self, offset: u32, len: usize) -> Result<bool, &'static str> {
        if self.profile == PramdacProfile::LegacyUnmodeled || len == 0 {
            return Ok(false);
        }
        let start = u64::from(offset);
        let end = start.saturating_add(len as u64);
        let base = u64::from(NVPLL_COEFF);
        if start >= base + 4 || end <= base {
            return Ok(false);
        }
        if offset == NVPLL_COEFF && len == 4 {
            Ok(true)
        } else {
            Err("NVPLL_COEFF exige acesso alinhado de 32 bits (ADR-0057)")
        }
    }
    pub(crate) fn write(&mut self, coefficient: u32) {
        self.coefficient = coefficient;
    }
    pub(crate) fn save(&self, out: &mut SnapshotWriter) {
        out.section("NPLL", 1);
        out.u32(match self.profile {
            PramdacProfile::LegacyUnmodeled => 0,
            PramdacProfile::PublicReferenceV1 => 1,
        });
        out.u32(self.coefficient);
    }
    pub(crate) fn load(&mut self, inp: &mut SnapshotReader<'_>) -> Result<(), SnapshotError> {
        let bad = |what| SnapshotError::BadValue {
            section: "NPLL",
            what,
        };
        if inp.section("NPLL", 1)? != 1 {
            return Err(bad("versão desconhecida"));
        }
        let profile = match inp.u32()? {
            0 => PramdacProfile::LegacyUnmodeled,
            1 => PramdacProfile::PublicReferenceV1,
            _ => return Err(bad("perfil desconhecido")),
        };
        let coefficient = inp.u32()?;
        if profile == PramdacProfile::LegacyUnmodeled && coefficient != 0 {
            return Err(bad("perfil legado não admite coeficiente programado"));
        }
        *self = Self {
            profile,
            coefficient,
        };
        Ok(())
    }
}
