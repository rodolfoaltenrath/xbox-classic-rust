//! Semáforos anônimos guest, com contagem e espera pelo scheduler (ADR-0096).
use super::{
    Atendimento, Dispatcher, Erro, Irql, Resultado, STATUS_ACCESS_VIOLATION,
    STATUS_INSUFFICIENT_RESOURCES, STATUS_INVALID_HANDLE, STATUS_INVALID_PARAMETER,
    descrever_argumentos,
};
use crate::imports::Call;
use std::collections::BTreeMap;
use xbox_core::{Snapshot, SnapshotReader, SnapshotWriter, error::SnapshotError};
use xbox_mem::{Acesso, Bus, GuestVirt};

const MAX_SEMAPHORES: usize = 4096;
const STATUS_SEMAPHORE_LIMIT_EXCEEDED: u32 = 0xC000_0047;

pub(super) fn load_kernel_version(
    inp: &mut SnapshotReader<'_>,
    version: u32,
) -> Result<Semaphores, SnapshotError> {
    let mut result = Semaphores::default();
    // KRNL <= 15 não continha SEMA: nunca herdar a tabela da sessão de destino.
    if version >= 16 {
        result.load(inp)?;
    }
    Ok(result)
}
#[derive(Debug, Default)]
pub(super) struct Semaphores {
    objects: BTreeMap<u32, (u32, u32)>,
}
impl Semaphores {
    pub(super) fn get(&self, handle: u32) -> Option<(u32, u32)> {
        self.objects.get(&handle).copied()
    }
    pub(super) fn close(&mut self, handle: u32) -> bool {
        self.objects.remove(&handle).is_some()
    }
    pub(super) fn acquire(&mut self, handle: u32) -> Option<bool> {
        let (count, _) = self.objects.get_mut(&handle)?;
        if *count == 0 {
            Some(false)
        } else {
            *count -= 1;
            Some(true)
        }
    }
    pub(super) fn validate(&self, d: &Dispatcher) -> Result<(), SnapshotError> {
        for &handle in self.objects.keys() {
            if handle < 4
                || !handle.is_multiple_of(4)
                || handle >= d.mutants.next_handle()
                || d.mutants.get(handle).is_some()
                || d.events.get(handle).is_some()
                || d.links.contains(handle)
                || d.files.contains(handle)
                || d.threads.child.is_some_and(|child| child.handle == handle)
            {
                return Err(bad("handle de semáforo inválido ou colidente"));
            }
        }
        Ok(())
    }
}
impl Snapshot for Semaphores {
    fn save(&self, out: &mut SnapshotWriter) {
        out.section("SEMA", 1);
        out.u32(self.objects.len() as u32);
        for (&handle, &(count, maximum)) in &self.objects {
            out.u32(handle);
            out.u32(count);
            out.u32(maximum);
        }
    }
    fn load(&mut self, inp: &mut SnapshotReader<'_>) -> Result<(), SnapshotError> {
        if inp.section("SEMA", 1)? != 1 {
            return Err(bad("versão desconhecida"));
        }
        let count = inp.u32()? as usize;
        if count > MAX_SEMAPHORES {
            return Err(bad("limite de semáforos"));
        }
        let mut objects = BTreeMap::new();
        let mut previous = 0;
        for _ in 0..count {
            let handle = inp.u32()?;
            let value = inp.u32()?;
            let maximum = inp.u32()?;
            if handle < 4
                || !handle.is_multiple_of(4)
                || handle <= previous
                || maximum == 0
                || maximum > i32::MAX as u32
                || value > maximum
            {
                return Err(bad("handle, ordem ou contagem inválidos"));
            }
            objects.insert(handle, (value, maximum));
            previous = handle;
        }
        self.objects = objects;
        Ok(())
    }
}
fn bad(what: &'static str) -> SnapshotError {
    SnapshotError::BadValue {
        section: "SEMA",
        what,
    }
}
fn variant(bus: &Bus, call: &Call, detalhe: &'static str) -> Erro {
    Erro::VarianteNaoImplementada {
        nome: call.export.name,
        ordinal: call.export.ordinal,
        chamador: call.return_pc,
        detalhe,
        argumentos: descrever_argumentos(bus, call),
    }
}
fn ram(bus: &Bus, at: u32, n: u32, access: Acesso) -> bool {
    at != 0
        && (0..n).all(|offset| {
            at.checked_add(offset)
                .and_then(|va| bus.mmu().consultar(GuestVirt(va)))
                .is_some_and(|(pa, perm)| perm.permite(access) && (pa.0 as usize) < bus.ram())
        })
}
impl Dispatcher {
    pub(super) fn create_semaphore(&mut self, bus: &mut Bus, call: &Call) -> Resultado {
        let output = call.argument(bus, 0)?;
        let attrs = call.argument(bus, 1)?;
        let initial = call.argument(bus, 2)?;
        let maximum = call.argument(bus, 3)?;
        if self.irql != Irql::Passive {
            return Err(variant(bus, call, "semáforos exigem Passive"));
        }
        if !ram(bus, output, 4, Acesso::Escrita)
            || attrs != 0 && !ram(bus, attrs, 12, Acesso::Leitura)
        {
            return Ok(Atendimento {
                eax: STATUS_ACCESS_VIOLATION,
            });
        }
        if attrs != 0 {
            let mut bytes = [0; 12];
            bus.ler_bytes(GuestVirt(attrs), &mut bytes)?;
            if bytes != [0; 12] {
                return Err(variant(
                    bus,
                    call,
                    "somente semáforos anônimos, sem atributos",
                ));
            }
        }
        if maximum == 0 || maximum > i32::MAX as u32 || initial > maximum {
            return Ok(Atendimento {
                eax: STATUS_INVALID_PARAMETER,
            });
        }
        let handle = self.mutants.next_handle();
        if self.semaphores.objects.len() >= MAX_SEMAPHORES || handle.checked_add(4).is_none() {
            return Ok(Atendimento {
                eax: STATUS_INSUFFICIENT_RESOURCES,
            });
        }
        bus.escrever_u32(GuestVirt(output), handle)?;
        self.mutants.reserve_handle();
        self.semaphores.objects.insert(handle, (initial, maximum));
        Ok(Atendimento { eax: 0 })
    }
    pub(super) fn release_semaphore(&mut self, bus: &mut Bus, call: &Call) -> Resultado {
        let handle = call.argument(bus, 0)?;
        let increment = call.argument(bus, 1)?;
        let previous = call.argument(bus, 2)?;
        if self.irql != Irql::Passive {
            return Err(variant(bus, call, "semáforos exigem Passive"));
        }
        if previous != 0 && !ram(bus, previous, 4, Acesso::Escrita) {
            return Ok(Atendimento {
                eax: STATUS_ACCESS_VIOLATION,
            });
        }
        let Some((count, maximum)) = self.semaphores.get(handle) else {
            return Ok(Atendimento {
                eax: STATUS_INVALID_HANDLE,
            });
        };
        if increment == 0 || increment > i32::MAX as u32 {
            return Ok(Atendimento {
                eax: STATUS_INVALID_PARAMETER,
            });
        }
        if increment > maximum - count {
            return Ok(Atendimento {
                eax: STATUS_SEMAPHORE_LIMIT_EXCEEDED,
            });
        }
        if previous != 0 {
            bus.escrever_u32(GuestVirt(previous), count)?;
        }
        self.semaphores
            .objects
            .insert(handle, (count + increment, maximum));
        Ok(Atendimento { eax: 0 })
    }
}
