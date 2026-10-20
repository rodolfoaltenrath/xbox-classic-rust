use std::{fmt::Write, path::Path};

pub(super) fn sources(output: &Path) -> String {
    let mut modules = String::new();
    let mut registers = String::new();
    let mut memory = String::new();
    for cc in 0..16 {
        for bits in [16, 32] {
            for (mode, modrm, cases) in [("reg", 0xC1, &mut registers), ("mem", 0x06, &mut memory)]
            {
                let name = format!("cmov_{cc}_{bits}_{mode}");
                let mut code = if bits == 16 { vec![0x66] } else { vec![] };
                code.extend_from_slice(&[0x0F, 0x40 + cc, modrm, 0xC3]);
                declare(output, &name, &code, &mut modules);
                writeln!(
                    cases,
                    "({cc}, {bits}, {name}::run as Run, include_bytes!(\"{name}.xbe\")),"
                )
                .unwrap();
            }
        }
    }
    for (name, code) in [
        ("cmov_self16", &[0x66, 0x0F, 0x44, 0xC0, 0xC3][..]),
        ("cmov_self32", &[0x0F, 0x44, 0xC0, 0xC3][..]),
        ("cmov_sp16", &[0x66, 0x0F, 0x44, 0xE0, 0xC3][..]),
        ("cmov_sp32", &[0x0F, 0x44, 0xE0, 0xC3][..]),
        ("cmov_fs_sib", &[0x64, 0x0F, 0x44, 0x44, 0x88, 4, 0xC3][..]),
        (
            "cmov_snapshot",
            &[0x39, 0xD8, 0xEB, 0, 0x0F, 0x44, 0xC1, 0xC3][..],
        ),
    ] {
        declare(output, name, code, &mut modules);
    }
    include_str!("../verification_conditional.inc")
        .replace("/* MODULES */", &modules)
        .replace("/* REGISTERS */", &registers)
        .replace("/* MEMORY */", &memory)
}

fn declare(output: &Path, name: &str, code: &[u8], modules: &mut String) {
    super::image(output, name, code);
    writeln!(modules, "mod {name} {{ include!(\"{name}.rs\"); }}").unwrap();
}
