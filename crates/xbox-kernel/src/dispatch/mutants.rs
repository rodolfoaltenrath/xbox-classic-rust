//! Objetos guest, sem mutexes ou handles do sistema operacional host.

use std::collections::BTreeMap;
use xbox_core::error::SnapshotError;
use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};

/// Identidade reservada para a única thread do perfil AOT atual.
/// O scheduler futuro deverá fornecer a identidade da thread em execução.
pub const INITIAL_THREAD: u32 = 1;
/// Limite de recursos do perfil, independente da memória disponível no host.
pub const MAX_MUTANTS: usize = 4096;

/// Estado de um mutex recursivo guest. Criar não implementa espera ou liberação.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mutant {
    pub owner: Option<u32>,
    pub recursion: u32,
}

#[derive(Debug)]
pub(super) struct Mutants {
    next: u32,
    objects: BTreeMap<u32, Mutant>,
}

impl Default for Mutants {
    fn default() -> Self {
        Self {
            next: 4,
            objects: BTreeMap::new(),
        }
    }
}

impl Mutants {
    pub(super) fn get(&self, handle: u32) -> Option<&Mutant> {
        self.objects.get(&handle)
    }

    /// Reserva lógica, ainda sem efeito: o handle só existe após o store guest.
    pub(super) fn prepare(&self, initial_owner: bool) -> Option<(u32, Mutant)> {
        if self.objects.len() >= MAX_MUTANTS || self.next.checked_add(4).is_none() {
            return None;
        }
        Some((
            self.next,
            Mutant {
                owner: initial_owner.then_some(INITIAL_THREAD),
                recursion: u32::from(initial_owner),
            },
        ))
    }

    pub(super) fn commit(&mut self, handle: u32, object: Mutant) {
        self.objects.insert(handle, object);
        self.next += 4;
    }
}

impl Snapshot for Mutants {
    fn save(&self, out: &mut SnapshotWriter) {
        out.section("KMUT", 1);
        out.u32(self.next);
        out.u32(self.objects.len() as u32);
        for (&handle, object) in &self.objects {
            out.u32(handle);
            out.u32(object.owner.unwrap_or(0));
            out.u32(object.recursion);
        }
    }

    fn load(&mut self, inp: &mut SnapshotReader<'_>) -> Result<(), SnapshotError> {
        let bad = |what| SnapshotError::BadValue {
            section: "KMUT",
            what,
        };
        if inp.section("KMUT", 1)? != 1 {
            return Err(bad("versão de objetos desconhecida"));
        }
        let next = inp.u32()?;
        let count = inp.u32()?;
        if next < 4 || !next.is_multiple_of(4) || count as usize > MAX_MUTANTS {
            return Err(bad("limite ou próximo handle inválido"));
        }
        let mut objects = BTreeMap::new();
        let mut previous = 0;
        for _ in 0..count {
            let handle = inp.u32()?;
            let owner = inp.u32()?;
            let recursion = inp.u32()?;
            if handle <= previous || handle >= next || !handle.is_multiple_of(4) {
                return Err(bad("handle inválido, duplicado ou fora de ordem"));
            }
            if !matches!((owner, recursion), (0, 0) | (INITIAL_THREAD, 1)) {
                return Err(bad(
                    "proprietário ou recursão incompatível com o perfil inicial",
                ));
            }
            objects.insert(
                handle,
                Mutant {
                    owner: (owner != 0).then_some(owner),
                    recursion,
                },
            );
            previous = handle;
        }
        *self = Self { next, objects };
        Ok(())
    }
}
