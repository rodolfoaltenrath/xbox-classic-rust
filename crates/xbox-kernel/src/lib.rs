//! Reimplementacao HLE do kernel do Xbox (xboxkrnl)
//!
//! Loader, inspeção, TLS nxdk e fronteira ABI de imports disponíveis. Os três dados
//! exportados que o homebrew do nxdk importa têm representação em memória guest
//! (ADR-0022), e o dispatcher executa serviços ou recusa de forma identificável
//! (ADR-0023). `NtCreateMutant` cria mutexes sem nome com handles guest e snapshot
//! (ADR-0032). SHA-1 Init/Update/Final usa contexto opaco na RAM (ADR-0033):
//! A consulta integral de EEPROM usa a imagem explícita da sessão (ADR-0034):
//! Espera e liberação de mutant, sem bloquear o host (ADR-0035):
//! Seções críticas recursivas em RAM guest (ADR-0037):
//! Leitura da configuração PCI de clock (ADR-0039):
//! Troca de LONG em RAM guest por `InterlockedExchange` (ADR-0040):
//! Tempo de sistema derivado do Clock e referência UTC explícita (ADR-0041):
//! Consulta de IRQL explícito da CPU guest (ADR-0042):
//! Reserva+commit privado em RAM por `NtAllocateVirtualMemory` (ADR-0043):
//! **17 de 69** serviços implementados, com variantes limitadas.
//! Scheduler e o resto dos serviços continuam pendentes.

pub mod dispatch;
pub mod imports;
pub mod loader;

/// Fase do roadmap em que este subsistema é implementado.
pub const ROADMAP_PHASE: &str = "Fase 4";
