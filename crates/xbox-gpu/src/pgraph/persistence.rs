use super::{ClearStats, Nv2aPgraph, PGRAPH_ERROR, State, Surface};
use xbox_core::{SnapshotReader, SnapshotWriter, error::SnapshotError};
fn bad(what: &'static str) -> SnapshotError {
    SnapshotError::BadValue {
        section: "PGRF",
        what,
    }
}
impl Nv2aPgraph {
    pub fn save(&self, out: &mut SnapshotWriter) {
        out.section("PGRF", 1);
        out.u32(u32::from(self.state.is_some()));
        let mut data = SnapshotWriter::new();
        if let Some(s) = &self.state {
            for value in [
                s.pending,
                s.enabled,
                s.trapped_address,
                s.trapped_data,
                s.ctx_switch1,
                s.fifo,
            ] {
                data.u32(value);
            }
            data.bool(s.scanout.is_some());
            if let Some(v) = s.scanout {
                data.u32(v);
            }
            let f = s.surface;
            for value in [
                f.notifier_dma,
                f.color_dma,
                f.zeta_dma,
                f.horizontal,
                f.vertical,
                f.format,
                f.pitch,
                f.color_offset,
                f.zeta_offset,
                f.z_clear,
                f.color_clear,
                f.rect_h,
                f.rect_v,
            ] {
                data.u32(value);
            }
            data.u64(s.stats.calls);
            data.u64(s.stats.pixels);
            data.bool(s.stats.first_color.is_some());
            if let Some(v) = s.stats.first_color {
                data.u32(v);
            }
            out.bytes(&data.finish());
        } else {
            out.bytes(&[]);
        }
    }
    pub fn load(&mut self, inp: &mut SnapshotReader<'_>) -> Result<(), SnapshotError> {
        if inp.section("PGRF", 1)? != 1 {
            return Err(bad("versão PGRAPH desconhecida"));
        }
        let profile = inp.u32()?;
        let bytes = inp.bytes()?;
        if profile == 0 && bytes.is_empty() {
            self.state = None;
            return Ok(());
        }
        if profile != 1 || bytes.len() > 4096 {
            return Err(bad("perfil/payload PGRAPH inválido"));
        }
        let mut r = SnapshotReader::new(bytes)?;
        let pending = r.u32()?;
        let enabled = r.u32()?;
        let trapped_address = r.u32()?;
        let trapped_data = r.u32()?;
        let ctx_switch1 = r.u32()?;
        let fifo = r.u32()?;
        let scanout = optional(&mut r)?;
        let surface = Surface {
            notifier_dma: r.u32()?,
            color_dma: r.u32()?,
            zeta_dma: r.u32()?,
            horizontal: r.u32()?,
            vertical: r.u32()?,
            format: r.u32()?,
            pitch: r.u32()?,
            color_offset: r.u32()?,
            zeta_offset: r.u32()?,
            z_clear: r.u32()?,
            color_clear: r.u32()?,
            rect_h: r.u32()?,
            rect_v: r.u32()?,
        };
        let stats = ClearStats {
            calls: r.u64()?,
            pixels: r.u64()?,
            first_color: optional(&mut r)?,
        };
        if pending & !PGRAPH_ERROR != 0
            || (pending != 0
                && (trapped_address & !0x00071FFC != 0
                    || trapped_address & 0x1FFC != 0x100
                    || trapped_data == 0
                    || ctx_switch1 & 0xFFF != 0x97))
        {
            return Err(bad("trap PGRAPH inconsistente"));
        }
        for instance in [surface.notifier_dma, surface.color_dma, surface.zeta_dma] {
            if instance & 15 != 0 || instance >= 0x100000 {
                return Err(bad("instância de surface inválida"));
            }
        }
        if r.remaining() != 0
            || (stats.calls == 0 && (stats.pixels != 0 || stats.first_color.is_some()))
        {
            return Err(bad("estatísticas/payload PGRAPH inconsistentes"));
        }
        self.state = Some(State {
            pending,
            enabled,
            trapped_address,
            trapped_data,
            ctx_switch1,
            fifo,
            scanout,
            surface,
            stats,
        });
        Ok(())
    }
}
fn optional(r: &mut SnapshotReader<'_>) -> Result<Option<u32>, SnapshotError> {
    match r.u8()? {
        0 => Ok(None),
        1 => Ok(Some(r.u32()?)),
        _ => Err(bad("marcador opcional não canônico")),
    }
}
