use core::arch::asm;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoubleShift {
    Shld,
    Shrd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoubleWidth {
    Word,
    Dword,
}

/// Referência física com instruções fixas e flags aritméticas de entrada
/// controladas. O chamador exclui contagens/resultados indefinidos (ADR-0016).
pub fn execute_double_shift(
    op: DoubleShift,
    width: DoubleWidth,
    a: u32,
    b: u32,
    count: u32,
    old: u32,
) -> (u32, u32) {
    let mut value = a;
    let flags: u64;
    let status = u64::from(old & 0x8D5);
    macro_rules! instruction {
        ($mnemonic:literal) => {
            match width {
                DoubleWidth::Word => {
                    // SAFETY: somente registradores e pilha balanceada. O valor
                    // de POPFQ preserva os bits de controle capturados no host;
                    // dados de entrada só afetam CF/PF/AF/ZF/SF/OF. Scratch é
                    // saída antecipada, CL é entrada explícita, e não há ponteiros.
                    unsafe { asm!(
                        "pushfq", "pop {initial}", "and {initial}, -2262",
                        "or {initial}, {status}", "push {initial}", "popfq",
                        concat!($mnemonic, " {value:x}, {source:x}, cl"),
                        "pushfq", "pop {flags}",
                        initial = out(reg) _, status = in(reg) status,
                        value = inout(reg) value, source = in(reg) b,
                        in("ecx") count, flags = lateout(reg) flags,
                    ); }
                }
                DoubleWidth::Dword => {
                    // SAFETY: mesmas invariantes de Word. DF/TF/IF/IOPL/AC não
                    // recebem dados do chamador; nenhum endereço guest é executado.
                    unsafe { asm!(
                        "pushfq", "pop {initial}", "and {initial}, -2262",
                        "or {initial}, {status}", "push {initial}", "popfq",
                        concat!($mnemonic, " {value:e}, {source:e}, cl"),
                        "pushfq", "pop {flags}",
                        initial = out(reg) _, status = in(reg) status,
                        value = inout(reg) value, source = in(reg) b,
                        in("ecx") count, flags = lateout(reg) flags,
                    ); }
                }
            }
        };
    }
    match op {
        DoubleShift::Shld => instruction!("shld"),
        DoubleShift::Shrd => instruction!("shrd"),
    }
    (value, flags as u32)
}

#[cfg(test)]
mod tests {
    use super::{DoubleShift, DoubleWidth, execute_double_shift};

    #[test]
    fn palavra_inteira_transfere_fonte_e_contagem_mascarada_zero_preserva_flags() {
        for op in [DoubleShift::Shld, DoubleShift::Shrd] {
            let (value, _) =
                execute_double_shift(op, DoubleWidth::Word, 0xA55A_1234, 0x9876, 16, 0);
            assert_eq!(value, 0xA55A_9876);
            let (value, flags) =
                execute_double_shift(op, DoubleWidth::Dword, 0x8123_4567, 0, 32, 0x8D5);
            assert_eq!(value, 0x8123_4567);
            assert_eq!(flags & 0x8D5, 0x8D5);
        }
    }
}
