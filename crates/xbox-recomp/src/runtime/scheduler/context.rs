use super::super::{MXCSR_RESET, State, threads::ThreadContext, x87};
use xbox_core::{Snapshot, SnapshotReader, SnapshotWriter, error::SnapshotError};
/// Só estado local. Clock, retired e perfil da máquina continuam na CPU única.
#[derive(Debug, Clone)]
pub(super) struct TaskContext {
    pub integer: ThreadContext,
    xmm: [[u32; 4]; 8],
    mxcsr: u32,
    fpu: x87::Fpu,
    sti_shadow: u8,
}
impl TaskContext {
    pub(super) fn capture(state: &State) -> Self {
        Self {
            integer: ThreadContext::capture(state),
            xmm: state.xmm,
            mxcsr: state.mxcsr,
            fpu: state.fpu,
            sti_shadow: state.sti_shadow,
        }
    }
    pub(super) fn validate_interrupt_profile(
        &self,
        profile: super::super::InterruptProfile,
    ) -> Result<(), SnapshotError> {
        if self.sti_shadow != 0 && profile != super::super::InterruptProfile::MaskedV1 {
            return Err(super::bad("shadow de STI fora do perfil"));
        }
        Ok(())
    }
    pub(super) fn restore(self, state: &mut State) {
        self.integer.restore(state);
        state.xmm = self.xmm;
        state.mxcsr = self.mxcsr;
        state.fpu = self.fpu;
        state.sti_shadow = self.sti_shadow;
        state.halted = None;
        state.breakpoint = None;
    }
    pub(super) fn save(&self, w: &mut SnapshotWriter) {
        w.section("TCTX", 2);
        self.integer.save(w);
        for r in self.xmm {
            for v in r {
                w.u32(v);
            }
        }
        w.u32(self.mxcsr);
        self.fpu.save(w);
        w.u8(self.sti_shadow);
    }
    pub(super) fn load(r: &mut SnapshotReader<'_>) -> Result<Self, SnapshotError> {
        let version = r.section("TCTX", 2)?;
        if version == 0 {
            return Err(super::bad("versão de contexto desconhecida"));
        }
        let mut value = Self::capture(&State::new(0));
        value.integer.load(r)?;
        for xmm in &mut value.xmm {
            for v in xmm {
                *v = r.u32()?;
            }
        }
        value.mxcsr = r.u32()?;
        if value.mxcsr != MXCSR_RESET {
            return Err(super::bad("MXCSR fora do perfil"));
        }
        value.fpu.load(r)?;
        value.sti_shadow = if version >= 2 { r.u8()? } else { 0 };
        if value.sti_shadow > 1 || value.sti_shadow != 0 && !value.integer.interrupts_enabled() {
            return Err(super::bad("shadow de STI inválida no contexto"));
        }
        Ok(value)
    }
}
