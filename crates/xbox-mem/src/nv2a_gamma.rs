//! DAC RGB de 8 bits, perfil da referência pública (ADR-0079).
//!
//! Só os dois offsets observados no nxdk. Identidade inicial e wrap são escolhas
//! do perfil xemu fixado, não medições de reset/analógico do console.
use xbox_core::{
    error::SnapshotError,
    snapshot::{SnapshotReader, SnapshotWriter},
};

pub const WRITE_INDEX: u32 = 0x0068_13C8;
pub const PALETTE_DATA: u32 = 0x0068_13C9;
const COMPONENTS: usize = 256 * 3;
/// Seção GAMM v1: cabeçalho, perfil, cursor e 768 componentes RGB.
pub const SNAPSHOT_BYTES: usize = 8 + 4 + 2 + COMPONENTS;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GammaProfile {
    /// BUS0 v1–v9: escritas não tinham efeito e a apresentação era bruta.
    LegacyUnmodeled,
    /// RGB8 imediato, identidade inicial e avanço circular da referência pública.
    PublicReferenceV1,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nv2aGamma {
    profile: GammaProfile,
    cursor: u16,
    palette: [[u8; 3]; 256],
}

impl Default for Nv2aGamma {
    fn default() -> Self {
        Self {
            profile: GammaProfile::PublicReferenceV1,
            cursor: 0,
            palette: std::array::from_fn(|i| [i as u8; 3]),
        }
    }
}

impl Nv2aGamma {
    pub(crate) const fn legacy() -> Self {
        Self {
            profile: GammaProfile::LegacyUnmodeled,
            cursor: 0,
            palette: [[0; 3]; 256],
        }
    }

    pub const fn profile(&self) -> GammaProfile {
        self.profile
    }

    /// Cursor do próximo componente (índice × 3 + canal R/G/B).
    pub const fn cursor(&self) -> u16 {
        self.cursor
    }

    /// Ausência preserva a apresentação de snapshots legados.
    pub const fn palette(&self) -> Option<&[[u8; 3]; 256]> {
        match self.profile {
            GammaProfile::LegacyUnmodeled => None,
            GammaProfile::PublicReferenceV1 => Some(&self.palette),
        }
    }

    /// Zero significa registradores fora do recorte. Acesso que se sobreponha
    /// aos dois bytes exige exatamente um byte, antes de qualquer efeito.
    pub(crate) fn validate_access(
        &self,
        offset: u32,
        len: usize,
        write: bool,
    ) -> Result<bool, &'static str> {
        if self.profile == GammaProfile::LegacyUnmodeled || len == 0 {
            return Ok(false);
        }
        let start = u64::from(offset);
        let end = start.saturating_add(len as u64);
        if start > u64::from(PALETTE_DATA) || end <= u64::from(WRITE_INDEX) {
            return Ok(false);
        }
        if len != 1 || !matches!(offset, WRITE_INDEX | PALETTE_DATA) {
            return Err("DAC gamma exige acesso de 8 bits (ADR-0079)");
        }
        if !write && offset == PALETTE_DATA {
            return Err("leitura de dados do DAC gamma não implementada neste perfil");
        }
        Ok(true)
    }

    pub(crate) const fn read(&self) -> u32 {
        (self.cursor / 3) as u32
    }

    pub(crate) fn write(&mut self, offset: u32, value: u8) {
        if offset == WRITE_INDEX {
            self.cursor = u16::from(value) * 3;
        } else {
            let cursor = usize::from(self.cursor);
            self.palette[cursor / 3][cursor % 3] = value;
            // Wrap público não confirmado em hardware; condição de saída em P21.
            self.cursor = (self.cursor + 1) % COMPONENTS as u16;
        }
    }

    pub(crate) fn save(&self, out: &mut SnapshotWriter) {
        out.section("GAMM", 1);
        out.u32(match self.profile {
            GammaProfile::LegacyUnmodeled => 0,
            GammaProfile::PublicReferenceV1 => 1,
        });
        out.u16(self.cursor);
        for entry in self.palette {
            out.raw(&entry);
        }
    }

    pub(crate) fn load(&mut self, reader: &mut SnapshotReader<'_>) -> Result<(), SnapshotError> {
        let bad = |what| SnapshotError::BadValue {
            section: "GAMM",
            what,
        };
        if reader.section("GAMM", 1)? != 1 {
            return Err(bad("versão desconhecida"));
        }
        let profile = match reader.u32()? {
            0 => GammaProfile::LegacyUnmodeled,
            1 => GammaProfile::PublicReferenceV1,
            _ => return Err(bad("perfil desconhecido")),
        };
        let cursor = reader.u16()?;
        if usize::from(cursor) >= COMPONENTS {
            return Err(bad("cursor fora da paleta"));
        }
        let mut palette = [[0; 3]; 256];
        for entry in &mut palette {
            entry.copy_from_slice(reader.raw(3)?);
        }
        if profile == GammaProfile::LegacyUnmodeled && (cursor != 0 || palette != [[0; 3]; 256]) {
            return Err(bad("perfil legado não admite estado de gamma programado"));
        }
        *self = Self {
            profile,
            cursor,
            palette,
        };
        Ok(())
    }
}
