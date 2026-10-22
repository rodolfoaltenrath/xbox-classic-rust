//! Eventos do kernel (`NtCreateEvent` e família), sem objetos nem threads do host.
//!
//! Um evento é um bit de sinalização com um tipo. A espera por um evento **não
//! sinalizado** só progrediria se outra thread ou uma interrupção o sinalizasse, e este
//! perfil ainda não tem scheduler nem entrega de interrupções: isso vira erro
//! identificável, não um bloqueio. Quem sinaliza *antes* de esperar, que é o caso do
//! `NtSetEvent` seguido de `NtWaitForSingleObject`, funciona.

use std::collections::BTreeMap;

use xbox_core::error::SnapshotError;
use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_mem::{Acesso, Bus, GuestVirt};

use super::links::Links;
use super::mutants::Mutants;
use super::{
    Atendimento, Erro, Resultado, STATUS_ACCESS_VIOLATION, STATUS_INSUFFICIENT_RESOURCES,
    STATUS_INVALID_HANDLE, STATUS_SUCCESS, descrever_argumentos, ram_range,
};
use crate::imports::Call;

/// Limite de recursos do perfil, independente da memória do host.
pub(super) const MAX_EVENTS: usize = 4096;
/// `STATUS_INVALID_PARAMETER`.
pub(super) const STATUS_INVALID_PARAMETER: u32 = 0xC000_000D;

/// `EVENT_TYPE`: o que acontece com um evento sinalizado quando uma espera o satisfaz.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EventKind {
    /// `NotificationEvent` (0): fica sinalizado até alguém limpá-lo.
    Notification,
    /// `SynchronizationEvent` (1): a espera satisfeita o limpa.
    Synchronization,
}

impl EventKind {
    const fn from_raw(raw: u32) -> Option<Self> {
        match raw {
            0 => Some(Self::Notification),
            1 => Some(Self::Synchronization),
            _ => None,
        }
    }
}

/// Estado de um evento.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Event {
    pub(super) kind: EventKind,
    pub(super) signaled: bool,
}

/// Resultado de esperar um evento sem bloquear.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Espera {
    /// Estava sinalizado; um evento de sincronização foi limpo.
    Satisfeita,
    /// Não sinalizado: só progrediria com scheduler ou interrupção.
    Bloquearia,
}

#[derive(Debug, Default)]
pub(super) struct Events {
    objects: BTreeMap<u32, Event>,
}

impl Events {
    pub(super) fn get(&self, handle: u32) -> Option<Event> {
        self.objects.get(&handle).copied()
    }

    pub(super) fn is_full(&self) -> bool {
        self.objects.len() >= MAX_EVENTS
    }

    pub(super) fn insert(&mut self, handle: u32, event: Event) {
        self.objects.insert(handle, event);
    }

    /// Sinaliza e devolve o estado anterior; `None` se o handle não existe.
    pub(super) fn set(&mut self, handle: u32) -> Option<bool> {
        let event = self.objects.get_mut(&handle)?;
        Some(std::mem::replace(&mut event.signaled, true))
    }

    /// Limpa e devolve o estado anterior.
    pub(super) fn clear(&mut self, handle: u32) -> Option<bool> {
        let event = self.objects.get_mut(&handle)?;
        Some(std::mem::replace(&mut event.signaled, false))
    }

    /// Espera sem bloquear. O estado só muda quando a espera é satisfeita.
    pub(super) fn wait(&mut self, handle: u32) -> Option<Espera> {
        let event = self.objects.get_mut(&handle)?;
        Some(if event.signaled {
            if event.kind == EventKind::Synchronization {
                event.signaled = false;
            }
            Espera::Satisfeita
        } else {
            Espera::Bloquearia
        })
    }

    pub(super) fn close(&mut self, handle: u32) -> bool {
        self.objects.remove(&handle).is_some()
    }

    /// Handles de evento não podem colidir com os de outros objetos nem passar do
    /// próximo handle a ser emitido.
    pub(super) fn validate(
        &self,
        mutants: &Mutants,
        links: &Links,
        child: Option<super::GuestThread>,
    ) -> Result<(), SnapshotError> {
        if self.objects.keys().any(|&handle| {
            handle < 4
                || !handle.is_multiple_of(4)
                || handle >= mutants.next_handle()
                || mutants.get(handle).is_some()
                || links.contains(handle)
                || child.is_some_and(|thread| thread.handle == handle)
        }) {
            return Err(bad("handle de evento inválido ou colide com outro objeto"));
        }
        Ok(())
    }
}

fn bad(what: &'static str) -> SnapshotError {
    SnapshotError::BadValue {
        section: "KEVT",
        what,
    }
}

impl Snapshot for Events {
    fn save(&self, out: &mut SnapshotWriter) {
        out.section("KEVT", 1);
        out.u32(self.objects.len() as u32);
        for (&handle, event) in &self.objects {
            out.u32(handle);
            out.u32(match event.kind {
                EventKind::Notification => 0,
                EventKind::Synchronization => 1,
            });
            out.bool(event.signaled);
        }
    }

    fn load(&mut self, inp: &mut SnapshotReader<'_>) -> Result<(), SnapshotError> {
        if inp.section("KEVT", 1)? != 1 {
            return Err(bad("versão desconhecida"));
        }
        let count = inp.u32()? as usize;
        if count > MAX_EVENTS {
            return Err(bad("limite de eventos excedido"));
        }
        let mut objects = BTreeMap::new();
        let mut previous = 0;
        for _ in 0..count {
            let handle = inp.u32()?;
            let kind = EventKind::from_raw(inp.u32()?).ok_or_else(|| bad("tipo desconhecido"))?;
            let signaled = inp.bool()?;
            if handle <= previous {
                return Err(bad("handle duplicado ou fora de ordem"));
            }
            objects.insert(handle, Event { kind, signaled });
            previous = handle;
        }
        self.objects = objects;
        Ok(())
    }
}

impl super::Dispatcher {
    /// `NTSTATUS NtCreateEvent(PHANDLE, POBJECT_ATTRIBUTES, EVENT_TYPE, BOOLEAN InitialState)`
    ///
    /// Sem `DesiredAccess`, como o `NtCreateMutant` do Xbox. Atributos com nome ou
    /// diretório ainda não são modelados e viram variante recusada, não objeto anônimo.
    pub(super) fn nt_create_event(&mut self, bus: &mut Bus, call: &Call) -> Resultado {
        let output = call.argument(bus, 0)?;
        let attributes = call.argument(bus, 1)?;
        let raw_kind = call.argument(bus, 2)?;
        // BOOLEAN é de 8 bits, embora ocupe uma palavra na ABI.
        let initial = call.argument(bus, 3)? as u8 != 0;
        if attributes != 0 {
            if !ram_range(bus, attributes, 12, Acesso::Leitura) {
                return Ok(Atendimento {
                    eax: STATUS_ACCESS_VIOLATION,
                });
            }
            let mut bytes = [0; 12];
            bus.ler_bytes(GuestVirt(attributes), &mut bytes)?;
            if bytes != [0; 12] {
                return Err(Erro::VarianteNaoImplementada {
                    nome: call.export.name,
                    ordinal: call.export.ordinal,
                    chamador: call.return_pc,
                    detalhe: "nomes, diretórios e flags de OBJECT_ATTRIBUTES ainda não modelados",
                    argumentos: descrever_argumentos(bus, call),
                });
            }
        }
        let Some(kind) = EventKind::from_raw(raw_kind) else {
            return Ok(Atendimento {
                eax: STATUS_INVALID_PARAMETER,
            });
        };
        if !ram_range(bus, output, 4, Acesso::Escrita) {
            return Ok(Atendimento {
                eax: STATUS_ACCESS_VIOLATION,
            });
        }
        let handle = self.mutants.next_handle();
        if self.events.is_full() || handle.checked_add(4).is_none() {
            return Ok(Atendimento {
                eax: STATUS_INSUFFICIENT_RESOURCES,
            });
        }
        // O store é validado inteiro pelo Bus antes de escrever: uma falta não publica o
        // objeto nem consome o handle.
        bus.escrever_bytes(GuestVirt(output), &handle.to_le_bytes())?;
        self.mutants.reserve_handle();
        self.events.insert(
            handle,
            Event {
                kind,
                signaled: initial,
            },
        );
        Ok(Atendimento {
            eax: STATUS_SUCCESS,
        })
    }

    /// `NtSetEvent(HANDLE, PLONG PreviousState)`, `NtClearEvent(HANDLE)` e
    /// `NtPulseEvent(HANDLE, PLONG PreviousState)`.
    ///
    /// `PreviousState` é o estado de sinalização anterior (0 ou 1) por definição do
    /// serviço, ao contrário do `PreviousCount` do mutant, que depende de uma convenção
    /// não verificada. O pulso, sem threads esperando, deixa o evento limpo: não há
    /// espera a liberar.
    pub(super) fn nt_event_operation(
        &mut self,
        bus: &mut Bus,
        call: &Call,
        operation: EventOperation,
    ) -> Resultado {
        let handle = call.argument(bus, 0)?;
        let previous_pointer = match operation {
            EventOperation::Clear => 0,
            EventOperation::Set | EventOperation::Pulse => call.argument(bus, 1)?,
        };
        if previous_pointer != 0 && !ram_range(bus, previous_pointer, 4, Acesso::Escrita) {
            return Ok(Atendimento {
                eax: STATUS_ACCESS_VIOLATION,
            });
        }
        // O store de saída precede a mudança de estado: uma falta não deixa o evento
        // alterado sem que o guest receba o valor anterior.
        let Some(current) = self.events.get(handle) else {
            return Ok(Atendimento {
                eax: STATUS_INVALID_HANDLE,
            });
        };
        if previous_pointer != 0 {
            bus.escrever_bytes(
                GuestVirt(previous_pointer),
                &u32::from(current.signaled).to_le_bytes(),
            )?;
        }
        match operation {
            EventOperation::Set => self.events.set(handle),
            EventOperation::Clear | EventOperation::Pulse => self.events.clear(handle),
        };
        Ok(Atendimento {
            eax: STATUS_SUCCESS,
        })
    }
}

/// O que `nt_event_operation` faz com o evento.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EventOperation {
    Set,
    Clear,
    Pulse,
}
