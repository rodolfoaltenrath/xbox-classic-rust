//! CPU x86-32: decoder + backends interpretador, JIT e hypervisor
//!
//! **Estado:** esqueleto. Implementação na **Fase 3** do [ROADMAP](../../../ROADMAP.md).
//! O oráculo inteiro opcional já existe para validar a recompilação AOT.
//!
//! Este crate é um dos autorizados a usar `unsafe` (ROADMAP §13.3). Toda construção
//! `unsafe` precisa de um comentário `// SAFETY:` declarando a invariante mantida.
#![allow(unsafe_code)]

/// Fase do roadmap em que este subsistema é implementado.
pub const ROADMAP_PHASE: &str = "Fase 3";

/// Referência de teste na CPU física; não é um backend de execução do Xbox.
#[cfg(all(feature = "isa-oracle", target_arch = "x86_64"))]
pub mod oracle;
