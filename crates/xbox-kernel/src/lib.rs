//! Reimplementacao HLE do kernel do Xbox (xboxkrnl)
//!
//! Loader, inspeção, TLS nxdk e fronteira ABI de imports disponíveis. Os três dados
//! exportados que o homebrew do nxdk importa têm representação em memória guest
//! (ADR-0022), e o dispatcher executa serviços ou recusa de forma identificável
//! (ADR-0023) — com **1 de 69** serviços implementados. Scheduler de threads e o resto
//! dos serviços continuam pendentes.

pub mod dispatch;
pub mod imports;
pub mod loader;

/// Fase do roadmap em que este subsistema é implementado.
pub const ROADMAP_PHASE: &str = "Fase 4";
