//! Continuação do pai sem cópia do Clock, retired ou perfil da única CPU guest.
use super::{RepeatState, State};
use xbox_core::{Snapshot, SnapshotReader, SnapshotWriter, error::SnapshotError};

#[derive(Debug, Clone)]
pub(super) struct ThreadContext {
    regs: [u32; 8],
    eip: u32,
    eflags: u32,
    fs_base: u32,
    returned: bool,
    repeat: Option<RepeatState>,
}
impl ThreadContext {
    pub(super) fn validate(&self, bus: &xbox_mem::Bus) -> Result<(), SnapshotError> {
        use xbox_kernel::loader::{STACK_BASE, STACK_SIZE, TIB_BASE};
        let executable = bus
            .mmu()
            .consultar(xbox_mem::GuestVirt(self.eip))
            .is_some_and(|(_, perm)| perm.permite(xbox_mem::Acesso::Execucao));
        if self.fs_base != TIB_BASE
            || !(STACK_BASE..=STACK_BASE + STACK_SIZE).contains(&self.regs[super::ESP])
            || self.repeat.is_some()
            || self.returned != (self.eip == super::RETURN_SENTINEL)
            || !self.returned && !executable
        {
            return Err(SnapshotError::BadValue {
                section: "TCXT",
                what: "continuação do pai fora do perfil de bootstrap",
            });
        }
        Ok(())
    }
    /// Contextos do scheduler podem estar em REP ou chamada de kernel pendente.
    pub(super) fn validate_task(
        &self,
        bus: &xbox_mem::Bus,
        id: u8,
        child: Option<xbox_kernel::dispatch::GuestThread>,
    ) -> Result<(), SnapshotError> {
        use xbox_kernel::loader::{STACK_BASE, STACK_SIZE, TIB_BASE};
        let (low, high, tib) = if id == 0 {
            (STACK_BASE, STACK_BASE + STACK_SIZE, TIB_BASE)
        } else {
            let child = child.ok_or(SnapshotError::BadValue {
                section: "SCHED",
                what: "contexto sem filha",
            })?;
            (child.region.base, child.stack_top(), child.tib())
        };
        let exec = bus
            .mmu()
            .consultar(xbox_mem::GuestVirt(self.eip))
            .is_some_and(|(_, p)| p.permite(xbox_mem::Acesso::Execucao));
        if self.fs_base != tib
            || !(low..=high).contains(&self.regs[super::ESP])
            || self.returned != (self.eip == super::RETURN_SENTINEL)
            || (!self.returned && !exec)
            || self.repeat.is_some_and(|r| r.pc != self.eip)
        {
            return Err(SnapshotError::BadValue {
                section: "SCHED",
                what: "contexto diverge de thread, pilha, TLS ou continuação",
            });
        }
        Ok(())
    }
    pub(super) fn pending_wait(&self) -> bool {
        self.eip == xbox_kernel::imports::trampoline(233)
    }
    pub(super) fn interrupts_enabled(&self) -> bool {
        self.eflags & super::INTERRUPT_FLAG != 0
    }
    pub(super) fn capture(state: &State) -> Self {
        Self {
            regs: state.regs,
            eip: state.eip,
            eflags: state.eflags,
            fs_base: state.fs_base,
            returned: state.returned,
            repeat: state.repeat,
        }
    }
    pub(super) fn restore(self, state: &mut State) {
        state.regs = self.regs;
        state.eip = self.eip;
        state.eflags = self.eflags;
        state.fs_base = self.fs_base;
        state.returned = self.returned;
        state.repeat = self.repeat;
    }
}
impl Snapshot for ThreadContext {
    fn save(&self, out: &mut SnapshotWriter) {
        out.section("TCXT", 1);
        for r in self.regs {
            out.u32(r);
        }
        out.u32(self.eip);
        out.u32(self.eflags);
        out.u32(self.fs_base);
        out.bool(self.returned);
        out.bool(self.repeat.is_some());
        if let Some(r) = self.repeat {
            out.u32(r.pc);
            out.u32(r.eflags);
        }
    }
    fn load(&mut self, inp: &mut SnapshotReader<'_>) -> Result<(), SnapshotError> {
        if inp.section("TCXT", 1)? != 1 {
            return Err(SnapshotError::BadValue {
                section: "TCXT",
                what: "versão de contexto desconhecida",
            });
        }
        for r in &mut self.regs {
            *r = inp.u32()?;
        }
        self.eip = inp.u32()?;
        self.eflags = inp.u32()?;
        self.fs_base = inp.u32()?;
        self.returned = inp.bool()?;
        self.repeat = if inp.bool()? {
            Some(RepeatState {
                pc: inp.u32()?,
                eflags: inp.u32()?,
            })
        } else {
            None
        };
        Ok(())
    }
}
