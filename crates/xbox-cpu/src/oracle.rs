//! Referência independente na CPU host para o subconjunto inteiro comum a IA-32 e
//! x86_64. Referências: Intel SDM vol. 2 e Rust Reference / inline assembly.
//! Não serve como oráculo de x87, modos de CPU, segmentação ou temporização Xbox.

use core::arch::asm;

mod double;
pub use double::{DoubleShift, DoubleWidth, execute_double_shift};
pub mod exchange;
pub mod muldiv;
pub mod strings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alu {
    Add,
    Adc,
    Sub,
    Sbb,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shift {
    Shl,
    Shr,
    Sar,
    Rol,
    Ror,
    Rcl,
    Rcr,
}

/// Executa um shift fixo por CL, com flags aritméticas de entrada controladas.
/// Bits não aritméticos de RFLAGS do host são preservados na inicialização.
/// Flags indefinidas na ISA não devem ser comparadas pelo chamador (ADR-0015).
pub fn execute_shift(op: Shift, width: Width, a: u32, count: u32, old: u32) -> (u32, u32) {
    let mut value = a;
    let flags: u64;
    let status = u64::from(old & 0x8D5);
    macro_rules! instruction {
        ($mnemonic:literal) => {
            match width {
                Width::Byte => {
                    // SAFETY: somente registradores e pilha balanceada. POPFQ
                    // restaura os bits não aritméticos capturados no mesmo asm;
                    // só CF/PF/AF/ZF/SF/OF recebem dados mascarados. DF, TF, IF,
                    // IOPL e AC não são alterados. Não há ponteiros guest.
                    unsafe { asm!(
                        "pushfq", "pop {initial}",
                        "and {initial}, -2262", // !0x8D5
                        "or {initial}, {status}", "push {initial}", "popfq",
                        concat!($mnemonic, " {value:l}, cl"), "pushfq", "pop {flags}",
                        initial = out(reg) _, status = in(reg) status,
                        value = inout(reg) value, in("ecx") count,
                        flags = lateout(reg) flags,
                    ); }
                }
                Width::Word => {
                    // SAFETY: mesmas invariantes de Byte; apenas a largura do
                    // shift muda. POPFQ nunca recebe flags de controle do caller.
                    unsafe { asm!(
                        "pushfq", "pop {initial}",
                        "and {initial}, -2262",
                        "or {initial}, {status}", "push {initial}", "popfq",
                        concat!($mnemonic, " {value:x}, cl"), "pushfq", "pop {flags}",
                        initial = out(reg) _, status = in(reg) status,
                        value = inout(reg) value, in("ecx") count,
                        flags = lateout(reg) flags,
                    ); }
                }
                Width::Dword => {
                    // SAFETY: mesmas invariantes de Byte/Word. O estado de
                    // controle do host é preservado; toda pilha é balanceada.
                    unsafe { asm!(
                        "pushfq", "pop {initial}",
                        "and {initial}, -2262",
                        "or {initial}, {status}", "push {initial}", "popfq",
                        concat!($mnemonic, " {value:e}, cl"), "pushfq", "pop {flags}",
                        initial = out(reg) _, status = in(reg) status,
                        value = inout(reg) value, in("ecx") count,
                        flags = lateout(reg) flags,
                    ); }
                }
            }
        };
    }
    match op {
        Shift::Shl => instruction!("shl"),
        Shift::Shr => instruction!("shr"),
        Shift::Sar => instruction!("sar"),
        Shift::Rol => instruction!("rol"),
        Shift::Ror => instruction!("ror"),
        Shift::Rcl => instruction!("rcl"),
        Shift::Rcr => instruction!("rcr"),
    }
    (value, flags as u32)
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
        Alu::Adc => instruction!("adc", ", {rhs:l}", ", {rhs:x}", ", {rhs:e}"),
        Alu::Sub => instruction!("sub", ", {rhs:l}", ", {rhs:x}", ", {rhs:e}"),
        Alu::Sbb => instruction!("sbb", ", {rhs:l}", ", {rhs:x}", ", {rhs:e}"),
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
    use super::{Alu, Shift, Width, execute, execute_shift};

    #[test]
    fn shifts_inicializam_flags_e_mascaram_contagem_sem_perder_sinal() {
        for old in [0, 1, 0x10, 0x800, 0x8D5] {
            for op in [Shift::Shl, Shift::Shr, Shift::Sar] {
                let (value, flags) = execute_shift(op, Width::Dword, 0x8000_0001, 32, old);
                assert_eq!(value, 0x8000_0001);
                assert_eq!(flags & 0x8D5, old);
            }
        }
        let (value, flags) = execute_shift(Shift::Sar, Width::Word, 0x1234_8000, 16, 0);
        assert_eq!(value, 0x1234_FFFF);
        assert_eq!(flags & 0xC5, 0x85);
    }

    #[test]
    fn registra_carry_overflow_e_preserva_bits_superiores() {
        let (value, flags) = execute(Alu::Add, Width::Byte, 0x1234_567F, 1, false);
        assert_eq!(value, 0x1234_5680);
        assert_eq!(flags & 0x8D5, 0x890);
        let (value, flags) = execute(Alu::Inc, Width::Word, 0x1234_FFFF, 0, true);
        assert_eq!(value, 0x1234_0000);
        assert_eq!(flags & 0x8D5, 0x55);
    }

    #[test]
    fn rotacao_com_carry_inclui_o_nono_bit() {
        let (value, flags) = execute_shift(Shift::Rcl, Width::Byte, 0x1234_5680, 1, 1);
        assert_eq!(value, 0x1234_5601);
        assert_eq!(flags & 0x801, 0x801);
        let (value, flags) = execute_shift(Shift::Rcr, Width::Byte, 0x1234_5601, 1, 1);
        assert_eq!(value, 0x1234_5680);
        assert_eq!(flags & 0x801, 0x801);
    }
}
