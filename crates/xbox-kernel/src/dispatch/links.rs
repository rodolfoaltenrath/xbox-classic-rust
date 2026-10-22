//! Namespace guest mínimo: \??\D: -> diretório de \Device\CdRom0.
//! Não monta mídia nem cria links no sistema operacional host (ADR-0046).
use super::mutants::Mutants;
use super::{
    Atendimento, Erro, Irql, Resultado, STATUS_ACCESS_VIOLATION, STATUS_INSUFFICIENT_RESOURCES,
    STATUS_INVALID_HANDLE, descrever_argumentos, ram_range,
};
use crate::imports::Call;
use std::collections::BTreeSet;
use xbox_core::{Snapshot, SnapshotReader, SnapshotWriter, error::SnapshotError};
use xbox_mem::{Acesso, Bus, GuestVirt};

pub const STATUS_OBJECT_NAME_NOT_FOUND: u32 = 0xC000_0034;
pub const STATUS_OBJECT_NAME_COLLISION: u32 = 0xC000_0035;
pub const MAX_LINK_HANDLES: usize = 256;
const MAX_PATH_BYTES: usize = 260;

#[derive(Debug, Default)]
pub(super) struct Links {
    target: Option<String>,
    handles: BTreeSet<u32>,
}
impl Links {
    pub(super) fn target(&self) -> Option<&str> {
        self.target.as_deref()
    }
    pub(super) fn contains(&self, handle: u32) -> bool {
        self.handles.contains(&handle)
    }
    pub(super) fn close(&mut self, handle: u32) -> Atendimento {
        Atendimento {
            eax: if self.handles.remove(&handle) {
                0
            } else {
                STATUS_INVALID_HANDLE
            },
        }
    }
    pub(super) fn create(&mut self, bus: &Bus, call: &Call, irql: Irql) -> Resultado {
        passive(bus, call, irql)?;
        let name = string(bus, call, call.argument(bus, 0)?)?;
        let target = string(bus, call, call.argument(bus, 1)?)?;
        if name != b"\\??\\D:" || !valid_target(&target) {
            return Err(variant(
                bus,
                call,
                "perfil só cria D: para diretório de CdRom0",
            ));
        }
        if self.target.is_some() {
            return Ok(Atendimento {
                eax: STATUS_OBJECT_NAME_COLLISION,
            });
        }
        self.target = Some(String::from_utf8(target).expect("caminho ASCII validado"));
        Ok(Atendimento { eax: 0 })
    }
    pub(super) fn open(
        &mut self,
        bus: &mut Bus,
        call: &Call,
        irql: Irql,
        handles: &mut Mutants,
    ) -> Resultado {
        passive(bus, call, irql)?;
        let output = call.argument(bus, 0)?;
        let attributes = call.argument(bus, 1)?;
        if !ram_range(bus, output, 4, Acesso::Escrita)
            || !ram_range(bus, attributes, 12, Acesso::Leitura)
        {
            return Ok(Atendimento {
                eax: STATUS_ACCESS_VIOLATION,
            });
        }
        let root = bus.ler_u32(GuestVirt(attributes))?;
        let name_at = bus.ler_u32(GuestVirt(attributes + 4))?;
        let flags = bus.ler_u32(GuestVirt(attributes + 8))?;
        if root != 0 || flags != 0x40 {
            return Err(variant(
                bus,
                call,
                "exige nome absoluto e OBJ_CASE_INSENSITIVE, sem outros atributos",
            ));
        }
        let name = string(bus, call, name_at)?;
        if !name.eq_ignore_ascii_case(b"\\??\\D:") {
            return Err(variant(bus, call, "consulta fora do namespace D: modelado"));
        }
        if self.target.is_none() {
            return Ok(Atendimento {
                eax: STATUS_OBJECT_NAME_NOT_FOUND,
            });
        }
        let Some((handle, _)) = handles
            .prepare(false)
            .filter(|_| self.handles.len() < MAX_LINK_HANDLES)
        else {
            return Ok(Atendimento {
                eax: STATUS_INSUFFICIENT_RESOURCES,
            });
        };
        // Toda leitura falível ocorreu antes de publicar a saída, que pode ser alias
        // dos argumentos. O contador é o mesmo de mutants e threads.
        bus.escrever_u32(GuestVirt(output), handle)?;
        handles.reserve_handle();
        self.handles.insert(handle);
        Ok(Atendimento { eax: 0 })
    }
    pub(super) fn validate(
        &self,
        handles: &Mutants,
        child: Option<super::GuestThread>,
    ) -> Result<(), SnapshotError> {
        if self.handles.iter().any(|&handle| {
            handle < 4
                || !handle.is_multiple_of(4)
                || handle >= handles.next_handle()
                || handles.get(handle).is_some()
                || child.is_some_and(|thread| thread.handle == handle)
        }) {
            return Err(bad("handle de link inválido ou colide com outro objeto"));
        }
        Ok(())
    }
}
fn passive(bus: &Bus, call: &Call, irql: Irql) -> Result<(), Erro> {
    if irql != Irql::Passive {
        return Err(variant(bus, call, "namespace só é atendido em Passive"));
    }
    Ok(())
}
impl super::Dispatcher {
    pub(super) fn nt_close(&mut self, bus: &Bus, call: &Call) -> Resultado {
        passive(bus, call, self.irql)?;
        let handle = call.argument(bus, 0)?;
        if self.mutants.get(handle).is_some()
            || self
                .child_thread()
                .is_some_and(|thread| thread.handle == handle)
        {
            return Err(variant(
                bus,
                call,
                "NtClose de mutant/thread ainda exige ciclo de vida próprio",
            ));
        }
        // Evento: o handle deixa de existir; reusar o número é problema do contador,
        // que só cresce.
        if self.events.close(handle) {
            return Ok(Atendimento { eax: 0 });
        }
        Ok(self.links.close(handle))
    }
}
fn string(bus: &Bus, call: &Call, at: u32) -> Result<Vec<u8>, Erro> {
    let invalid = || Erro::ArgumentoInvalido {
        nome: call.export.name,
        ordinal: call.export.ordinal,
        detalhe: "OBJECT_STRING inválida ou fora da RAM/faixa de 260 bytes do perfil".to_owned(),
    };
    if !ram_range(bus, at, 8, Acesso::Leitura) {
        return Err(invalid());
    }
    let length = u32::from(bus.ler_u16(GuestVirt(at))?);
    let maximum = u32::from(bus.ler_u16(GuestVirt(at + 2))?);
    let buffer = bus.ler_u32(GuestVirt(at + 4))?;
    if length == 0
        || length > maximum
        || length as usize > MAX_PATH_BYTES
        || !ram_range(bus, buffer, length, Acesso::Leitura)
    {
        return Err(invalid());
    }
    let mut bytes = vec![0; length as usize];
    bus.ler_bytes(GuestVirt(buffer), &mut bytes)?;
    if bytes.contains(&0) {
        return Err(invalid());
    }
    Ok(bytes)
}
fn valid_target(target: &[u8]) -> bool {
    let root = b"\\Device\\CdRom0";
    target.len() >= root.len()
        && target.len() <= MAX_PATH_BYTES
        && target[..root.len()].eq_ignore_ascii_case(root)
        && (target.len() == root.len() || target[root.len()] == b'\\')
        && target.iter().all(|&b| b.is_ascii_graphic() || b == b' ')
        && !target.iter().any(|b| matches!(b, b'/' | b':'))
        && !target
            .split(|&b| b == b'\\')
            .any(|part| part == b"." || part == b"..")
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
fn bad(what: &'static str) -> SnapshotError {
    SnapshotError::BadValue {
        section: "SYML",
        what,
    }
}
impl Snapshot for Links {
    fn save(&self, out: &mut SnapshotWriter) {
        out.section("SYML", 1);
        out.bytes(self.target.as_deref().unwrap_or("").as_bytes());
        out.u32(self.handles.len() as u32);
        for &handle in &self.handles {
            out.u32(handle);
        }
    }
    fn load(&mut self, inp: &mut SnapshotReader<'_>) -> Result<(), SnapshotError> {
        if inp.section("SYML", 1)? != 1 {
            return Err(bad("versão desconhecida"));
        }
        let bytes = inp.bytes()?;
        let target = if bytes.is_empty() {
            None
        } else {
            if !valid_target(bytes) {
                return Err(bad("destino fora do perfil CdRom0"));
            }
            Some(String::from_utf8(bytes.to_vec()).map_err(|_| bad("destino não ASCII"))?)
        };
        let count = inp.u32()? as usize;
        if count > MAX_LINK_HANDLES || count != 0 && target.is_none() {
            return Err(bad("contagem ou objetos sem link"));
        }
        let mut handles = BTreeSet::new();
        let mut previous = 0;
        for _ in 0..count {
            let handle = inp.u32()?;
            if handle < 4 || !handle.is_multiple_of(4) || handle <= previous {
                return Err(bad("handles inválidos, repetidos ou fora de ordem"));
            }
            handles.insert(handle);
            previous = handle;
        }
        *self = Self { target, handles };
        Ok(())
    }
}
