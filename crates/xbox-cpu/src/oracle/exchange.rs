//! Oráculo físico de CMPXCHG: somente memória local válida, nunca ponteiro guest.
use super::Width;
use core::arch::asm;

pub fn compare_exchange(
    width: Width,
    mut accumulator: u32,
    mut destination: u32,
    source: u32,
    locked: bool,
) -> (u32, u32, u32) {
    let flags: u64;
    macro_rules! execute {
        ($prefix:literal, $size:literal, $source:literal) => {{
            // SAFETY: address aponta para destination, u32 local vivo e exclusivo.
            // Cada variante acessa no máximo 4 bytes; não recebe endereço guest.
            // CMPXCHG é suportado em x86_64; pushfq/pop balanceiam a pilha e
            // flags aritméticas são capturadas sem alterar DF/IF ou executar guest.
            unsafe { asm!(
                concat!($prefix, "cmpxchg ", $size, " ptr [{address}], {source:", $source, "}"),
                "pushfq", "pop {flags}",
                address = in(reg) &mut destination,
                source = in(reg) source,
                inout("eax") accumulator,
                flags = lateout(reg) flags,
            ); }
        }};
    }
    match (width, locked) {
        (Width::Byte, false) => execute!("", "byte", "l"),
        (Width::Word, false) => execute!("", "word", "x"),
        (Width::Dword, false) => execute!("", "dword", "e"),
        (Width::Byte, true) => execute!("lock ", "byte", "l"),
        (Width::Word, true) => execute!("lock ", "word", "x"),
        (Width::Dword, true) => execute!("lock ", "dword", "e"),
    }
    (accumulator, destination, flags as u32)
}
