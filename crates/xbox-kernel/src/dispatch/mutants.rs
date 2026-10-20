//! Objetos guest, sem mutexes ou handles do sistema operacional host.

use std::collections::BTreeMap;
use xbox_core::error::SnapshotError;
use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};

/// Identidade reservada para a única thread do perfil AOT atual.
/// O scheduler futuro deverá fornecer a identidade da thread em execução.
pub const INITIAL_THREAD: u32 = 1;
/// Limite de recursos do perfil, independente da memória disponível no host.
pub const MAX_MUTANTS: usize = 4096;

/// Limite da contagem de recursão.
///
/// O campo correspondente no kernel é `LONG`, então o estouro acontece em `i32::MAX` —
/// não em `u32::MAX`. Passar disso devolve `STATUS_MUTANT_LIMIT_EXCEEDED` em vez de
/// dar wrap na contagem.
pub const MAX_RECURSION: u32 = i32::MAX as u32;

/// Estado de um mutex recursivo guest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mutant {
    pub owner: Option<u32>,
    pub recursion: u32,
}

/// Resultado de tentar liberar um mutex.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Liberacao {
    /// Liberado; `anterior` é a contagem antes da liberação e `livre` diz se chegou a
    /// zero e o mutex passou a não ter proprietário.
    Liberado { anterior: u32, livre: bool },
    /// Quem chamou não é a thread proprietária — inclui o caso de mutex livre.
    NaoEraProprietario,
}

/// Resultado de tentar adquirir um mutex.
///
/// A espera **não** bloqueia a thread do host: ou a aquisição é imediata, ou é um
/// estado que este perfil não representa e que vira erro identificável.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aquisicao {
    /// Adquirido agora ou por recursão; a contagem resultante acompanha.
    Adquirido(u32),
    /// Pertence a outra thread: exigiria bloquear e escalonar, e não há scheduler.
    OutroProprietario(u32),
    /// A contagem de recursão estourou [`MAX_RECURSION`].
    LimiteDeRecursao,
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

    /// Tenta adquirir o mutex para `thread`.
    ///
    /// `None` quando o handle não existe. O estado só é alterado em
    /// [`Aquisicao::Adquirido`]: os outros casos não deixam efeito.
    pub(super) fn acquire(&mut self, handle: u32, thread: u32) -> Option<Aquisicao> {
        let object = self.objects.get_mut(&handle)?;
        Some(match object.owner {
            // Livre: a aquisição é imediata e a contagem começa em 1.
            None => {
                object.owner = Some(thread);
                object.recursion = 1;
                Aquisicao::Adquirido(1)
            }
            // Já é da mesma thread: mutant é recursivo, então a contagem sobe.
            Some(owner) if owner == thread => {
                if object.recursion >= MAX_RECURSION {
                    Aquisicao::LimiteDeRecursao
                } else {
                    object.recursion += 1;
                    Aquisicao::Adquirido(object.recursion)
                }
            }
            // De outra thread: só progrediria bloqueando, e não há scheduler.
            Some(owner) => Aquisicao::OutroProprietario(owner),
        })
    }

    /// Libera um nível de recursão do mutex.
    ///
    /// `None` quando o handle não existe. O estado só muda quando a thread é a
    /// proprietária.
    pub(super) fn release(&mut self, handle: u32, thread: u32) -> Option<Liberacao> {
        let object = self.objects.get_mut(&handle)?;
        Some(match object.owner {
            Some(owner) if owner == thread => {
                let anterior = object.recursion;
                object.recursion -= 1;
                let livre = object.recursion == 0;
                if livre {
                    object.owner = None;
                }
                Liberacao::Liberado { anterior, livre }
            }
            // Livre ou de outra thread: liberar seria corromper a posse.
            _ => Liberacao::NaoEraProprietario,
        })
    }
}

impl Snapshot for Mutants {
    fn save(&self, out: &mut SnapshotWriter) {
        // v2 permite recursão acima de 1, que a espera do ADR-0035 passou a produzir.
        // A v1 só representava os estados iniciais de criação.
        out.section("KMUT", 2);
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
        let version = inp.section("KMUT", 2)?;
        if !matches!(version, 1 | 2) {
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
            // v1 só sabia gravar os estados de criação; v2 aceita recursão adquirida.
            // As duas exigem coerência entre proprietário e contagem, e só a
            // identidade de thread que existe neste perfil.
            let coerente = match (owner, recursion) {
                // Livre com contagem zero, ou adquirido uma vez: os dois estados que a
                // v1 sabia gravar.
                (0, 0) | (INITIAL_THREAD, 1) => true,
                // Recursão acima de 1 só a partir da v2, que a espera introduziu.
                (INITIAL_THREAD, n) => version >= 2 && (2..=MAX_RECURSION).contains(&n),
                _ => false,
            };
            if !coerente {
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
