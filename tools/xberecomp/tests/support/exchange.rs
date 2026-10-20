use super::image;
use std::{fmt::Write, path::Path};

pub(super) fn sources(output: &Path) -> String {
    let mut modules = String::new();
    let mut memory = String::new();
    let mut registers = String::new();
    let mut conditions = String::new();
    declare(
        output,
        "cx_sib",
        &[0xF0, 0x0F, 0xB1, 0x54, 0x88, 4, 0xC3],
        &mut modules,
    );
    declare(
        output,
        "cx_fs",
        &[0x64, 0xF0, 0x0F, 0xB1, 0x16, 0xC3],
        &mut modules,
    );
    for (bits, prefix, opcode) in [
        (8, vec![], 0xB0),
        (16, vec![0x66], 0xB1),
        (32, vec![], 0xB1),
    ] {
        for lock in [false, true] {
            let name = format!("cx_mem_{bits}_{}", u8::from(lock));
            let mut code = prefix.clone();
            if lock {
                code.push(0xF0);
            }
            code.extend_from_slice(&[0x0F, opcode, 0x16, 0xC3]); // [esi],dl/dx/edx
            declare(output, &name, &code, &mut modules);
            writeln!(
                memory,
                "({bits}, {lock}, {name}::run as Run, include_bytes!(\"{name}.xbe\")),"
            )
            .unwrap();
        }
        for (dst, src) in [(0, 2), (2, 0), (1, 2), (4, 0)] {
            let name = format!("cx_reg_{bits}_{dst}_{src}");
            let mut code = prefix.clone();
            code.extend_from_slice(&[0x0F, opcode, 0xC0 | src << 3 | dst, 0xC3]);
            declare(output, &name, &code, &mut modules);
            writeln!(
                registers,
                "({bits}, {dst}, {src}, {name}::run as Run, include_bytes!(\"{name}.xbe\")),"
            )
            .unwrap();
        }
    }
    for cc in 0..16 {
        for (mode, modrm) in [(0, 0xC0), (1, 0xC4), (2, 0x06)] {
            // AL, AH, [ESI]
            let name = format!("set_{cc}_{mode}");
            declare(output, &name, &[0x0F, 0x90 + cc, modrm, 0xC3], &mut modules);
            writeln!(
                conditions,
                "({cc}, {mode}, {name}::run as Run, include_bytes!(\"{name}.xbe\")),"
            )
            .unwrap();
        }
    }
    include_str!("../verification_exchange.inc")
        .replace("/* MODULES */", &modules)
        .replace("/* MEMORY */", &memory)
        .replace("/* REGISTERS */", &registers)
        .replace("/* CONDITIONS */", &conditions)
}

fn declare(output: &Path, name: &str, code: &[u8], modules: &mut String) {
    image(output, name, code);
    writeln!(modules, "mod {name} {{ include!(\"{name}.rs\"); }}").unwrap();
}
