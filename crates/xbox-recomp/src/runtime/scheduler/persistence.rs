use super::super::State;
use super::{Scheduler, Status, TaskContext, bad};
use xbox_core::{SnapshotReader, SnapshotWriter, error::SnapshotError};
use xbox_kernel::dispatch::{Dispatcher, ThreadPhase};
use xbox_mem::Bus;
impl Scheduler {
    pub(in crate::runtime) fn save_profile(value: Option<&Self>, w: &mut SnapshotWriter) {
        w.section("SCHD", 1);
        w.u32(u32::from(value.is_some()));
        let mut data = SnapshotWriter::new();
        if let Some(s) = value {
            data.u8(s.current);
            data.bool(s.child_started);
            data.u64(s.last_switch);
            for status in s.status {
                let (tag, time) = match status {
                    Status::Absent => (0, 0),
                    Status::Running => (1, 0),
                    Status::Ready => (2, 0),
                    Status::Sleeping(t) => (3, t),
                    Status::KernelWait => (4, 0),
                    Status::Finished => (5, 0),
                };
                data.u8(tag);
                data.u64(time);
            }
            for context in &s.contexts {
                data.bool(context.is_some());
                if let Some(c) = context {
                    c.save(&mut data);
                }
            }
            w.bytes(&data.finish());
        } else {
            w.bytes(&[]);
        }
    }
    pub(in crate::runtime) fn load_profile(
        r: &mut SnapshotReader<'_>,
    ) -> Result<Option<Self>, SnapshotError> {
        if r.section("SCHD", 1)? != 1 {
            return Err(bad("versão de scheduler desconhecida"));
        }
        let profile = r.u32()?;
        let bytes = r.bytes()?;
        if profile == 0 && bytes.is_empty() {
            return Ok(None);
        }
        if profile != 1 || bytes.len() > 4096 {
            return Err(bad("perfil ou tamanho de scheduler inválido"));
        }
        let mut r = SnapshotReader::new(bytes)?;
        let current = r.u8()?;
        let child_started = boolean(&mut r)?;
        let last_switch = r.u64()?;
        if current > 1 {
            return Err(bad("thread ativa fora do perfil"));
        }
        let mut status = [Status::Absent; 2];
        for status in &mut status {
            let tag = r.u8()?;
            let time = r.u64()?;
            *status = match (tag, time) {
                (0, 0) => Status::Absent,
                (1, 0) => Status::Running,
                (2, 0) => Status::Ready,
                (3, t) if t != 0 => Status::Sleeping(t),
                (4, 0) => Status::KernelWait,
                (5, 0) => Status::Finished,
                _ => return Err(bad("estado de thread inválido")),
            };
        }
        let mut contexts = [None, None];
        for context in &mut contexts {
            if boolean(&mut r)? {
                *context = Some(TaskContext::load(&mut r)?);
            }
        }
        if r.remaining() != 0 {
            return Err(bad("bytes extras no scheduler"));
        }
        Ok(Some(Self {
            current,
            status,
            contexts,
            child_started,
            last_switch,
        }))
    }
    pub(in crate::runtime) fn validate(
        &self,
        bus: &Bus,
        kernel: &Dispatcher,
        state: &State,
    ) -> Result<(), SnapshotError> {
        let current = usize::from(self.current);
        let child = kernel.child_thread();
        if self.last_switch > state.clock.cycles()
            || kernel.current_thread() != u32::from(self.current) + 1
            || self.status[0] == Status::Absent
            || self.contexts[current].is_some()
            || matches!(
                self.status[current],
                Status::Absent | Status::Ready | Status::Sleeping(_)
            )
        {
            return Err(bad("CPU, estado ou relógio divergem do scheduler"));
        }
        if self.child_started {
            let child = child.ok_or_else(|| bad("recurso da filha ausente"))?;
            if self.status[1] == Status::Absent
                || matches!(child.phase, ThreadPhase::Terminated(_))
                    != (self.status[1] == Status::Finished)
                || (self.status[1] != Status::Finished
                    && (child.phase == ThreadPhase::Running) != (current == 1))
            {
                return Err(bad("fase kernel diverge da filha escalonada"));
            }
        } else if child.is_some()
            || current != 0
            || self.status[1] != Status::Absent
            || self.contexts[1].is_some()
        {
            return Err(bad("filha não iniciada incoerente"));
        }
        for id in 0..2 {
            if id != current
                && matches!(
                    self.status[id],
                    Status::Ready | Status::Sleeping(_) | Status::KernelWait | Status::Finished
                )
                && !(id == 1 && self.status[id] == Status::Finished)
                && self.contexts[id].is_none()
            {
                return Err(bad("continuação inativa ausente"));
            }
            if id != current && self.status[id] == Status::Running {
                return Err(bad("duas threads ativas"));
            }
            if let Some(context) = &self.contexts[id] {
                context.integer.validate_task(bus, id as u8, child)?;
            }
        }
        super::super::threads::ThreadContext::capture(state).validate_task(
            bus,
            self.current,
            child,
        )?;
        // ThreadPhase não armazena deadline: a CPU selecionada pode estar esperando
        // no trampolim. A continuação permanece reconstruível por EIP/ESP/RAM.
        if self.status[current] == Status::KernelWait
            && xbox_kernel::imports::entry(bus, state.eip).is_err()
        {
            return Err(bad("espera sem chamada de kernel pendente"));
        }
        if self.status[current] == Status::Finished
            && (!state.returned
                || current != 0
                || !matches!(self.status[1], Status::Absent | Status::Finished))
        {
            return Err(bad("retorno final incoerente"));
        }
        Ok(())
    }
}
fn boolean(r: &mut SnapshotReader<'_>) -> Result<bool, SnapshotError> {
    match r.u8()? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(bad("boolean não canônico")),
    }
}
