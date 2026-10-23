//! NV2A: MMIO, pushbuffer, state tracker, shaders e texturas
//!
//! **Estado:** esqueleto. Implementação na **Fase 5** do [ROADMAP](../../../ROADMAP.md).
//!
//! Este crate é um dos autorizados a usar `unsafe` (ROADMAP §13.3). Toda construção
//! `unsafe` precisa de um comentário `// SAFETY:` declarando a invariante mantida.
#![allow(unsafe_code)]

pub mod pushbuffer;

/// Fase do roadmap em que este subsistema é implementado.
pub const ROADMAP_PHASE: &str = "Fase 5";
