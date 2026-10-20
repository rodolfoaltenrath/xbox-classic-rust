use crate::DivideFault;
use crate::ir::Width;

use super::sign_extend;

/// Produto completo (até 64 bits) e CF/OF. Flags aritméticas restantes são
/// indefinidas e o emissor as normaliza para zero (ADR-0017).
pub fn multiply_values(a: u32, b: u32, width: Width, signed: bool) -> (u64, bool) {
    if signed {
        let a = i64::from(sign_extend(a, width).cast_signed());
        let b = i64::from(sign_extend(b, width).cast_signed());
        let product = a * b;
        let bound = 1i64 << (width.bits() - 1);
        (
            product.cast_unsigned(),
            product < -bound || product >= bound,
        )
    } else {
        let product = u64::from(a & width.mask()) * u64::from(b & width.mask());
        (product, product > u64::from(width.mask()))
    }
}

/// Divisão exata em i128 evita panic em MIN/-1 e permite validar o quociente
/// antes de escrever AL/AH, AX/DX ou EAX/EDX. Resto tem o sinal do dividendo.
pub fn divide_values(
    low: u32,
    high: u32,
    divisor: u32,
    width: Width,
    signed: bool,
) -> Result<(u32, u32), DivideFault> {
    let bits = width.bits();
    let divisor = divisor & width.mask();
    if divisor == 0 {
        return Err(DivideFault::ZeroDivisor);
    }
    let raw = if width == Width::Byte {
        u64::from(low & 0xFFFF)
    } else {
        (u64::from(high & width.mask()) << bits) | u64::from(low & width.mask())
    };
    let (dividend, divisor, min, max) = if signed {
        let dividend = i128::from(raw)
            - if raw >> (2 * bits - 1) != 0 {
                1i128 << (2 * bits)
            } else {
                0
            };
        let divisor = i128::from(sign_extend(divisor, width).cast_signed());
        let bound = 1i128 << (bits - 1);
        (dividend, divisor, -bound, bound - 1)
    } else {
        (
            i128::from(raw),
            i128::from(divisor),
            0,
            i128::from(width.mask()),
        )
    };
    let quotient = dividend / divisor;
    if quotient < min || quotient > max {
        return Err(DivideFault::QuotientOverflow);
    }
    let remainder = dividend % divisor;
    Ok((
        quotient.cast_unsigned() as u32 & width.mask(),
        remainder.cast_unsigned() as u32 & width.mask(),
    ))
}
