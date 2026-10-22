//! Configuração de memória PFB com perfil de firmware público (ADR-0058).
//!
//! Valores programados pelo Cromwell, não reset físico medido. O recorte guarda
//! CFG0/CFG1; não implementa geometria elétrica, tiling, PRAMIN ou comandos GPU.
use xbox_core::{
    error::SnapshotError,
    snapshot::{SnapshotReader, SnapshotWriter},
};

pub const PFB_CFG0: u32 = 0x0010_0200;
pub const PFB_CFG1: u32 = 0x0010_0204;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PfbProfile {
    /// BUS0 v1–v3 ou arena sem tamanho de console: zero com telemetria.
    LegacyUnmodeled,
    /// Inicialização Cromwell: CFG0 depende de 64/128 MiB; CFG1=0x11448000.
    FirmwareMemoryV1,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nv2aPfb {
    profile: PfbProfile,
    cfg0: u32,
    cfg1: u32,
}
impl Nv2aPfb {
    pub(crate) const fn for_ram(bytes: usize) -> Self {
        let cfg0 = match bytes {
            0x0400_0000 => 0x0307_0003,
            0x0800_0000 => 0x0307_0103,
            _ => return Self::legacy(),
        };
        Self {
            profile: PfbProfile::FirmwareMemoryV1,
            cfg0,
            cfg1: 0x1144_8000,
        }
    }
    pub(crate) const fn legacy() -> Self {
        Self {
            profile: PfbProfile::LegacyUnmodeled,
            cfg0: 0,
            cfg1: 0,
        }
    }
    pub const fn profile(&self) -> PfbProfile {
        self.profile
    }
    /// Estado autoritativo compartilhado por VA, aliases e acesso físico.
    pub const fn read(&self, offset: u32) -> Option<u32> {
        if matches!(self.profile, PfbProfile::LegacyUnmodeled) {
            return None;
        }
        match offset {
            PFB_CFG0 => Some(self.cfg0),
            PFB_CFG1 => Some(self.cfg1),
            _ => None,
        }
    }
    pub(crate) fn validate_access(&self, offset: u32, len: usize) -> Result<bool, &'static str> {
        if self.profile == PfbProfile::LegacyUnmodeled || len == 0 {
            return Ok(false);
        }
        let start = u64::from(offset);
        let end = start.saturating_add(len as u64);
        if start >= u64::from(PFB_CFG1) + 4 || end <= u64::from(PFB_CFG0) {
            return Ok(false);
        }
        if matches!(offset, PFB_CFG0 | PFB_CFG1) && len == 4 {
            Ok(true)
        } else {
            Err("PFB_CFG0/CFG1 exigem acesso alinhado de 32 bits (ADR-0058)")
        }
    }
    pub(crate) fn write(&mut self, offset: u32, value: u32) {
        match offset {
            PFB_CFG0 => self.cfg0 = value,
            PFB_CFG1 => self.cfg1 = value,
            _ => unreachable!("offset validado pelo Bus"),
        }
    }
    pub(crate) fn save(&self, out: &mut SnapshotWriter) {
        out.section("PFBM", 1);
        out.u32(match self.profile {
            PfbProfile::LegacyUnmodeled => 0,
            PfbProfile::FirmwareMemoryV1 => 1,
        });
        out.u32(self.cfg0);
        out.u32(self.cfg1);
    }
    pub(crate) fn load(&mut self, inp: &mut SnapshotReader<'_>) -> Result<(), SnapshotError> {
        let bad = |what| SnapshotError::BadValue {
            section: "PFBM",
            what,
        };
        if inp.section("PFBM", 1)? != 1 {
            return Err(bad("versão desconhecida"));
        }
        let profile = match inp.u32()? {
            0 => PfbProfile::LegacyUnmodeled,
            1 => PfbProfile::FirmwareMemoryV1,
            _ => return Err(bad("perfil desconhecido")),
        };
        let cfg0 = inp.u32()?;
        let cfg1 = inp.u32()?;
        if profile == PfbProfile::LegacyUnmodeled && (cfg0 != 0 || cfg1 != 0) {
            return Err(bad("perfil legado não admite configuração programada"));
        }
        *self = Self {
            profile,
            cfg0,
            cfg1,
        };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perfil_depende_da_ram_real_e_nao_inventa_geometria_para_arena_de_teste() {
        for (ram, expected) in [(64 << 20, 0x03070003), (128 << 20, 0x03070103)] {
            let pfb = Nv2aPfb::for_ram(ram);
            assert_eq!(pfb.profile(), PfbProfile::FirmwareMemoryV1);
            assert_eq!(pfb.read(PFB_CFG0), Some(expected));
            assert_eq!(pfb.read(PFB_CFG1), Some(0x11448000));
        }
        for ram in [0x4000, 32 << 20, 256 << 20] {
            let pfb = Nv2aPfb::for_ram(ram);
            assert_eq!(pfb.profile(), PfbProfile::LegacyUnmodeled);
            assert_eq!(pfb.read(PFB_CFG0), None);
            assert_eq!(pfb.validate_access(PFB_CFG0, 1), Ok(false));
        }
    }
}
