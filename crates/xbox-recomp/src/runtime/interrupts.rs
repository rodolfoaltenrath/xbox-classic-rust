//! Entrega de interrupções: vblank, ISR e DPC como chamadas guest aninhadas (ADR-0055).
//!
//! O código recompilado é uma máquina de blocos sem pilha nativa, então uma rotina de
//! interrupção não é "chamada" pelo host: a sessão **empilha um quadro na pilha do guest**
//! (argumentos, endereço de retorno sentinela), aponta o EIP para a rotina e roda o código
//! nativo até ela retornar ao sentinela — o mesmo truque do retorno da thread de sistema.
//! Depois restaura o contexto interrompido.
//!
//! ## Pontos de entrega
//!
//! Só nas fronteiras que o host já tem: antes de cada fatia de `conduzir` (inclusive a que
//! repete uma chamada de kernel bloqueada). Não há preempção no meio de um bloco, e não
//! precisa haver: um quadro dura ~12 milhões de ciclos e uma fatia, ~1 milhão.
//!
//! ## Atômico para o host
//!
//! ISR e DPC rodam até o fim dentro de uma única chamada. O host nunca vê a CPU no meio de
//! uma rotina de interrupção, então o snapshot não precisa de um quadro interrompido — e
//! uma rotina que não retorna vira erro, não um laço infinito.
//!
//! ## O relógio de vblank
//!
//! Um vblank a cada `frame_cycles(59,94 Hz)` ciclos do Clock único. Vários períodos sem
//! atendimento viram **um** (a pendência trava, como no hardware).

use xbox_core::Clock;
use xbox_core::time::VBLANK_NTSC_MHZ;
use xbox_kernel::dispatch::{GuestCall, Irql};
use xbox_mem::Bus;

use super::{
    ESP, Exit, INTERRUPT_FLAG, RETURN_SENTINEL, Servico, Session, State, threads::ThreadContext,
};
use crate::{Error, Result};

/// Fatias que uma rotina de interrupção pode usar antes de ser dada como presa.
const MAX_ROUTINE_SLICES: u32 = 64;
/// DPCs entregues por ponto de entrega: um DPC que se reenfileira sempre não prende o host.
const MAX_DPCS_PER_DELIVERY: u32 = 256;

impl Session {
    /// Duração de um vblank, em ciclos.
    pub(super) fn vblank_period() -> u64 {
        Clock::frame_cycles(VBLANK_NTSC_MHZ)
    }

    /// Vblanks já gerados desde o início da sessão.
    pub const fn vblanks(&self) -> u64 {
        self.vblanks
    }

    /// Trava a pendência de vblank se o relógio passou de um ou mais períodos.
    pub(super) fn sync_vblank(&mut self) {
        let due = self.state.clock.cycles() / Self::vblank_period();
        if due > self.vblanks {
            self.vblanks = due;
            self.bus.nv2a_irq_mut().raise_vblank();
        }
    }

    /// Deixa o tempo guest correr até o próximo vblank (a espera bloqueada).
    pub(super) fn advance_to_next_vblank(&mut self) -> Result<()> {
        let target = self
            .vblanks
            .checked_add(1)
            .and_then(|next| next.checked_mul(Self::vblank_period()))
            .ok_or(Error::TimeExhausted { pc: self.state.eip })?;
        if self.state.clock.cycles() < target {
            self.state.clock.set(target);
            self.publicar_tick_count()?;
        }
        Ok(())
    }

    /// Entrega o que estiver pendente: o ISR, se a linha está ligada e a CPU aceita
    /// interrupções, e depois os DPCs enfileirados.
    pub(super) fn deliver_interrupts<R>(&mut self, run: &mut R, blocks: u64) -> Result<()>
    where
        R: FnMut(&mut State, &mut Bus, u64) -> Result<Exit>,
    {
        self.sync_vblank();
        if self.state.stopped_exit().is_some() {
            return Ok(());
        }
        // IF não é consultada: o AOT não tem STI e o main começa com IF desligada (P11).
        if let Some(call) = self.kernel.pending_interrupt(&self.bus) {
            self.invoke_guest(call, run, blocks, false)?;
        }
        let mut delivered = 0;
        while self.kernel.irql() < Irql::Dispatch
            && delivered < MAX_DPCS_PER_DELIVERY
            && let Some(call) = self.kernel.next_dpc(&mut self.bus)?
        {
            let previous = self.kernel.enter_dpc_level();
            let result = self.invoke_guest(call, run, blocks, true);
            self.kernel.restore_irql(previous);
            result?;
            delivered += 1;
        }
        Ok(())
    }

    /// Chama `call` como `stdcall` na pilha do guest e restaura o contexto ao retornar.
    fn invoke_guest<R>(
        &mut self,
        call: GuestCall,
        run: &mut R,
        blocks: u64,
        interrupts_enabled: bool,
    ) -> Result<()>
    where
        R: FnMut(&mut State, &mut Bus, u64) -> Result<Exit>,
    {
        let previous = self.in_interrupt;
        self.in_interrupt = true;
        let result = self.invoke_guest_inner(call, run, blocks, interrupts_enabled);
        self.in_interrupt = previous;
        result
    }
    fn invoke_guest_inner<R>(
        &mut self,
        call: GuestCall,
        run: &mut R,
        blocks: u64,
        interrupts_enabled: bool,
    ) -> Result<()>
    where
        R: FnMut(&mut State, &mut Bus, u64) -> Result<Exit>,
    {
        let saved = ThreadContext::capture(&self.state);
        let mut esp = self.state.regs[ESP];
        for argument in call.arguments[..call.count].iter().rev() {
            esp = esp.wrapping_sub(4);
            self.bus.escrever_u32(xbox_mem::GuestVirt(esp), *argument)?;
        }
        esp = esp.wrapping_sub(4);
        self.bus
            .escrever_u32(xbox_mem::GuestVirt(esp), RETURN_SENTINEL)?;
        self.state.regs[ESP] = esp;
        self.state.eip = call.entry;
        self.state.returned = false;
        self.state.repeat = None;
        // O gate de interrupção do hardware limpa IF no ISR; o DPC roda com ela ligada.
        if interrupts_enabled {
            self.state.eflags |= INTERRUPT_FLAG;
        } else {
            self.state.eflags &= !INTERRUPT_FLAG;
        }
        for _ in 0..MAX_ROUTINE_SLICES {
            match self.run_slice(run, blocks)? {
                Exit::Returned => {
                    saved.restore(&mut self.state);
                    return Ok(());
                }
                Exit::Budget => {}
                Exit::Breakpoint { pc } => {
                    return Err(Error::GuestBreakpoint {
                        pc,
                        next_eip: self.state.eip,
                    });
                }
                Exit::Halted {
                    pc,
                    interrupts_enabled,
                } => {
                    return Err(Error::GuestHalted {
                        pc,
                        next_eip: self.state.eip,
                        interrupts_enabled,
                    });
                }
                Exit::KernelCall { .. } => {
                    if self.atender_kernel_ou_bloquear()? == Servico::Bloqueado {
                        return Err(Error::BlockedInInterrupt { pc: self.state.eip });
                    }
                }
            }
        }
        Err(Error::InterruptStuck {
            entry: call.entry,
            slices: MAX_ROUTINE_SLICES,
        })
    }
}
