//! Referência independente na CPU host para o subconjunto inteiro comum a IA-32 e
//! x86_64. Referências: Intel SDM vol. 2 e Rust Reference / inline assembly.
//! Não serve como oráculo de x87, modos de CPU, segmentação ou temporização Xbox.

use core::arch::asm;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alu {
    Add,
    Sub,
    Xor,
    And,
    Or,
    Cmp,
    Test,
    Inc,
    Dec,
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Width {
    Byte,
    Word,
    Dword,
}

/// Executa somente instruções fixas sobre registradores inicializados. CF de
/// entrada é controlado por BT; resultados e flags são capturados antes do Rust.
pub fn execute(op: Alu, width: Width, a: u32, b: u32, carry: bool) -> (u32, u32) {
    let mut value = a;
    let flags: u64;
    let carry = u32::from(carry);
    macro_rules! instruction {
        ($mnemonic:literal, $tail8:literal, $tail16:literal, $tail32:literal) => {
            match width {
                Width::Byte => {
                    // SAFETY: apenas ALU em registradores; PUSHFQ/POP usam a pilha
                    // válida do chamador e mantêm seu tamanho. Não altera DF ou
                    // acessa ponteiros. Flags são declaradas clobbered pelo asm.
                    unsafe { asm!(
                        "bt {carry:e}, 0", concat!($mnemonic, " {value:l}", $tail8),
                        "pushfq", "pop {flags}", "// rhs {rhs:e}",
                        value = inout(reg) value, rhs = in(reg) b,
                        carry = in(reg) carry, flags = lateout(reg) flags,
                    ); }
                }
                Width::Word => {
                    // SAFETY: mesmas invariantes do caso Byte; só o tamanho da
                    // operação ALU muda. Nenhum endereço guest é executado.
                    unsafe { asm!(
                        "bt {carry:e}, 0", concat!($mnemonic, " {value:x}", $tail16),
                        "pushfq", "pop {flags}", "// rhs {rhs:e}",
                        value = inout(reg) value, rhs = in(reg) b,
                        carry = in(reg) carry, flags = lateout(reg) flags,
                    ); }
                }
                Width::Dword => {
                    // SAFETY: mesmas invariantes dos casos Byte/Word. A pilha
                    // termina exatamente onde começou e não há acesso a memória
                    // além do par PUSHFQ/POP balanceado.
                    unsafe { asm!(
                        "bt {carry:e}, 0", concat!($mnemonic, " {value:e}", $tail32),
                        "pushfq", "pop {flags}", "// rhs {rhs:e}",
                        value = inout(reg) value, rhs = in(reg) b,
                        carry = in(reg) carry, flags = lateout(reg) flags,
                    ); }
                }
            }
        };
    }
    match op {
        Alu::Add => instruction!("add", ", {rhs:l}", ", {rhs:x}", ", {rhs:e}"),
        Alu::Sub => instruction!("sub", ", {rhs:l}", ", {rhs:x}", ", {rhs:e}"),
        Alu::Xor => instruction!("xor", ", {rhs:l}", ", {rhs:x}", ", {rhs:e}"),
        Alu::And => instruction!("and", ", {rhs:l}", ", {rhs:x}", ", {rhs:e}"),
        Alu::Or => instruction!("or", ", {rhs:l}", ", {rhs:x}", ", {rhs:e}"),
        Alu::Cmp => instruction!("cmp", ", {rhs:l}", ", {rhs:x}", ", {rhs:e}"),
        Alu::Test => instruction!("test", ", {rhs:l}", ", {rhs:x}", ", {rhs:e}"),
        Alu::Inc => instruction!("inc", "", "", ""),
        Alu::Dec => instruction!("dec", "", "", ""),
        Alu::Neg => instruction!("neg", "", "", ""),
        Alu::Not => instruction!("not", "", "", ""),
    }
    (value, flags as u32)
}

#[cfg(test)]
mod tests {
    use super::{Alu, Width, execute};

    #[test]
    fn registra_carry_overflow_e_preserva_bits_superiores() {
        let (value, flags) = execute(Alu::Add, Width::Byte, 0x1234_567F, 1, false);
        assert_eq!(value, 0x1234_5680);
        assert_eq!(flags & 0x8D5, 0x890);
        let (value, flags) = execute(Alu::Inc, Width::Word, 0x1234_FFFF, 0, true);
        assert_eq!(value, 0x1234_0000);
        assert_eq!(flags & 0x8D5, 0x55);
    }
}
