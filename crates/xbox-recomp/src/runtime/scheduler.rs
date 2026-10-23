//! Scheduler determinístico de duas threads guest (ADR-0070/P19).
mod context;
mod persistence;
use super::{EAX, ESP, INTERRUPT_FLAG, MXCSR_RESET, RETURN_SENTINEL, Servico, Session};
use crate::{Error, Result};
use context::TaskContext;
use xbox_core::error::SnapshotError;
use xbox_kernel::dispatch::Irql;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulingProfile {
    LegacyBootstrap,
    TwoThreadV1,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Absent,
    Running,
    Ready,
    Sleeping(u64),
    KernelWait,
    Finished,
}
#[derive(Debug)]
pub(super) struct Scheduler {
    current: u8,
    status: [Status; 2],
    contexts: [Option<TaskContext>; 2],
    child_started: bool,
    last_switch: u64,
}
const QUANTUM: u64 = 4096;
fn bad(what: &'static str) -> SnapshotError {
    SnapshotError::BadValue {
        section: "SCHD",
        what,
    }
}
impl Default for Scheduler {
    fn default() -> Self {
        Self {
            current: 0,
            status: [Status::Running, Status::Absent],
            contexts: [None, None],
            child_started: false,
            last_switch: 0,
        }
    }
}
impl Scheduler {
    pub(super) fn parked_parent(&self) -> bool {
        self.current == 1 && self.contexts[0].is_some()
    }
    fn wake_deadlines(&mut self, now: u64) {
        for status in &mut self.status {
            if matches!(*status,Status::Sleeping(deadline) if deadline<=now) {
                *status = Status::Ready;
            }
        }
    }
    fn wake_waiters(&mut self) {
        for status in &mut self.status {
            if *status == Status::KernelWait {
                *status = Status::Ready;
            }
        }
    }
    fn other(&self) -> usize {
        1 - usize::from(self.current)
    }
    fn runnable(&self) -> Option<usize> {
        let other = self.other();
        if self.status[other] == Status::Ready {
            Some(other)
        } else if matches!(
            self.status[usize::from(self.current)],
            Status::Running | Status::Ready
        ) {
            Some(usize::from(self.current))
        } else {
            None
        }
    }
}
impl Session {
    pub fn scheduling_profile(&self) -> SchedulingProfile {
        if self.scheduler.is_some() {
            SchedulingProfile::TwoThreadV1
        } else {
            SchedulingProfile::LegacyBootstrap
        }
    }
    fn switch_task(&mut self, target: usize) -> Result<()> {
        let scheduler = self.scheduler.as_mut().expect("scheduler");
        let old = usize::from(scheduler.current);
        if old == target {
            scheduler.status[target] = Status::Running;
            return Ok(());
        }
        if scheduler.status[old] != Status::Finished || old == 0 {
            scheduler.contexts[old] = Some(TaskContext::capture(&self.state));
        }
        if scheduler.status[old] == Status::Running {
            scheduler.status[old] = Status::Ready;
        }
        let context = scheduler.contexts[target]
            .take()
            .ok_or_else(|| xbox_core::Error::format("contexto de thread ausente"))?;
        self.kernel.select_thread(target as u32 + 1)?;
        context.restore(&mut self.state);
        scheduler.current = target as u8;
        scheduler.status[target] = Status::Running;
        scheduler.last_switch = self.state.clock.cycles();
        Ok(())
    }
    fn start_scheduled_child(&mut self) {
        let scheduler = self.scheduler.as_mut().expect("scheduler");
        if scheduler.child_started {
            return;
        }
        let Some(child) = self.kernel.start_ready_thread() else {
            return;
        };
        scheduler.contexts[0] = Some(TaskContext::capture(&self.state));
        scheduler.status[0] = Status::Ready;
        scheduler.status[1] = Status::Running;
        scheduler.current = 1;
        scheduler.child_started = true;
        scheduler.last_switch = self.state.clock.cycles();
        self.state.regs = [0; 8];
        self.state.regs[ESP] = child.esp();
        self.state.eip = child.entry;
        self.state.eflags = 2 | INTERRUPT_FLAG;
        self.state.sti_shadow = 0;
        self.state.fs_base = child.tib();
        self.state.repeat = None;
        self.state.returned = false;
        self.state.halted = None;
        self.state.breakpoint = None;
        self.state.xmm = [[0; 4]; 8];
        self.state.mxcsr = MXCSR_RESET;
        self.state.fpu = super::x87::Fpu::default();
    }
    /// O scheduler só salta tempo quando nenhuma thread pode executar.
    pub(super) fn schedule_idle(&mut self) -> Result<()> {
        let Some(scheduler) = &mut self.scheduler else {
            return self.advance_to_next_vblank();
        };
        scheduler.wake_deadlines(self.state.clock.cycles());
        if let Some(next) = scheduler.runnable() {
            return self.switch_task(next);
        }
        let deadline = scheduler
            .status
            .iter()
            .filter_map(|s| {
                if let Status::Sleeping(t) = s {
                    Some(*t)
                } else {
                    None
                }
            })
            .min();
        let irq = if scheduler.status.contains(&Status::KernelWait)
            && self.kernel.interrupt_can_signal(&self.bus)
        {
            self.vblanks
                .checked_add(1)
                .and_then(|n| n.checked_mul(Self::vblank_period()))
        } else {
            None
        };
        let Some(target) = deadline.into_iter().chain(irq).min() else {
            return Err(Error::WouldBlock { pc: self.state.eip });
        };
        if target > self.state.clock.cycles() {
            self.state.clock.set(target);
            self.publicar_tick_count()?;
        }
        let scheduler = self.scheduler.as_mut().expect("scheduler");
        scheduler.wake_deadlines(self.state.clock.cycles());
        scheduler.wake_waiters();
        let next = scheduler
            .runnable()
            .ok_or_else(|| xbox_core::Error::format("deadline sem thread executável"))?;
        self.switch_task(next)
    }
    pub(super) fn prepare_schedule(&mut self) -> Result<()> {
        if self.in_interrupt || self.state.stopped_exit().is_some() {
            return Ok(());
        }
        let Some(scheduler) = &mut self.scheduler else {
            return Ok(());
        };
        scheduler.wake_deadlines(self.state.clock.cycles());
        if self.kernel.irql() != Irql::Passive
            || self.state.interrupt_profile == super::InterruptProfile::MaskedV1
                && !self.state.maskable_interrupts_enabled()
        {
            return Ok(());
        }
        let current = usize::from(scheduler.current);
        if scheduler.status[current] != Status::Running {
            return self.schedule_idle();
        }
        if scheduler.status[scheduler.other()] == Status::Ready
            && self
                .state
                .clock
                .cycles()
                .saturating_sub(scheduler.last_switch)
                >= QUANTUM
        {
            let next = scheduler.other();
            self.switch_task(next)?;
        }
        Ok(())
    }
    pub(super) fn scheduled_budget(&self, requested: u64) -> u64 {
        if self.kernel.irql() != Irql::Passive {
            return requested;
        }
        if let Some(scheduler) = &self.scheduler
            && matches!(
                scheduler.status[scheduler.other()],
                Status::Ready | Status::Sleeping(_)
            )
        {
            requested.min(QUANTUM)
        } else {
            requested
        }
    }
    fn validate_wait_slot(&self, call: &xbox_kernel::imports::Call) -> Result<()> {
        if self.in_interrupt
            || self.state.interrupt_profile != super::InterruptProfile::MaskedV1
            || call.export.ordinal != 233
        {
            return Ok(());
        }
        let scheduler = self.scheduler.as_ref().expect("scheduler");
        if scheduler.contexts[scheduler.other()]
            .as_ref()
            .is_some_and(|task| task.integer.pending_wait())
        {
            return Err(xbox_kernel::dispatch::Erro::VarianteNaoImplementada {
                nome:call.export.name, ordinal:call.export.ordinal, chamador:call.return_pc,
                detalhe:"P19: uma única chamada NtWait pendente; segunda espera exige filas por thread",
                argumentos:format!("0x{:08X}, 0x{:08X}, 0x{:08X}",call.argument(&self.bus,0)?,call.argument(&self.bus,1)?,call.argument(&self.bus,2)?),
            }.into());
        }
        Ok(())
    }
    pub(super) fn scheduled_kernel(&mut self) -> Result<Servico> {
        use xbox_kernel::dispatch::{
            ORDINAL_PS_CREATE_SYSTEM_THREAD_EX, ORDINAL_PS_TERMINATE_SYSTEM_THREAD,
        };
        if self.state.stopped_exit().is_some() {
            return Err(xbox_core::Error::format("CPU parada não atende serviço").into());
        }
        let call = self.kernel_call()?;
        self.validate_wait_slot(&call)?;
        let terminating = call.export.ordinal == ORDINAL_PS_TERMINATE_SYSTEM_THREAD;
        if self.in_interrupt
            && matches!(
                call.export.ordinal,
                ORDINAL_PS_TERMINATE_SYSTEM_THREAD | ORDINAL_PS_CREATE_SYSTEM_THREAD_EX
            )
        {
            return Err(Error::BlockedInInterrupt { pc: self.state.eip });
        }
        if terminating && self.kernel.irql() != Irql::Passive {
            return Err(xbox_kernel::dispatch::Erro::ThreadState(
                "término escalonado exige Passive",
            )
            .into());
        }
        if !terminating {
            self.validate_kernel_return(&call)?;
        }
        let answer = if self.in_interrupt {
            self.kernel.atender_com_clock(
                &mut self.bus,
                self.state.eip,
                &self.state.regs,
                &self.state.clock,
            )?
        } else {
            self.kernel.atender_escalonado(
                &mut self.bus,
                self.state.eip,
                &self.state.regs,
                &self.state.clock,
            )?
        };
        if terminating {
            self.finish_scheduled_thread(answer.eax, true)?;
            return Ok(Servico::Concluido);
        }
        let time = self.kernel.take_time_request();
        let blocked = self.kernel.take_block_request();
        let deadline = if let Some(time) = time {
            if self.in_interrupt {
                return Err(Error::BlockedInInterrupt { pc: self.state.eip });
            }
            Some(
                self.state
                    .clock
                    .cycles()
                    .checked_add(time.ciclos)
                    .ok_or(Error::TimeExhausted { pc: self.state.eip })?,
            )
        } else {
            None
        };
        if blocked {
            let scheduler = self.scheduler.as_mut().expect("scheduler");
            scheduler.status[usize::from(scheduler.current)] = Status::KernelWait;
            if let Some(next) = scheduler.runnable() {
                self.switch_task(next)?;
                return Ok(Servico::Concluido);
            }
            return Ok(Servico::Bloqueado);
        }
        self.apply_kernel_return(&call, answer.eax);
        // ISR/DPC pode sinalizar um objeto de outra thread. Torná-la Ready é
        // permitido aqui; a troca continua adiada até sair da interrupção.
        self.scheduler.as_mut().expect("scheduler").wake_waiters();
        if !self.in_interrupt {
            if let Some(deadline) = deadline {
                let scheduler = self.scheduler.as_mut().expect("scheduler");
                scheduler.status[usize::from(scheduler.current)] = Status::Sleeping(deadline);
                self.schedule_idle()?;
            }
            self.start_scheduled_child();
        }
        Ok(Servico::Concluido)
    }
    fn finish_scheduled_thread(&mut self, status: u32, terminated: bool) -> Result<bool> {
        if self.kernel.irql() != Irql::Passive {
            return Err(xbox_kernel::dispatch::Erro::ThreadState(
                "término escalonado exige Passive",
            )
            .into());
        }
        let scheduler = self.scheduler.as_mut().expect("scheduler");
        let current = usize::from(scheduler.current);
        if current == 1 && !terminated {
            self.kernel.terminate_running_thread(status)?;
        }
        scheduler.status[current] = Status::Finished;
        scheduler.wake_waiters();
        if scheduler.status[0] == Status::Finished
            && matches!(scheduler.status[1], Status::Absent | Status::Finished)
        {
            if current == 1 {
                let parent = scheduler.contexts[0]
                    .take()
                    .ok_or_else(|| xbox_core::Error::format("retorno inicial ausente"))?;
                parent.restore(&mut self.state);
                self.kernel.select_thread(1)?;
                scheduler.current = 0;
            }
            return Ok(false);
        }
        if terminated {
            self.state.returned = true;
            self.state.eip = RETURN_SENTINEL;
        }
        self.schedule_idle()?;
        Ok(true)
    }
    pub(super) fn scheduled_return(&mut self) -> Result<bool> {
        if !self.state.returned {
            return Ok(false);
        }
        self.finish_scheduled_thread(self.state.regs[EAX], false)
    }
}
