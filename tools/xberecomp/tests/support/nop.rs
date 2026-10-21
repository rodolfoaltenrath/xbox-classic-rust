use std::{fmt::Write, path::Path};

pub(super) fn sources(output: &Path) -> String {
    let mut shapes = vec![0x90, 0x66, 0x90];
    for code in [
        &[0x0F, 0x1F, 0][..],
        &[0x0F, 0x1F, 0x40, 0],
        &[0x0F, 0x1F, 0x44, 0, 0],
        &[0x66, 0x0F, 0x1F, 0x44, 0, 0],
        &[0x0F, 0x1F, 0x80, 0, 0, 0, 0],
        &[0x0F, 0x1F, 0x84, 0, 0, 0, 0, 0],
        &[0x66, 0x0F, 0x1F, 0x84, 0, 0, 0, 0, 0],
    ] {
        shapes.extend(code);
    }
    shapes.extend([0xEB, 0, 0xC3]);
    let mut modules = String::new();
    for (name, code) in [
        ("nop_shapes", shapes.as_slice()),
        (
            "nop_addresses",
            &[
                0x64, 0x0F, 0x1F, 0, 0x67, 0x0F, 0x1F, 0, 0x0F, 0x1F, 0xC1, 0xEB, 0, 0xC3,
            ][..],
        ),
    ] {
        super::image(output, name, code);
        writeln!(modules, "mod {name} {{ include!(\"{name}.rs\"); }}").unwrap();
    }
    include_str!("../verification_nop.inc").replace("/* MODULES */", &modules)
}
