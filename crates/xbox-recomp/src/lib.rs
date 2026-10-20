//! Recompilação estática experimental: XBE → IR → Rust → executável nativo.
//!
//! O decoder só participa da tradução. A execução gerada usa o bus do Obsidian e
//! estado explícito, sem pilha nativa para representar chamadas do guest.

pub mod analysis;
pub mod emit;
pub mod ir;
pub mod runtime;

/// Motivo de #DE. O AOT devolve o erro ao host; ainda não há entrega via IDT.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DivideFault {
    #[error("divisor zero")]
    ZeroDivisor,
    #[error("quociente fora da largura do destino")]
    QuotientOverflow,
}

/// Programa de demonstração escrito em bytes x86 públicos pelo próprio projeto:
/// soma 1..=10, chama uma rotina que soma 7, grava 62 em 0x12020 e retorna.
pub const DEMO_CODE: &[u8] = &[
    0x31, 0xC0, // xor eax,eax
    0xB9, 10, 0, 0, 0, // mov ecx,10
    0x01, 0xC8, // add eax,ecx
    0x83, 0xE9, 1, // sub ecx,1
    0x75, 0xF9, // jnz loop
    0xE8, 6, 0, 0, 0, // call routine
    0xA3, 0x20, 0x20, 0x01, 0,    // mov [0x12020],eax
    0xC3, // ret
    0x83, 0xC0, 7,    // routine: add eax,7
    0xC3, // ret
];

/// Falha recuperável de análise, carregamento ou execução AOT.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Format(#[from] xbox_core::Error),
    #[error(transparent)]
    Memory(#[from] xbox_mem::Falta),
    #[error("recompilação em 0x{pc:08X}: {detail}")]
    Analysis { pc: u32, detail: String },
    #[error("endereço 0x{0:08X} não tem bloco recompilado")]
    UnknownBlock(u32),
    #[error(
        "shift duplo em 0x{pc:08X}: contagem {count} maior que operando de {bits} bits tem resultado indefinido"
    )]
    UndefinedDoubleShift { pc: u32, count: u32, bits: u32 },
    #[error("#DE em 0x{pc:08X}: {reason}")]
    Divide { pc: u32, reason: DivideFault },
}

/// Resultado do subsistema AOT.
pub type Result<T> = std::result::Result<T, Error>;
