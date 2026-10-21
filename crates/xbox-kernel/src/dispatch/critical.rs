//! Seções críticas recursivas, inteiramente na RAM guest (ADR-0037).
//!
//! Layout e assinaturas: nxdk 14d5ee9, lib/xboxkrnl/{xboxdef,xboxkrnl}.h (CC0).
//! O evento embutido tem lista vazia e não sinalizada neste perfil sem scheduler.
//! `OwningThread` contém a identidade guest ativa (1/2), não um ponteiro KTHREAD.

use xbox_mem::{Acesso, Bus, GuestVirt};

use super::{Atendimento, Erro, Resultado, descrever_argumentos, ram_range};
use crate::imports::Call;

pub const SECTION_SIZE: usize = 28;
const EVENT_HEADER: u32 = 0x0004_0001; // SynchronizationEvent, sizeof(KEVENT)/4.
const LOCK: usize = 16;
const RECURSION: usize = 20;
const OWNER: usize = 24;

struct Section {
    bytes: [u8; SECTION_SIZE],
}

impl Section {
    fn word(&self, offset: usize) -> u32 {
        let mut word = [0; 4];
        word.copy_from_slice(&self.bytes[offset..offset + 4]);
        u32::from_le_bytes(word)
    }

    fn set(&mut self, offset: usize, value: u32) {
        self.bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn read(bus: &Bus, call: &Call, address: u32) -> Result<Self, Erro> {
        validate_range(bus, call, address, Acesso::Leitura)?;
        validate_range(bus, call, address, Acesso::Escrita)?;
        let mut section = Self {
            bytes: [0; SECTION_SIZE],
        };
        bus.ler_bytes(GuestVirt(address), &mut section.bytes)?;
        if section.word(0) != EVENT_HEADER {
            return Err(invalid(
                call,
                "evento de seção crítica não inicializado ou inválido",
            ));
        }
        if section.word(4) != 0 || section.word(8) != address + 8 || section.word(12) != address + 8
        {
            return Err(variant(
                bus,
                call,
                "evento sinalizado ou fila de espera exige scheduler guest",
            ));
        }
        let lock = section.word(LOCK).cast_signed();
        let recursion = section.word(RECURSION).cast_signed();
        let owner = section.word(OWNER);
        let free = lock == -1 && recursion == 0 && owner == 0;
        let held = lock >= 0 && recursion > 0 && owner != 0 && lock >= recursion - 1;
        if !free && !held {
            return Err(invalid(
                call,
                "LockCount, RecursionCount e OwningThread incoerentes",
            ));
        }
        Ok(section)
    }

    fn has_waiters(&self) -> bool {
        self.word(LOCK).wrapping_add(1) != self.word(RECURSION)
    }

    fn write(&self, bus: &mut Bus, address: u32) -> Resultado {
        // Todos os bytes já foram validados como RAM gravável; sem efeitos MMIO.
        bus.escrever_bytes(GuestVirt(address), &self.bytes)?;
        Ok(Atendimento { eax: 0 }) // VOID; TryEnter troca para BOOLEAN após o commit.
    }
}

pub(super) fn initialize(bus: &mut Bus, call: &Call) -> Resultado {
    let address = call.argument(bus, 0)?;
    validate_range(bus, call, address, Acesso::Escrita)?;
    let mut section = Section {
        bytes: [0; SECTION_SIZE],
    };
    section.set(0, EVENT_HEADER);
    section.set(8, address + 8);
    section.set(12, address + 8);
    section.set(LOCK, u32::MAX);
    section.write(bus, address)
}

pub(super) fn enter(bus: &mut Bus, call: &Call, attempt: bool, current: u32) -> Resultado {
    let address = call.argument(bus, 0)?;
    let mut section = Section::read(bus, call, address)?;
    let owner = section.word(OWNER);
    if owner != 0 && owner != current {
        if attempt {
            return Ok(Atendimento { eax: 0 }); // FALSE, sem adquirir nem alterar.
        }
        return Err(variant(
            bus,
            call,
            "entrada bloqueante exige scheduler de threads guest",
        ));
    }
    if section.has_waiters() {
        return Err(variant(
            bus,
            call,
            "seção com esperadores exige scheduler de threads guest",
        ));
    }
    let recursion = section.word(RECURSION);
    if recursion == i32::MAX as u32 {
        return Err(variant(
            bus,
            call,
            "limite de recursão exige exceção guest ainda não modelada",
        ));
    }
    section.set(LOCK, recursion);
    section.set(RECURSION, recursion + 1);
    section.set(OWNER, current);
    section.write(bus, address)?;
    Ok(Atendimento {
        eax: u32::from(attempt),
    })
}

pub(super) fn leave(bus: &mut Bus, call: &Call, current: u32) -> Resultado {
    let address = call.argument(bus, 0)?;
    let mut section = Section::read(bus, call, address)?;
    if section.word(OWNER) != current {
        return Err(invalid(
            call,
            "liberação sem propriedade pela thread guest atual",
        ));
    }
    if section.has_waiters() {
        return Err(variant(
            bus,
            call,
            "liberação com esperadores exige scheduler de threads guest",
        ));
    }
    let recursion = section.word(RECURSION) - 1;
    section.set(RECURSION, recursion);
    section.set(LOCK, recursion.wrapping_sub(1));
    if recursion == 0 {
        section.set(OWNER, 0);
    }
    section.write(bus, address)
}

fn validate_range(bus: &Bus, call: &Call, address: u32, access: Acesso) -> Result<(), Erro> {
    if !ram_range(bus, address, SECTION_SIZE as u32, access) {
        return Err(invalid(
            call,
            "CriticalSection deve ocupar 28 bytes válidos de RAM guest",
        ));
    }
    Ok(())
}

fn invalid(call: &Call, detail: &str) -> Erro {
    Erro::ArgumentoInvalido {
        nome: call.export.name,
        ordinal: call.export.ordinal,
        detalhe: detail.to_owned(),
    }
}

fn variant(bus: &Bus, call: &Call, detail: &'static str) -> Erro {
    Erro::VarianteNaoImplementada {
        nome: call.export.name,
        ordinal: call.export.ordinal,
        chamador: call.return_pc,
        detalhe: detail,
        argumentos: descrever_argumentos(bus, call),
    }
}
