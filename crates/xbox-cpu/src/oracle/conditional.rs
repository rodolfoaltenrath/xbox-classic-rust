//! Referência física de `CMOVcc` em registradores locais, sem ponteiros guest.
use super::Width;
use core::arch::asm;

pub fn conditional_move(
    condition: u8,
    width: Width,
    mut destination: u32,
    source: u32,
    old: u32,
) -> (u32, u32) {
    let flags: u64;
    let status = u64::from(old & 0x8D5);
    macro_rules! execute {
        ($mnemonic:literal, $size:literal) => {{
            // SAFETY: somente registradores e pilha balanceada. CMOV é parte
            // de x86_64. POPFQ recebe os bits não aritméticos capturados neste
            // asm; apenas CF/PF/AF/ZF/SF/OF recebem entrada mascarada. DF, TF,
            // IF, IOPL e AC não são alterados. Nenhum endereço guest é acessado.
            unsafe { asm!(
                "pushfq", "pop {initial}",
                "and {initial}, -2262", "or {initial}, {status}",
                "push {initial}", "popfq",
                concat!($mnemonic, " {destination:", $size, "}, {source:", $size, "}"),
                "pushfq", "pop {flags}",
                initial = out(reg) _, status = in(reg) status,
                destination = inout(reg) destination, source = in(reg) source,
                flags = lateout(reg) flags,
            ); }
        }};
    }
    macro_rules! select {
        ($size:literal) => {
            match condition {
                0 => execute!("cmovo", $size),
                1 => execute!("cmovno", $size),
                2 => execute!("cmovb", $size),
                3 => execute!("cmovae", $size),
                4 => execute!("cmove", $size),
                5 => execute!("cmovne", $size),
                6 => execute!("cmovbe", $size),
                7 => execute!("cmova", $size),
                8 => execute!("cmovs", $size),
                9 => execute!("cmovns", $size),
                10 => execute!("cmovp", $size),
                11 => execute!("cmovnp", $size),
                12 => execute!("cmovl", $size),
                13 => execute!("cmovge", $size),
                14 => execute!("cmovle", $size),
                15 => execute!("cmovg", $size),
                _ => panic!("condição CMOVcc fora de 0..16"),
            }
        };
    }
    match width {
        Width::Word => select!("x"),
        Width::Dword => select!("e"),
        Width::Byte => panic!("CMOVcc não tem operando de byte"),
    }
    (destination, flags as u32)
}
