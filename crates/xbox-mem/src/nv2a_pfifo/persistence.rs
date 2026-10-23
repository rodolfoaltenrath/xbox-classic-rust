use super::{
    MethodRecord,
    commands::{Binding, CommandState, MAX_METHOD_RECORDS},
};
use xbox_core::{SnapshotReader, SnapshotWriter, error::SnapshotError};
fn bad(what: &'static str) -> SnapshotError {
    SnapshotError::BadValue {
        section: "PCMD",
        what,
    }
}
impl CommandState {
    pub(super) fn save(&self, out: &mut SnapshotWriter) {
        out.section("PCMD", 1);
        for binding in self.bindings {
            out.bool(binding.is_some());
            if let Some(b) = binding {
                out.u32(b.handle);
                out.u32(b.instance);
                out.u32(b.class);
            }
        }
        out.u32(self.methods.len() as u32);
        for r in &self.methods {
            out.u32(r.class);
            out.u8(r.subchannel);
            out.u16(r.method);
            out.u64(r.count);
            out.u32(r.first);
            out.u32(r.last);
            out.u32(r.first_resolved);
            out.u32(r.last_resolved);
        }
    }
    pub(super) fn load(inp: &mut SnapshotReader<'_>) -> Result<Self, SnapshotError> {
        if inp.section("PCMD", 1)? != 1 {
            return Err(bad("versão de comandos desconhecida"));
        }
        let mut state = Self::default();
        for dest in &mut state.bindings {
            match inp.u8()? {
                0 => (),
                1 => {
                    let b = Binding {
                        handle: inp.u32()?,
                        instance: inp.u32()?,
                        class: inp.u32()?,
                    };
                    if b.instance >= 0x100000 || b.instance & 15 != 0 || b.class > 0xFFF {
                        return Err(bad("binding GPU inválido"));
                    }
                    *dest = Some(b);
                }
                _ => return Err(bad("marcador de binding não canônico")),
            }
        }
        let count = inp.u32()? as usize;
        if count > MAX_METHOD_RECORDS {
            return Err(bad("limite de registros de métodos excedido"));
        }
        for _ in 0..count {
            let r = MethodRecord {
                class: inp.u32()?,
                subchannel: inp.u8()?,
                method: inp.u16()?,
                count: inp.u64()?,
                first: inp.u32()?,
                last: inp.u32()?,
                first_resolved: inp.u32()?,
                last_resolved: inp.u32()?,
            };
            if r.class > 0xFFF
                || r.subchannel >= 8
                || r.method > 0x1FFC
                || r.method & 3 != 0
                || r.count == 0
                || state
                    .methods
                    .last()
                    .is_some_and(|prev| prev.key() >= r.key())
            {
                return Err(bad("registro de método inválido ou fora de ordem"));
            }
            state.methods.push(r);
        }
        if inp.remaining() != 0 {
            return Err(bad("bytes extras no estado de comandos"));
        }
        Ok(state)
    }
}
