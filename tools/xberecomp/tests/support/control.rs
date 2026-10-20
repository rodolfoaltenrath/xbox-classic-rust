use std::{fmt::Write, path::Path};

pub(super) fn sources(output: &Path) -> String {
    let mut modules = String::new();
    for (name, code) in [
        ("control_cli", &[0xFA, 0xC3][..]),
        // O store posterior não pode executar enquanto a CPU estiver suspensa.
        ("control_hlt", &[0xF4, 0xA3, 0x20, 0x20, 1, 0, 0xC3][..]),
        ("control_assert", &[0xFA, 0xF4, 0xEB, 0xFD][..]),
    ] {
        super::image(output, name, code);
        writeln!(modules, "mod {name} {{ include!(\"{name}.rs\"); }}").unwrap();
    }
    include_str!("../verification_control.inc").replace("/* MODULES */", &modules)
}
