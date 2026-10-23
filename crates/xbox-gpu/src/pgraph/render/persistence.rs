use super::{CONSTANTS, DrawStats, PROGRAMS, Render};
use xbox_core::{SnapshotReader, SnapshotWriter, error::SnapshotError};
fn bad(what: &'static str) -> SnapshotError {
    SnapshotError::BadValue {
        section: "PRND",
        what,
    }
}
impl Render {
    pub(in crate::pgraph) fn save(&self, w: &mut SnapshotWriter) {
        w.section("PRND", 1);
        w.u32(self.registers.len() as u32);
        for (&method, &value) in &self.registers {
            w.u16(method);
            w.u32(value);
        }
        w.u32(self.program_load);
        w.u32(self.constant_load);
        for (p, &mask) in self.program.iter().zip(&self.program_mask) {
            w.u8(mask);
            for &word in p {
                w.u32(word);
            }
        }
        for &mask in &self.constant_mask {
            w.u8(mask);
        }
        for p in &self.constants {
            for &word in p {
                w.u32(word);
            }
        }
        for value in [self.stats.calls, self.stats.triangles, self.stats.pixels] {
            w.u64(value);
        }
    }
    pub(in crate::pgraph) fn load(r: &mut SnapshotReader<'_>) -> Result<Self, SnapshotError> {
        if r.section("PRND", 1)? != 1 {
            return Err(bad("versao raster desconhecida"));
        }
        let n = r.u32()?;
        if n > 256 {
            return Err(bad("registros raster excedem limite"));
        }
        let mut state = Self::default();
        let mut previous = None;
        for _ in 0..n {
            let method = r.u16()?;
            let value = r.u32()?;
            if !Self::models(method)
                || matches!(method, 0x1810 | 0xB00..=0xBFC | 0x1E9C | 0x1EA4)
                || previous.is_some_and(|p| method <= p)
            {
                return Err(bad("registro raster invalido ou desordenado"));
            }
            previous = Some(method);
            state.registers.insert(method, value);
        }
        state.program_load = r.u32()?;
        state.constant_load = r.u32()?;
        if state.program_load > PROGRAMS as u32
            || state.constant_load > CONSTANTS as u32
            || state.reg(0x1EA0) >= PROGRAMS as u32
            || !matches!(state.reg(0x17FC), 0 | 5)
        {
            return Err(bad("cursores ou primitiva raster invalidos"));
        }
        for (p, mask) in state.program.iter_mut().zip(&mut state.program_mask) {
            *mask = r.u8()?;
            if *mask > 15 {
                return Err(bad("mascara de upload invalida"));
            }
            for word in p {
                *word = r.u32()?;
            }
        }
        for mask in &mut state.constant_mask {
            *mask = r.u8()?;
            if *mask > 15 {
                return Err(bad("mascara de constantes invalida"));
            }
        }
        for p in &mut state.constants {
            for word in p {
                *word = r.u32()?;
            }
        }
        for reg in [0x19C, 0x1A0] {
            if state.reg(reg) & 15 != 0 || state.reg(reg) >= 0x100000 {
                return Err(bad("instancia de vertices invalida"));
            }
        }
        state.stats = DrawStats {
            calls: r.u64()?,
            triangles: r.u64()?,
            pixels: r.u64()?,
        };
        if state.stats.calls == 0 && (state.stats.triangles != 0 || state.stats.pixels != 0) {
            return Err(bad("estatisticas raster inconsistentes"));
        }
        Ok(state)
    }
}
