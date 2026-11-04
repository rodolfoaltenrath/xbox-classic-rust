//! Esperas por objetos: um motor só para `NtWaitForSingleObject(Ex)` e
//! `NtWaitForMultipleObjectsEx` (ADR-0099, subentrega 12.4c).
//!
//! O contrato vem do nxdk fixado (`xboxkrnl.h`): `Timeout` opcional, onde zero significa
//! "não esperar se não puder ser satisfeita já"; o status devolvido é `STATUS_WAIT_0 +
//! índice` na espera por qualquer um, `STATUS_TIMEOUT` ao expirar. A implementação de
//! referência do Cxbx-Reloaded encaminha as duas esperas simples para a múltipla, o que
//! este motor também faz: uma espera simples é uma múltipla de um objeto.
//!
//! A espera **nunca bloqueia o host**. Se não pode ser satisfeita agora e o perfil tem
//! quem possa sinalizar (scheduler ou interrupção), o serviço pede o bloqueio e a sessão
//! repete a chamada depois; a tentativa seguinte reavalia tudo do zero. O que sobrevive de
//! uma tentativa à outra é só o registro da espera pendente (prazo e thread) e os eventos
//! pelos quais ela espera, para que um pulso a libere.
//!
//! Fora do perfil, e portanto recusado como variante: espera alertável (sem APC),
//! timeout absoluto, handles repetidos, objetos de arquivo e links, e uma segunda espera
//! pendente de outra thread (P19).

use xbox_core::error::SnapshotError;
use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_core::time::Clock;
use xbox_mem::{Acesso, Bus, GuestVirt};

use super::threads::variant;
use super::{
    Atendimento, Erro, Irql, Resultado, STATUS_ACCESS_VIOLATION, STATUS_INVALID_HANDLE,
    STATUS_INVALID_PARAMETER, STATUS_MUTANT_LIMIT_EXCEEDED, STATUS_WAIT_0, ThreadPhase,
    mutants::Aquisicao, ram_range,
};
use crate::imports::Call;

/// `STATUS_TIMEOUT` (ntstatus.h do nxdk).
pub const STATUS_TIMEOUT: u32 = 0x0000_0102;
/// `MAXIMUM_WAIT_OBJECTS`: limite de handles de uma espera múltipla.
pub const MAX_WAIT_OBJECTS: u32 = 64;

/// A espera bloqueada que a sessão vai repetir.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PendingWait {
    /// Thread que espera.
    pub thread: u32,
    /// Prazo absoluto em ciclos do Clock; `None` espera sem limite.
    pub deadline: Option<u64>,
}

#[derive(Debug, Default)]
pub(super) struct Waits {
    pub pending: Option<PendingWait>,
}

impl Snapshot for Waits {
    fn save(&self, out: &mut SnapshotWriter) {
        out.section("WAIT", 1);
        out.bool(self.pending.is_some());
        if let Some(pending) = self.pending {
            out.u32(pending.thread);
            out.bool(pending.deadline.is_some());
            out.u64(pending.deadline.unwrap_or(0));
        }
    }

    fn load(&mut self, inp: &mut SnapshotReader<'_>) -> Result<(), SnapshotError> {
        let bad = |what| SnapshotError::BadValue {
            section: "WAIT",
            what,
        };
        if inp.section("WAIT", 1)? != 1 {
            return Err(bad("versão desconhecida"));
        }
        self.pending = if inp.bool()? {
            let thread = inp.u32()?;
            let has_deadline = inp.bool()?;
            let deadline = inp.u64()?;
            if !matches!(thread, 1 | 2)
                || !has_deadline && deadline != 0
                || has_deadline && deadline == 0
            {
                return Err(bad("espera pendente não canônica"));
            }
            Some(PendingWait {
                thread,
                deadline: has_deadline.then_some(deadline),
            })
        } else {
            None
        };
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Thread,
    Semaphore,
    Event,
    Mutant,
}

enum Classified {
    Target(Target),
    Invalid,
    Refused(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WaitType {
    All,
    Any,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Timeout {
    Infinite,
    Poll,
    Relative(u64),
}

impl super::Dispatcher {
    /// `NTSTATUS NtWaitForSingleObject(HANDLE, BOOLEAN Alertable, PLARGE_INTEGER Timeout)`
    pub(super) fn nt_wait_for_single_object(
        &mut self,
        bus: &mut Bus,
        call: &Call,
        scheduled: bool,
        now: Option<u64>,
    ) -> Resultado {
        let handle = call.argument(bus, 0)?;
        // BOOLEAN ocupa uma palavra na ABI, mas só o byte baixo importa.
        let alertable = call.argument(bus, 1)? as u8 != 0;
        let timeout = call.argument(bus, 2)?;
        self.wait_objects(
            bus,
            call,
            scheduled,
            now,
            &[handle],
            WaitType::Any,
            0,
            alertable,
            timeout,
        )
    }

    /// `NTSTATUS NtWaitForSingleObjectEx(HANDLE, KPROCESSOR_MODE, BOOLEAN, PLARGE_INTEGER)`
    pub(super) fn nt_wait_for_single_object_ex(
        &mut self,
        bus: &mut Bus,
        call: &Call,
        scheduled: bool,
        now: Option<u64>,
    ) -> Resultado {
        let handle = call.argument(bus, 0)?;
        let mode = call.argument(bus, 1)?;
        let alertable = call.argument(bus, 2)? as u8 != 0;
        let timeout = call.argument(bus, 3)?;
        self.wait_objects(
            bus,
            call,
            scheduled,
            now,
            &[handle],
            WaitType::Any,
            mode,
            alertable,
            timeout,
        )
    }

    /// `NTSTATUS NtWaitForMultipleObjectsEx(ULONG Count, PHANDLE, WAIT_TYPE, KPROCESSOR_MODE,
    /// BOOLEAN Alertable, PLARGE_INTEGER Timeout)`
    pub(super) fn nt_wait_for_multiple_objects_ex(
        &mut self,
        bus: &mut Bus,
        call: &Call,
        scheduled: bool,
        now: Option<u64>,
    ) -> Resultado {
        let count = call.argument(bus, 0)?;
        let pointer = call.argument(bus, 1)?;
        let raw_type = call.argument(bus, 2)?;
        let mode = call.argument(bus, 3)?;
        let alertable = call.argument(bus, 4)? as u8 != 0;
        let timeout = call.argument(bus, 5)?;
        if count == 0 || count > MAX_WAIT_OBJECTS || raw_type > 1 {
            return Ok(Atendimento {
                eax: STATUS_INVALID_PARAMETER,
            });
        }
        if !ram_range(bus, pointer, count * 4, Acesso::Leitura) {
            return Ok(Atendimento {
                eax: STATUS_ACCESS_VIOLATION,
            });
        }
        let mut handles = Vec::with_capacity(count as usize);
        for index in 0..count {
            handles.push(bus.ler_u32(GuestVirt(pointer + index * 4))?);
        }
        // `WAIT_TYPE`: WaitAll = 0, WaitAny = 1.
        let kind = if raw_type == 0 {
            WaitType::All
        } else {
            WaitType::Any
        };
        self.wait_objects(
            bus, call, scheduled, now, &handles, kind, mode, alertable, timeout,
        )
    }

    fn classify_wait_target(&self, handle: u32) -> Classified {
        if self
            .child_thread()
            .is_some_and(|child| child.handle == handle && !self.threads.handle_closed)
        {
            return Classified::Target(Target::Thread);
        }
        if self.files.contains(handle) {
            return Classified::Refused(
                "espera de objeto de arquivo fora do perfil de I/O síncrona",
            );
        }
        if self.links.contains(handle) {
            return Classified::Refused("link simbólico não é objeto aguardável");
        }
        if self.semaphores.get(handle).is_some() {
            if self.irql != Irql::Passive {
                return Classified::Refused("espera de semáforo exige Passive");
            }
            return Classified::Target(Target::Semaphore);
        }
        if self.events.get(handle).is_some() {
            return Classified::Target(Target::Event);
        }
        if self.mutants.get(handle).is_some() {
            return Classified::Target(Target::Mutant);
        }
        Classified::Invalid
    }

    /// O objeto satisfaria uma espera agora? Não altera nada.
    fn target_signaled(&self, target: Target, handle: u32) -> bool {
        match target {
            Target::Thread => self
                .child_thread()
                .is_some_and(|child| matches!(child.phase, ThreadPhase::Terminated(_))),
            Target::Semaphore => self.semaphores.peek(handle).unwrap_or(false),
            Target::Event => self.events.peek(handle).unwrap_or(false),
            Target::Mutant => self
                .mutants
                .peek(handle, self.current_thread())
                .unwrap_or(false),
        }
    }

    /// Consome o objeto de uma espera satisfeita. Devolve um status que substitui
    /// `STATUS_WAIT_0 + índice` (estouro da recursão de um mutant).
    fn consume_target(&mut self, target: Target, handle: u32) -> Option<u32> {
        match target {
            Target::Thread => None,
            Target::Semaphore => {
                self.semaphores.acquire(handle);
                None
            }
            Target::Event => {
                self.events.consume(handle);
                None
            }
            Target::Mutant => match self.mutants.acquire(handle, self.current_thread()) {
                Some(Aquisicao::LimiteDeRecursao) => Some(STATUS_MUTANT_LIMIT_EXCEEDED),
                _ => None,
            },
        }
    }

    /// Lê o `Timeout`. `Err(status)` é um erro de ABI (ponteiro); o timeout absoluto e o
    /// estouro do prazo são tratados pelo chamador.
    fn read_wait_timeout(
        bus: &Bus,
        call: &Call,
        pointer: u32,
    ) -> Result<Result<Timeout, u32>, Erro> {
        if pointer == 0 {
            return Ok(Ok(Timeout::Infinite));
        }
        if !ram_range(bus, pointer, 8, Acesso::Leitura) {
            return Ok(Err(STATUS_ACCESS_VIOLATION));
        }
        let value = bus.ler_u64(GuestVirt(pointer))?.cast_signed();
        Ok(Ok(match value {
            0 => Timeout::Poll,
            // Negativo: intervalo relativo em unidades de 100 ns. Um intervalo que não
            // cabe nos ciclos de 64 bits é, na prática, infinito.
            v if v < 0 => Clock::hundred_ns_to_cycles(v.unsigned_abs())
                .map_or(Timeout::Infinite, Timeout::Relative),
            _ => {
                return Err(variant(
                    bus,
                    call,
                    "timeout absoluto exige conversão do tempo de sistema ainda não modelada",
                ));
            }
        }))
    }

    fn finish_wait(&mut self) {
        self.waits.pending = None;
        self.events.clear_waiters();
    }

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    fn wait_objects(
        &mut self,
        bus: &mut Bus,
        call: &Call,
        scheduled: bool,
        now: Option<u64>,
        handles: &[u32],
        kind: WaitType,
        mode: u32,
        alertable: bool,
        timeout_pointer: u32,
    ) -> Resultado {
        if alertable {
            // Espera alertável exige entrega de APC, que não existe.
            return Err(variant(
                bus,
                call,
                "espera alertável exige APCs, ainda não modeladas",
            ));
        }
        if mode > 1 {
            return Ok(Atendimento {
                eax: STATUS_INVALID_PARAMETER,
            });
        }
        if scheduled && self.irql != Irql::Passive {
            return Err(variant(
                bus,
                call,
                "bloqueio escalonado exige Passive neste perfil",
            ));
        }
        let timeout = match Self::read_wait_timeout(bus, call, timeout_pointer)? {
            Ok(timeout) => timeout,
            Err(status) => return Ok(Atendimento { eax: status }),
        };
        let thread = self.current_thread();
        if self
            .waits
            .pending
            .is_some_and(|pending| pending.thread != thread)
        {
            return Err(variant(
                bus,
                call,
                "P19: uma única espera pendente; outra thread já espera e exige filas por thread",
            ));
        }
        let mut targets = Vec::with_capacity(handles.len());
        for (index, &handle) in handles.iter().enumerate() {
            if handles[..index].contains(&handle) {
                return Err(variant(
                    bus,
                    call,
                    "handles repetidos na espera múltipla ainda não modelados",
                ));
            }
            match self.classify_wait_target(handle) {
                Classified::Target(target) => targets.push(target),
                Classified::Invalid => {
                    self.finish_wait();
                    return Ok(Atendimento {
                        eax: STATUS_INVALID_HANDLE,
                    });
                }
                Classified::Refused(detail) => return Err(variant(bus, call, detail)),
            }
        }

        // Tentativa de satisfazer a espera.
        let signaled: Vec<bool> = targets
            .iter()
            .zip(handles)
            .map(|(&target, &handle)| self.target_signaled(target, handle))
            .collect();
        let satisfied = match kind {
            WaitType::Any => signaled
                .iter()
                .position(|&is_signaled| is_signaled)
                .or_else(|| self.events.take_released(handles)),
            WaitType::All => signaled.iter().all(|&is_signaled| is_signaled).then_some(0),
        };
        if let Some(index) = satisfied {
            let mut status = None;
            match kind {
                WaitType::Any if signaled[index] => {
                    status = self.consume_target(targets[index], handles[index]);
                }
                WaitType::Any => {} // liberada por um pulso: nada a consumir
                WaitType::All => {
                    for (&target, &handle) in targets.iter().zip(handles) {
                        status = self.consume_target(target, handle).or(status);
                    }
                }
            }
            self.finish_wait();
            return Ok(Atendimento {
                eax: status.unwrap_or(STATUS_WAIT_0 + index as u32),
            });
        }

        // Não satisfeita agora.
        if timeout == Timeout::Poll {
            self.finish_wait();
            return Ok(Atendimento {
                eax: STATUS_TIMEOUT,
            });
        }
        if matches!(timeout, Timeout::Relative(_)) && !scheduled {
            return Err(variant(
                bus,
                call,
                "espera com timeout exige o scheduler de duas threads",
            ));
        }
        let can_signal = |target: Target| match target {
            Target::Thread | Target::Mutant => scheduled,
            Target::Semaphore | Target::Event => scheduled || self.interrupt_can_signal(bus),
        };
        let unsignaled: Vec<Target> = targets
            .iter()
            .zip(&signaled)
            .filter_map(|(&target, &is_signaled)| (!is_signaled).then_some(target))
            .collect();
        let blockable = match kind {
            WaitType::Any => unsignaled.iter().any(|&target| can_signal(target)),
            WaitType::All => unsignaled.iter().all(|&target| can_signal(target)),
        };
        if !blockable {
            let detail = match unsignaled.iter().find(|&&target| !can_signal(target)) {
                Some(Target::Thread) => {
                    "espera de thread viva exige yield/escalonamento ainda não modelado"
                }
                Some(Target::Mutant) => "espera bloqueante exige scheduler de threads guest",
                Some(Target::Semaphore) => {
                    "espera de semáforo sem sinal exige scheduler ou interrupção"
                }
                _ => "espera de evento não sinalizado exige scheduler ou interrupção",
            };
            return Err(variant(bus, call, detail));
        }
        // O prazo nasce na primeira tentativa e sobrevive às repetições da chamada.
        let deadline = match (self.waits.pending, timeout) {
            (Some(pending), _) => pending.deadline,
            (None, Timeout::Relative(cycles)) => now.and_then(|now| now.checked_add(cycles)),
            (None, _) => None,
        };
        if let (Some(deadline), Some(now)) = (deadline, now)
            && now >= deadline
        {
            self.finish_wait();
            return Ok(Atendimento {
                eax: STATUS_TIMEOUT,
            });
        }
        for (&target, &handle) in targets.iter().zip(handles) {
            if target == Target::Event {
                self.events.register_waiter(handle);
            }
        }
        self.waits.pending = Some(PendingWait { thread, deadline });
        self.pending_block = true;
        self.pending_block_deadline = deadline;
        Ok(Atendimento { eax: STATUS_WAIT_0 })
    }

    /// Só a sessão chama: o prazo (em ciclos) da espera que acabou de bloquear.
    pub fn take_block_deadline(&mut self) -> Option<u64> {
        self.pending_block_deadline.take()
    }
}

/// Seções finais de KRNL: semáforos (v16) e a espera pendente (v17). Separadas só para o
/// `load` do kernel não crescer sem limite; v1–v16 migram para nenhuma espera pendente.
pub(super) fn load_late_sections(
    inp: &mut SnapshotReader<'_>,
    version: u32,
) -> Result<(super::semaphores::Semaphores, Waits), SnapshotError> {
    let semaphores = super::semaphores::load_kernel_version(inp, version)?;
    let mut waits = Waits::default();
    if version >= 17 {
        waits.load(inp)?;
    }
    Ok((semaphores, waits))
}
