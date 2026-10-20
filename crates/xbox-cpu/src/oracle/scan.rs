//! Oráculo físico de BSF/BSR, comparando apenas resultados definidos pela ISA.
use super::Width;
use core::arch::asm;

/// Se a origem for zero, comparar somente ZF: o destino é indefinido na
/// referência clássica. CF/PF/AF/SF/OF nunca são referências neste oráculo.
pub fn bit_scan(reverse: bool, width: Width, mut destination: u32, source: u32) -> (u32, u32) {
    let flags: u64;
    macro_rules! execute {
        ($mnemonic:literal, $size:literal) => {{
            // SAFETY: somente registradores locais e pilha balanceada para
            // capturar RFLAGS. BSF/BSR fazem parte de x86_64 e não mudam flags
            // de controle. Não há ponteiros guest, escrita de RFLAGS ou FFI.
            unsafe { asm!(
                concat!($mnemonic, " {destination:", $size, "}, {source:", $size, "}"),
                "pushfq", "pop {flags}",
                destination = inout(reg) destination, source = in(reg) source,
                flags = lateout(reg) flags,
            ); }
        }};
    }
    match (reverse, width) {
        (false, Width::Word) => execute!("bsf", "x"),
        (false, Width::Dword) => execute!("bsf", "e"),
        (true, Width::Word) => execute!("bsr", "x"),
        (true, Width::Dword) => execute!("bsr", "e"),
        (_, Width::Byte) => panic!("BSF/BSR não têm operando de byte"),
    }
    (destination, flags as u32)
}
