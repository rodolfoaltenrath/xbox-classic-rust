use std::{fmt::Write, path::Path};

pub(super) fn sources(output: &Path) -> String {
    let mut modules = String::new();
    for (name, code) in [
        ("bp_direct", &[0xCC, 0xA3, 0x20, 0x20, 1, 0, 0xC3][..]),
        ("bp_branch", &[0x74, 1, 0xCC, 0xC3][..]),
        ("bp_store", &[0xA3, 0x20, 0x20, 1, 0, 0xCC][..]),
    ] {
        super::image(output, name, code);
        writeln!(modules, "mod {name} {{ include!(\"{name}.rs\"); }}").unwrap();
    }
    include_str!("../verification_breakpoint.inc").replace("/* MODULES */", &modules)
}
