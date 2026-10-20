use std::{fmt::Write, path::Path};

pub(super) fn sources(output: &Path) -> String {
    let mut modules = String::new();
    let mut registers = String::new();
    let mut memory = String::new();
    let mut aliases = String::new();
    let mut addresses = String::new();
    for (name, opcode, reverse) in [("bsf", 0xBC, false), ("bsr", 0xBD, true)] {
        for bits in [16, 32] {
            let prefix = if bits == 16 { vec![0x66] } else { vec![] };
            for (mode, modrm, cases) in [("reg", 0xC1, &mut registers), ("mem", 0x06, &mut memory)]
            {
                let module = format!("{name}_{bits}_{mode}");
                let mut code = prefix.clone();
                code.extend_from_slice(&[0x0F, opcode, modrm, 0xC3]);
                declare(output, &module, &code, &mut modules);
                writeln!(
                    cases,
                    "({reverse}, {bits}, {module}::run as Run, include_bytes!(\"{module}.xbe\")),"
                )
                .unwrap();
            }
            for (mode, modrm, end) in [("self", 0xC0, 0xC3), ("sp", 0xE0, 0xCC)] {
                let module = format!("{name}_{bits}_{mode}");
                let mut code = prefix.clone();
                code.extend_from_slice(&[0x0F, opcode, modrm, end]);
                declare(output, &module, &code, &mut modules);
            }
            writeln!(aliases,"({reverse}, {bits}, {name}_{bits}_self::run as Run, {name}_{bits}_sp::run as Run, include_bytes!(\"{name}_{bits}_self.xbe\").as_slice(), include_bytes!(\"{name}_{bits}_sp.xbe\").as_slice()),").unwrap();
            let module = format!("{name}_{bits}_fs");
            let mut code = vec![0x64];
            code.extend_from_slice(&prefix);
            code.extend_from_slice(&[0x0F, opcode, 0x44, 0x88, 4, 0xC3]);
            declare(output, &module, &code, &mut modules);
            writeln!(
                addresses,
                "({reverse}, {bits}, {module}::run as Run, include_bytes!(\"{module}.xbe\")),"
            )
            .unwrap();
        }
    }
    declare(
        output,
        "scan_branch",
        &[
            0x0F, 0xBD, 0xC1, 0xEB, 0, 0x74, 6, 0xBB, 1, 0, 0, 0, 0xC3, 0xBB, 2, 0, 0, 0, 0xC3,
        ],
        &mut modules,
    );
    include_str!("../verification_scan.inc")
        .replace("/* MODULES */", &modules)
        .replace("/* REGISTERS */", &registers)
        .replace("/* MEMORY */", &memory)
        .replace("/* ALIASES */", &aliases)
        .replace("/* ADDRESSES */", &addresses)
}

fn declare(output: &Path, name: &str, code: &[u8], modules: &mut String) {
    super::image(output, name, code);
    writeln!(modules, "mod {name} {{ include!(\"{name}.rs\"); }}").unwrap();
}
