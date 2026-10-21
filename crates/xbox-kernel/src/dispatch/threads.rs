//! Uma filha guest cooperativa, com recurso próprio e continuação explícita.
use super::{
    Atendimento, Erro, Irql, Resultado, STATUS_ACCESS_VIOLATION, STATUS_INVALID_PARAMETER,
    STATUS_NO_MEMORY, STATUS_SUCCESS, VirtualAllocation, VirtualMemory, descrever_argumentos,
    ram_range,
};
use super::{
    mutants::Mutants,
    virtual_memory::{peek_word, physical_word},
};
use crate::imports::Call;
use xbox_core::{Snapshot, SnapshotReader, SnapshotWriter, error::SnapshotError};
use xbox_mem::{Acesso, Bus, GuestVirt};

pub const CHILD_THREAD: u32 = 2;
pub const MAX_THREAD_STACK: u32 = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadPhase {
    Ready,
    Running,
    Terminated(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuestThread {
    pub handle: u32,
    pub region: VirtualAllocation,
    pub stack_size: u32,
    pub tls_size: u32,
    pub start: u32,
    pub context: u32,
    pub entry: u32,
    pub phase: ThreadPhase,
}

impl GuestThread {
    pub const fn stack_top(self) -> u32 {
        self.region.base + self.stack_size
    }
    pub const fn tib(self) -> u32 {
        self.stack_top()
    }
    /// Só o prefixo KTHREAD cabe nesta página; campos além dele não são modelados.
    pub const fn object(self) -> u32 {
        self.tib() + 4096 - 0x2C
    }
    pub const fn tls(self) -> u32 {
        if self.tls_size == 0 {
            0
        } else {
            self.stack_top() - self.tls_size
        }
    }
    pub const fn esp(self) -> u32 {
        self.stack_top() - self.tls_size - 12
    }
    fn valid(self) -> bool {
        self.handle >= 4
            && self.handle.is_multiple_of(4)
            && (65536..=MAX_THREAD_STACK).contains(&self.stack_size)
            && self.stack_size.is_multiple_of(65536)
            && (self.tls_size == 0 || self.tls_size >= 4 && self.tls_size % 16 == 4)
            && self.tls_size <= self.stack_size / 2
            && self.region.base.is_multiple_of(65536)
            && self.region.size == self.stack_size + 4096
            && self
                .region
                .base
                .checked_add(self.region.size)
                .is_some_and(|end| end <= crate::loader::STACK_BASE)
            && self.start != 0
            && self.entry != 0
    }

    fn initialize(self, bus: &mut Bus) -> Result<(), Erro> {
        for (address, value) in [
            (self.tib(), u32::MAX),
            (self.tib() + 4, self.stack_top()),
            (self.tib() + 8, self.region.base),
            (self.tib() + 0x18, self.tib()),
            (self.object() + 0x1C, self.stack_top()),
            (self.object() + 0x20, self.region.base),
            (self.object() + 0x24, self.esp()),
            (self.object() + 0x28, self.tls()),
            (self.esp(), crate::loader::RETURN_SENTINEL),
            (self.esp() + 4, self.start),
            (self.esp() + 8, self.context),
        ] {
            bus.escrever_u32(GuestVirt(address), value)?;
        }
        Ok(())
    }
}

#[derive(Debug, Default)]
pub(super) struct Threads {
    pub child: Option<GuestThread>,
}

impl Threads {
    pub(super) fn current(&self) -> u32 {
        if self
            .child
            .is_some_and(|child| child.phase == ThreadPhase::Running)
        {
            CHILD_THREAD
        } else {
            1
        }
    }
    pub(super) fn create(
        &mut self,
        bus: &mut Bus,
        call: &Call,
        memory: &mut VirtualMemory,
        mutants: &mut Mutants,
        irql: Irql,
    ) -> Resultado {
        let mut args = [0; 10];
        for (i, value) in args.iter_mut().enumerate() {
            *value = call.argument(bus, i as u32)?;
        }
        let [
            output,
            extension,
            stack,
            tls,
            id_out,
            start,
            context,
            suspended,
            debugger,
            entry,
        ] = args;
        if self.child.is_some() {
            return Err(variant(
                bus,
                call,
                "perfil admite uma única filha por sessão",
            ));
        }
        if irql != Irql::Passive
            || extension != 0
            || suspended as u8 != 0
            || debugger as u8 != 0
            || entry == 0
        {
            return Err(variant(
                bus,
                call,
                "exige Passive, extensão zero, execução imediata e SystemRoutine explícita",
            ));
        }
        let Some(stack_size) = stack
            .checked_add(65535)
            .map(|n| n & !65535)
            .filter(|n| (65536..=MAX_THREAD_STACK).contains(n))
        else {
            return Ok(Atendimento {
                eax: STATUS_INVALID_PARAMETER,
            });
        };
        if tls > stack_size / 2 || tls != 0 && (tls < 4 || tls % 16 != 4) {
            return Ok(Atendimento {
                eax: STATUS_INVALID_PARAMETER,
            });
        }
        let status = validate_outputs(bus, output, id_out);
        if status != STATUS_SUCCESS {
            return Ok(Atendimento { eax: status });
        }
        for target in [entry, start] {
            if !executable(memory, bus, target) {
                return Err(variant(
                    bus,
                    call,
                    "rotinas da thread exigem código guest executável imutável",
                ));
            }
        }
        let Some((handle, _)) = mutants.prepare(false) else {
            return Ok(Atendimento {
                eax: STATUS_NO_MEMORY,
            });
        };
        let Some(region) = memory.allocate_resource(bus, stack_size + 4096)? else {
            return Ok(Atendimento {
                eax: STATUS_NO_MEMORY,
            });
        };
        let child = GuestThread {
            handle,
            region,
            stack_size,
            tls_size: tls,
            start,
            context,
            entry,
            phase: ThreadPhase::Ready,
        };
        // Recursos novos são RAM própria zerada; saídas validadas antes do commit.
        child.initialize(bus)?;
        bus.escrever_u32(GuestVirt(output), handle)?;
        if id_out != 0 {
            bus.escrever_u32(GuestVirt(id_out), CHILD_THREAD)?;
        }
        mutants.reserve_handle();
        self.child = Some(child);
        Ok(Atendimento {
            eax: STATUS_SUCCESS,
        })
    }

    pub(super) fn validate(
        &self,
        mutants: &Mutants,
        memory: &VirtualMemory,
        bus: Option<&Bus>,
    ) -> Result<(), SnapshotError> {
        let Some(child) = self.child else {
            return if mutants.owners_valid(false) {
                Ok(())
            } else {
                Err(bad("mutex pertence a thread inexistente"))
            };
        };
        if !child.valid()
            || child.handle >= mutants.next_handle()
            || mutants.get(child.handle).is_some()
            || !mutants.owners_valid(true)
            || !memory.allocations().contains(&child.region)
        {
            return Err(bad("objeto, handle ou região de thread inconsistente"));
        }
        if matches!(child.phase, ThreadPhase::Terminated(_)) && mutants.has_owner(CHILD_THREAD) {
            return Err(bad(
                "thread terminada ainda possui mutant sem abandono modelado",
            ));
        }
        if let Some(bus) = bus {
            if bus
                .mmu()
                .consultar(GuestVirt(child.region.base + child.region.size))
                .is_some()
            {
                return Err(bad("página após prefixo KTHREAD deve permanecer sem mapa"));
            }
            for (at, value) in [
                (child.tib() + 4, child.stack_top()),
                (child.tib() + 8, child.region.base),
                (child.tib() + 0x18, child.tib()),
                (child.object() + 0x1C, child.stack_top()),
                (child.object() + 0x20, child.region.base),
                (child.object() + 0x28, child.tls()),
            ] {
                if peek_word(bus, at) != Some(value) {
                    return Err(bad("TIB/KTHREAD diverge do recurso de thread"));
                }
            }
            for target in [child.start, child.entry] {
                if !executable(memory, bus, target) {
                    return Err(bad("entrada de thread sem código imutável"));
                }
            }
        }
        Ok(())
    }
}

impl Snapshot for Threads {
    fn save(&self, out: &mut SnapshotWriter) {
        out.section("THRD", 1);
        out.bool(self.child.is_some());
        if let Some(child) = self.child {
            for value in [
                child.handle,
                child.region.base,
                child.region.size,
                child.stack_size,
                child.tls_size,
                child.start,
                child.context,
                child.entry,
            ] {
                out.u32(value);
            }
            let (phase, status) = match child.phase {
                ThreadPhase::Ready => (0, 0),
                ThreadPhase::Running => (1, 0),
                ThreadPhase::Terminated(status) => (2, status),
            };
            out.u8(phase);
            out.u32(status);
        }
    }
    fn load(&mut self, inp: &mut SnapshotReader<'_>) -> Result<(), SnapshotError> {
        if inp.section("THRD", 1)? != 1 {
            return Err(bad("versão de threads desconhecida"));
        }
        let child = match inp.u8()? {
            0 => None,
            1 => {
                let mut values = [0; 8];
                for value in &mut values {
                    *value = inp.u32()?;
                }
                let [
                    handle,
                    base,
                    size,
                    stack_size,
                    tls_size,
                    start,
                    context,
                    entry,
                ] = values;
                let phase = inp.u8()?;
                let status = inp.u32()?;
                let phase = match (phase, status) {
                    (0, 0) => ThreadPhase::Ready,
                    (1, 0) => ThreadPhase::Running,
                    (2, status) => ThreadPhase::Terminated(status),
                    _ => return Err(bad("fase de thread inválida")),
                };
                let child = GuestThread {
                    handle,
                    region: VirtualAllocation { base, size },
                    stack_size,
                    tls_size,
                    start,
                    context,
                    entry,
                    phase,
                };
                if !child.valid() {
                    return Err(bad("geometria de thread inválida"));
                }
                Some(child)
            }
            _ => return Err(bad("presença de thread inválida")),
        };
        self.child = child;
        Ok(())
    }
}

pub(super) fn bad(what: &'static str) -> SnapshotError {
    SnapshotError::BadValue {
        section: "THRD",
        what,
    }
}
fn executable(memory: &VirtualMemory, bus: &Bus, target: u32) -> bool {
    memory.image().is_some_and(|image| {
        target >= image.base && u64::from(target) < u64::from(image.base) + u64::from(image.size)
    }) && bus
        .mmu()
        .consultar(GuestVirt(target))
        .is_some_and(|(pa, perm)| {
            (pa.0 as usize) < bus.ram()
                && perm.permite(Acesso::Execucao)
                && !perm.permite(Acesso::Escrita)
        })
}
fn validate_outputs(bus: &Bus, output: u32, id_out: u32) -> u32 {
    if !ram_range(bus, output, 4, Acesso::Escrita)
        || id_out != 0 && !ram_range(bus, id_out, 4, Acesso::Escrita)
    {
        return STATUS_ACCESS_VIOLATION;
    }
    if id_out != 0 {
        let a = physical_word(bus, output);
        let b = physical_word(bus, id_out);
        if a.iter().any(|p| b.contains(p)) {
            return STATUS_INVALID_PARAMETER;
        }
    }
    STATUS_SUCCESS
}
pub(super) fn variant(bus: &Bus, call: &Call, detalhe: &'static str) -> Erro {
    Erro::VarianteNaoImplementada {
        nome: call.export.name,
        ordinal: call.export.ordinal,
        chamador: call.return_pc,
        detalhe,
        argumentos: descrever_argumentos(bus, call),
    }
}
