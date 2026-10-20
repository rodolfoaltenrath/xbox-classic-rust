//! Referência física de MUL/IMUL e DIV/IDIV. A pré-checagem usa DIV/IDIV de
//! 64 bits para provar que uma divisão estreita não entregará #DE ao host.

use core::arch::asm;

use super::Width;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DivideError {
    ZeroDivisor,
    QuotientOverflow,
}

/// Devolve EAX, EDX e flags após o produto implícito físico.
pub fn multiply(
    width: Width,
    signed: bool,
    mut low: u32,
    mut high: u32,
    source: u32,
) -> (u32, u32, u32) {
    let flags: u64;
    macro_rules! instruction {
        ($op:literal) => {
            match width {
                Width::Byte => {
                    // SAFETY: multiplicação fixa sobre AL/CL, sem ponteiros ou
                    // possibilidade de #DE. EAX/EDX são declarados como saídas;
                    // PUSHFQ/POP é balanceado e não altera flags de controle.
                    unsafe { asm!(concat!($op, " cl"), "pushfq", "pop {flags}",
                        inout("eax") low, inout("edx") high, in("ecx") source,
                        flags = lateout(reg) flags); }
                }
                Width::Word => {
                    // SAFETY: mesmas invariantes de Byte, agora DX:AX = AX * CX.
                    unsafe { asm!(concat!($op, " cx"), "pushfq", "pop {flags}",
                        inout("eax") low, inout("edx") high, in("ecx") source,
                        flags = lateout(reg) flags); }
                }
                Width::Dword => {
                    // SAFETY: mesmas invariantes; produto cabe em EDX:EAX.
                    unsafe { asm!(concat!($op, " ecx"), "pushfq", "pop {flags}",
                        inout("eax") low, inout("edx") high, in("ecx") source,
                        flags = lateout(reg) flags); }
                }
            }
        };
    }
    if signed {
        instruction!("imul");
    } else {
        instruction!("mul");
    }
    (low, high, flags as u32)
}

/// Erros são classificados pela faixa do quociente físico de 64 bits. Só após
/// essa prova a instrução física exata de 8/16/32 bits é executada. Não injeta #DE
/// nativo: zero e MIN64/-1 são recusados antes do assembly (ADR-0017).
pub fn divide(
    width: Width,
    signed: bool,
    mut low: u32,
    mut high: u32,
    source: u32,
) -> Result<(u32, u32), DivideError> {
    check_division(width, signed, low, high, source)?;
    macro_rules! instruction {
        ($op:literal) => {
            match width {
                Width::Byte => {
                    // SAFETY: check_division provou divisor não zero e quociente
                    // representável, com os mesmos AX e CL. Só registradores;
                    // EAX/EDX e flags aritméticas estão declarados clobbered.
                    unsafe { asm!(concat!($op, " cl"), inout("eax") low,
                        inout("edx") high, in("ecx") source); }
                }
                Width::Word => {
                    // SAFETY: mesma prova para DX:AX / CX. Não há memória guest,
                    // nem possibilidade de overflow depois da pré-checagem.
                    unsafe { asm!(concat!($op, " cx"), inout("eax") low,
                        inout("edx") high, in("ecx") source); }
                }
                Width::Dword => {
                    // SAFETY: mesma prova para EDX:EAX / ECX. #DE nunca é usado
                    // como mecanismo de controle no processo de teste.
                    unsafe { asm!(concat!($op, " ecx"), inout("eax") low,
                        inout("edx") high, in("ecx") source); }
                }
            }
        };
    }
    if signed {
        instruction!("idiv");
    } else {
        instruction!("div");
    }
    Ok((low, high))
}

fn check_division(
    width: Width,
    signed: bool,
    low: u32,
    high: u32,
    source: u32,
) -> Result<(), DivideError> {
    let raw = match width {
        Width::Byte => u64::from(low as u16),
        Width::Word => (u64::from(high as u16) << 16) | u64::from(low as u16),
        Width::Dword => (u64::from(high) << 32) | u64::from(low),
    };
    let divisor = match width {
        Width::Byte => source & 0xFF,
        Width::Word => source & 0xFFFF,
        Width::Dword => source,
    };
    if divisor == 0 {
        return Err(DivideError::ZeroDivisor);
    }
    let fits = if signed {
        let dividend = match width {
            Width::Byte => i64::from((raw as u16).cast_signed()),
            Width::Word => i64::from((raw as u32).cast_signed()),
            Width::Dword => raw.cast_signed(),
        };
        let divisor = match width {
            Width::Byte => i64::from((divisor as u8).cast_signed()),
            Width::Word => i64::from((divisor as u16).cast_signed()),
            Width::Dword => i64::from(divisor.cast_signed()),
        };
        if dividend == i64::MIN && divisor == -1 {
            return Err(DivideError::QuotientOverflow);
        }
        let mut quotient = dividend;
        // SAFETY: divisor não zero e MIN64/-1 excluído. CQO prepara RDX:RAX;
        // RCX é independente das saídas fixas, não há memória nem pilha guest.
        unsafe {
            asm!("cqo", "idiv rcx", inout("rax") quotient,
            lateout("rdx") _, in("rcx") divisor);
        }
        match width {
            Width::Byte => i8::try_from(quotient).is_ok(),
            Width::Word => i16::try_from(quotient).is_ok(),
            Width::Dword => i32::try_from(quotient).is_ok(),
        }
    } else {
        let mut quotient = raw;
        // SAFETY: RDX=0 e divisor não zero garantem quociente de até 64 bits.
        // Somente RAX/RDX/RCX e flags aritméticas são alterados.
        unsafe {
            asm!("xor edx, edx", "div rcx", inout("rax") quotient,
            lateout("rdx") _, in("rcx") u64::from(divisor));
        }
        match width {
            Width::Byte => u8::try_from(quotient).is_ok(),
            Width::Word => u16::try_from(quotient).is_ok(),
            Width::Dword => u32::try_from(quotient).is_ok(),
        }
    };
    if fits {
        Ok(())
    } else {
        Err(DivideError::QuotientOverflow)
    }
}

#[cfg(test)]
mod tests {
    use super::{DivideError, Width, divide, multiply};

    #[test]
    fn produto_divisao_e_guardas_nao_alteram_partes_preservadas() {
        let (low, high, flags) = multiply(Width::Byte, true, 0xA55A_0080, 0x1234_5678, 2);
        assert_eq!(
            (low, high, flags & 0x801),
            (0xA55A_FF00, 0x1234_5678, 0x801)
        );
        assert_eq!(
            divide(Width::Byte, true, 0xA55A_FFF9, 0x1234_5678, 3),
            Ok((0xA55A_FFFE, 0x1234_5678))
        );
        assert_eq!(
            divide(Width::Dword, true, 0, 0x8000_0000, u32::MAX),
            Err(DivideError::QuotientOverflow)
        );
        assert_eq!(
            divide(Width::Byte, false, 1, 0, 256),
            Err(DivideError::ZeroDivisor)
        );
    }
}
