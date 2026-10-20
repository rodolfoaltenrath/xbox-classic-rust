//! Reimplementacao HLE do kernel do Xbox (xboxkrnl)
//!
//! Loader, inspeção, TLS nxdk e fronteira ABI de imports disponíveis. Dados
//! exportados, scheduler e serviços HLE continuam pendentes na Fase 4.

pub mod imports;
pub mod loader;

/// Fase do roadmap em que este subsistema é implementado.
pub const ROADMAP_PHASE: &str = "Fase 4";
