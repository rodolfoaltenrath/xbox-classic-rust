//! Reimplementacao HLE do kernel do Xbox (xboxkrnl)
//!
//! Loader de imagem e inspeção pelo bus disponíveis. TLS, resolução de imports e
//! serviços HLE continuam pendentes na Fase 4 do roadmap.

pub mod loader;

/// Fase do roadmap em que este subsistema é implementado.
pub const ROADMAP_PHASE: &str = "Fase 4";
