//! Suspensão, retomada e cessão da CPU da filha guest (ADR-0098, subentrega 12.4a).
//!
//! Estes serviços só têm sentido com o scheduler de duas threads: sem ele a filha corre
//! enquanto o pai está estacionado e nada pode ser suspenso. Em `LegacyBootstrap` viram
//! variante recusada, como a espera bloqueante. O serviço guarda o estado do objeto e
//! **pede** o efeito de escalonamento; quem troca de thread é a sessão.
//!
//! A contagem de suspensão e os valores de status seguem o contrato público do NT que o
//! nxdk fixado consome (`lib/winapi/thread.c`): `SuspendThread`/`ResumeThread` devolvem a
//! contagem anterior e `SwitchToThread` testa `STATUS_NO_YIELD_PERFORMED`.

use xbox_mem::{Acesso, Bus, GuestVirt};

use super::threads::{MAX_SUSPEND_COUNT, variant};
use super::{
    Atendimento, Erro, Irql, Resultado, STATUS_ACCESS_VIOLATION, STATUS_INVALID_HANDLE,
    STATUS_SUCCESS, ThreadPhase, ram_range,
};
use crate::imports::Call;

/// `STATUS_SUSPEND_COUNT_EXCEEDED` (ntstatus.h do nxdk).
pub const STATUS_SUSPEND_COUNT_EXCEEDED: u32 = 0xC000_004A;
/// `STATUS_THREAD_IS_TERMINATING`: valor do NT; o nxdk fixado não o define.
pub const STATUS_THREAD_IS_TERMINATING: u32 = 0xC000_004B;
/// `STATUS_NO_YIELD_PERFORMED` (ntstatus.h do nxdk).
pub const STATUS_NO_YIELD_PERFORMED: u32 = 0x4000_0024;

/// Efeito pedido à sessão por um serviço de controle de thread.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct ControlRequest {
    /// A elegibilidade da filha mudou (suspensa ou retomada): a sessão reavalia quem roda.
    pub reschedule: bool,
    /// `NtYieldExecution`: a sessão decide o status conforme haja outra thread pronta.
    pub yield_cpu: bool,
}

impl super::Dispatcher {
    /// Resolve o handle da filha viva, sem aceitar handle fechado nem alheio.
    fn open_child_handle(&self, handle: u32) -> bool {
        !self.threads.handle_closed
            && self
                .threads
                .child
                .is_some_and(|child| child.handle == handle)
    }

    fn thread_control_guard(&self, bus: &Bus, call: &Call, scheduled: bool) -> Result<(), Erro> {
        if !scheduled {
            return Err(variant(
                bus,
                call,
                "suspensão, retomada e cessão exigem o scheduler de duas threads",
            ));
        }
        if self.irql != Irql::Passive {
            return Err(variant(
                bus,
                call,
                "controle de thread exige Passive neste perfil",
            ));
        }
        Ok(())
    }

    /// `NTSTATUS NtSuspendThread(HANDLE ThreadHandle, PULONG PreviousSuspendCount OPTIONAL)`
    pub(super) fn nt_suspend_thread(
        &mut self,
        bus: &mut Bus,
        call: &Call,
        scheduled: bool,
    ) -> Resultado {
        let handle = call.argument(bus, 0)?;
        let previous = call.argument(bus, 1)?;
        self.thread_control_guard(bus, call, scheduled)?;
        if !self.open_child_handle(handle) {
            return Ok(Atendimento {
                eax: STATUS_INVALID_HANDLE,
            });
        }
        // Saída validada antes de qualquer efeito: uma falta não deixa a thread suspensa.
        if previous != 0 && !ram_range(bus, previous, 4, Acesso::Escrita) {
            return Ok(Atendimento {
                eax: STATUS_ACCESS_VIOLATION,
            });
        }
        let child = self.threads.child.expect("handle da filha resolvido");
        if matches!(child.phase, ThreadPhase::Terminated(_)) {
            return Ok(Atendimento {
                eax: STATUS_THREAD_IS_TERMINATING,
            });
        }
        if self.threads.suspend >= MAX_SUSPEND_COUNT {
            return Ok(Atendimento {
                eax: STATUS_SUSPEND_COUNT_EXCEEDED,
            });
        }
        if previous != 0 {
            bus.escrever_bytes(
                GuestVirt(previous),
                &u32::from(self.threads.suspend).to_le_bytes(),
            )?;
        }
        self.threads.suspend += 1;
        self.control_request.reschedule = true;
        Ok(Atendimento {
            eax: STATUS_SUCCESS,
        })
    }

    /// `NTSTATUS NtResumeThread(HANDLE ThreadHandle, PULONG PreviousSuspendCount OPTIONAL)`
    pub(super) fn nt_resume_thread(
        &mut self,
        bus: &mut Bus,
        call: &Call,
        scheduled: bool,
    ) -> Resultado {
        let handle = call.argument(bus, 0)?;
        let previous = call.argument(bus, 1)?;
        self.thread_control_guard(bus, call, scheduled)?;
        if !self.open_child_handle(handle) {
            return Ok(Atendimento {
                eax: STATUS_INVALID_HANDLE,
            });
        }
        if previous != 0 && !ram_range(bus, previous, 4, Acesso::Escrita) {
            return Ok(Atendimento {
                eax: STATUS_ACCESS_VIOLATION,
            });
        }
        if previous != 0 {
            bus.escrever_bytes(
                GuestVirt(previous),
                &u32::from(self.threads.suspend).to_le_bytes(),
            )?;
        }
        // Retomar uma thread que não está suspensa não é erro: a contagem fica em zero.
        if self.threads.suspend > 0 {
            self.threads.suspend -= 1;
            self.control_request.reschedule = true;
        }
        Ok(Atendimento {
            eax: STATUS_SUCCESS,
        })
    }

    /// `NTSTATUS NtYieldExecution(void)`: o status final depende de haver outra thread
    /// pronta, o que só a sessão sabe; ela substitui o `EAX` provisório.
    pub(super) fn nt_yield_execution(
        &mut self,
        bus: &mut Bus,
        call: &Call,
        scheduled: bool,
    ) -> Resultado {
        self.thread_control_guard(bus, call, scheduled)?;
        self.control_request.yield_cpu = true;
        Ok(Atendimento {
            eax: STATUS_SUCCESS,
        })
    }

    /// Contagem de suspensão da filha.
    pub fn child_suspend_count(&self) -> u32 {
        u32::from(self.threads.suspend)
    }

    /// Só a sessão chama: devolve e zera o pedido de escalonamento do último serviço.
    pub fn take_reschedule_request(&mut self) -> bool {
        std::mem::take(&mut self.control_request.reschedule)
    }

    /// Só a sessão chama: `NtYieldExecution` foi pedido pelo último serviço?
    pub fn take_yield_request(&mut self) -> bool {
        std::mem::take(&mut self.control_request.yield_cpu)
    }
}
