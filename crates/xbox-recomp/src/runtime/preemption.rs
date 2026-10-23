//! IF/shadow de STI e pontos de preempção reconstituídos por Session (ADR-0072).
use super::{INTERRUPT_FLAG, Session, State, TickPublication};
use crate::Result;
use xbox_core::{SnapshotReader, error::SnapshotError};
use xbox_kernel::dispatch::{GuestCall, Irql};
use xbox_mem::{Bus, GuestVirt};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum InterruptProfile {
    LegacyBoundary = 0,
    MaskedV1 = 1,
}
/// Estado efêmero de uma chamada run: não é contexto CPU nem dado de snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Poll {
    deadline: Option<u64>,
}
impl State {
    pub(super) fn load_execution_profiles(
        &mut self,
        version: u32,
        inp: &mut SnapshotReader<'_>,
    ) -> std::result::Result<(), SnapshotError> {
        self.tick_publication = if version >= 9 {
            match inp.u8()? {
                0 => TickPublication::BoundariesOnly,
                1 => TickPublication::ClockLinkedV1,
                _ => {
                    return Err(SnapshotError::BadValue {
                        section: "AOTC",
                        what: "perfil de publicação de tick desconhecido",
                    });
                }
            }
        } else {
            TickPublication::BoundariesOnly
        };
        self.interrupt_profile = if version >= 10 {
            match inp.u8()? {
                0 => InterruptProfile::LegacyBoundary,
                1 => InterruptProfile::MaskedV1,
                _ => {
                    return Err(SnapshotError::BadValue {
                        section: "AOTC",
                        what: "perfil de interrupção desconhecido",
                    });
                }
            }
        } else {
            InterruptProfile::LegacyBoundary
        };
        self.sti_shadow = if version >= 10 { inp.u8()? } else { 0 };
        if self.sti_shadow > 1
            || self.sti_shadow != 0
                && (self.interrupt_profile != InterruptProfile::MaskedV1
                    || self.eflags & INTERRUPT_FLAG == 0)
        {
            return Err(SnapshotError::BadValue {
                section: "AOTC",
                what: "shadow de STI não canônica",
            });
        }
        self.poll = None;
        Ok(())
    }

    pub fn maskable_interrupts_enabled(&self) -> bool {
        self.eflags & INTERRUPT_FLAG != 0 && self.sti_shadow == 0
    }
    pub fn set_interrupts(&mut self) {
        if self.eflags & INTERRUPT_FLAG == 0 && self.interrupt_profile == InterruptProfile::MaskedV1
        {
            self.sti_shadow = 2;
        }
        self.eflags |= INTERRUPT_FLAG;
    }
    /// Cada elemento REP bem sucedido é uma fronteira de reconhecimento (ADR-0072).
    pub fn finish_interrupt_step(&mut self) {
        self.sti_shadow = self.sti_shadow.saturating_sub(1);
    }
    /// Só entre instruções/elementos REP; nenhuma instrução RMW é partida.
    pub fn preemption_requested(&self, bus: &Bus) -> bool {
        self.interrupt_profile == InterruptProfile::MaskedV1
            && self.maskable_interrupts_enabled()
            && self.poll.is_some_and(|poll| {
                bus.gpu_irq_line_asserted()
                    || poll.deadline.is_some_and(|t| self.clock.cycles() >= t)
            })
    }
}
impl Session {
    pub(super) fn interrupt_priority(&self, call: GuestCall) -> Result<Irql> {
        let level = self.bus.ler_u32(GuestVirt(call.arguments[0] + 12))?;
        u8::try_from(level)
            .ok()
            .and_then(Irql::from_level)
            .filter(|level| *level > Irql::Dispatch)
            .ok_or_else(|| xbox_core::Error::format("KINTERRUPT exige DIRQL em 3..=31").into())
    }
    pub(super) fn execution_poll(&self) -> Result<Option<Poll>> {
        if self.in_interrupt || self.state.interrupt_profile != InterruptProfile::MaskedV1 {
            return Ok(None);
        }
        let Some(call) = self.kernel.connected_gpu_interrupt(&self.bus) else {
            return Ok(None);
        };
        if self.kernel.irql() >= self.interrupt_priority(call)? {
            return Ok(None);
        }
        let deadline = if self.kernel.interrupt_can_signal(&self.bus) {
            self.vblanks
                .checked_add(1)
                .and_then(|n| n.checked_mul(Self::vblank_period()))
        } else {
            None
        };
        Ok(Some(Poll { deadline }))
    }
}
