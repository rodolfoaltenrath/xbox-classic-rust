//! Referência física limitada a buffers do host, nunca a ponteiros guest.

use super::Width;
use core::arch::asm;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StringOp {
    Movs,
    Stos,
    Lods,
    Cmps,
    Scas,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Repeat {
    None,
    Always,
    Equal,
    NotEqual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outcome {
    pub eax: u32,
    pub flags: u32,
    pub count: u32,
    pub source_delta: i64,
    pub destination_delta: i64,
}

/// Executa somente strings fixas sobre slices com faixa integral validada.
/// O endereço host é de 64 bits; address16 do guest exige testes separados.
#[allow(clippy::too_many_arguments)]
pub fn execute(
    op: StringOp,
    width: Width,
    repeat: Repeat,
    source: &[u8],
    destination: &mut [u8],
    count: u32,
    eax: u32,
    old: u32,
) -> Outcome {
    let bytes = match width {
        Width::Byte => 1,
        Width::Word => 2,
        Width::Dword => 4,
    };
    let elements = if repeat == Repeat::None {
        1
    } else {
        count as usize
    };
    let size = elements.checked_mul(bytes).expect("faixa do oráculo");
    assert!(source.len() >= size && destination.len() >= size);
    let start = if old & 0x400 != 0 {
        size.saturating_sub(bytes)
    } else {
        0
    };
    let initial_src = source.as_ptr().wrapping_add(start) as usize;
    let initial_dst = destination.as_mut_ptr().wrapping_add(start) as usize;
    let mut src = initial_src;
    let mut dst = initial_dst;
    let mut remaining = count as usize;
    let mut value = eax;
    let flags: u64;
    let status = u64::from(old & 0xCD5);
    macro_rules! instruction {
        ($text:expr) => {
            // SAFETY: faixa de todos os elementos validada nos dois slices antes
            // de entrar no asm. RSI só lê source; RDI só lê/escreve destination,
            // cuja referência é exclusiva. Strings/operandos são fixos. Nenhum
            // endereço guest é usado. RCX, RSI, RDI e EAX são inout explícitos.
            // POPFQ só altera status e DF; stack balanceada. DF é SEMPRE limpo
            // antes de sair, inclusive em zero iterações. Sem chamadas ou faults
            // deliberados enquanto DF=1. Não usar nomem/readonly/preserves_flags.
            unsafe { asm!(
                "pushfq", "pop {initial}", "and {initial}, -3286",
                "or {initial}, {status}", "push {initial}", "popfq",
                $text, "pushfq", "pop {flags}", "cld",
                initial = out(reg) _, status = in(reg) status,
                inout("rsi") src, inout("rdi") dst, inout("rcx") remaining,
                inout("eax") value, flags = lateout(reg) flags,
            ); }
        };
    }
    macro_rules! sized {
        ($prefix:literal, $byte:literal, $word:literal, $dword:literal) => {
            match width {
                Width::Byte => instruction!(concat!($prefix, $byte)),
                Width::Word => instruction!(concat!($prefix, $word)),
                Width::Dword => instruction!(concat!($prefix, $dword)),
            }
        };
    }
    match (op, repeat) {
        (StringOp::Movs, Repeat::None) => sized!("", "movsb", "movsw", "movsd"),
        (StringOp::Movs, Repeat::Always) => sized!("rep ", "movsb", "movsw", "movsd"),
        (StringOp::Stos, Repeat::None) => sized!("", "stosb", "stosw", "stosd"),
        (StringOp::Stos, Repeat::Always) => sized!("rep ", "stosb", "stosw", "stosd"),
        (StringOp::Lods, Repeat::None) => sized!("", "lodsb", "lodsw", "lodsd"),
        (StringOp::Lods, Repeat::Always) => sized!("rep ", "lodsb", "lodsw", "lodsd"),
        (StringOp::Cmps, Repeat::None) => sized!("", "cmpsb", "cmpsw", "cmpsd"),
        (StringOp::Cmps, Repeat::Equal) => sized!("repe ", "cmpsb", "cmpsw", "cmpsd"),
        (StringOp::Cmps, Repeat::NotEqual) => sized!("repne ", "cmpsb", "cmpsw", "cmpsd"),
        (StringOp::Scas, Repeat::None) => sized!("", "scasb", "scasw", "scasd"),
        (StringOp::Scas, Repeat::Equal) => sized!("repe ", "scasb", "scasw", "scasd"),
        (StringOp::Scas, Repeat::NotEqual) => sized!("repne ", "scasb", "scasw", "scasd"),
        _ => panic!("prefixo/operação fora do subconjunto documentado"),
    }
    Outcome {
        eax: value,
        flags: flags as u32,
        count: remaining as u32,
        source_delta: i64::try_from(src as i128 - initial_src as i128).unwrap(),
        destination_delta: i64::try_from(dst as i128 - initial_dst as i128).unwrap(),
    }
}
