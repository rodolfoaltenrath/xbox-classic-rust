use std::{fmt::Write, path::Path};

pub(super) fn sources(output: &Path) -> String {
    let mut modules = String::new();
    for (name, code) in [
        ("sfence_direct", &[0x0F, 0xAE, 0xF8, 0xC3][..]),
        (
            "sfence_stores",
            &[
                0xA3, 0x20, 0x20, 1, 0, // mov [0x12020],eax
                0x0F, 0xAE, 0xF8, 0xEB, 0, // sfence; jmp next
                0x89, 0x0D, 0x24, 0x20, 1, 0, // mov [0x12024],ecx
                0x0F, 0xAE, 0xF8, 0xC3, // sfence; ret
            ][..],
        ),
    ] {
        super::image(output, name, code);
        writeln!(modules, "mod {name} {{ include!(\"{name}.rs\"); }}").unwrap();
    }
    include_str!("../verification_ordering.inc").replace("/* MODULES */", &modules)
}
