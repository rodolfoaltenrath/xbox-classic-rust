use std::{fmt::Write, path::Path};
use xbox_formats::fixtures::{xbe_com_codigo, xbe_com_imports};
use xbox_recomp::{analysis, emit};

pub(super) fn sources(output: &Path) -> String {
    let mut modules = String::new();
    for (name, code) in [
        (
            "tsc_pair",
            &[
                0x0F, 0x31, 0x89, 0xC1, 0x89, 0xD3, 0xEB, 0, 0x0F, 0x31, 0xC3,
            ][..],
        ),
        ("tsc_rep", &[0xF3, 0xA4, 0x0F, 0x31, 0xC3][..]),
        ("tsc_fault", &[0xA3, 0x20, 0x20, 1, 0, 0x0F, 0x31, 0xC3][..]),
    ] {
        write(output, name, &xbe_com_codigo(code).unwrap(), &mut modules);
    }
    let kernel = xbe_com_imports(
        &[
            0x6A, 0, 0x68, 0x20, 0x20, 1, 0, 0xFF, 0x15, 0, 0x20, 1, 0, 0x0F, 0x31, 0xC3,
        ],
        &[289],
    )
    .unwrap();
    write(output, "tsc_kernel", &kernel, &mut modules);
    include_str!("../verification_timing.inc").replace("/* MODULES */", &modules)
}

fn write(output: &Path, name: &str, image: &[u8], modules: &mut String) {
    let program = analysis::analyze(image).unwrap();
    std::fs::write(
        output.join(format!("src/{name}.rs")),
        emit::rust_source(&program),
    )
    .unwrap();
    std::fs::write(output.join(format!("src/{name}.xbe")), image).unwrap();
    writeln!(modules, "mod {name} {{ include!(\"{name}.rs\"); }}").unwrap();
}
