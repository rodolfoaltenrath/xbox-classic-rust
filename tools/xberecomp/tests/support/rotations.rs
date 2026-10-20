use std::fmt::Write;
use std::path::Path;

use super::image;

pub(super) fn sources(output: &Path) -> String {
    let mut modules = String::new();
    let mut cases = String::new();
    let mut memory = String::new();
    let mut faults = String::new();
    for (op, group) in [("Rol", 0), ("Ror", 1), ("Rcl", 2), ("Rcr", 3)] {
        registers(output, op, group, &mut modules, &mut cases);
        for (width, bits) in [("Byte", 8), ("Word", 16), ("Dword", 32)] {
            let name = format!("rotate_mem_{op}_{bits}").to_lowercase();
            let mut code = if bits == 16 { vec![0x66] } else { vec![] };
            code.extend_from_slice(&[
                0xD2 + u8::from(bits != 8),
                0x05 | group << 3,
                0x20,
                0x20,
                1,
                0,
                0xC3,
            ]);
            declare(output, &name, &code, &mut modules);
            case(&mut memory, op, width, None, &name);
        }
        for (suffix, address) in [("readonly", 0x11000u32), ("unmapped", 0x5000_0000)] {
            let name = format!("rotate_{op}_{suffix}").to_lowercase();
            let mut code = vec![0xD3, 0x05 | group << 3];
            code.extend_from_slice(&address.to_le_bytes());
            code.push(0xC3);
            declare(output, &name, &code, &mut modules);
            writeln!(
                faults,
                "({name}::run as Run, include_bytes!(\"{name}.xbe\") as &[u8]),"
            )
            .unwrap();
        }
    }
    for (name, code) in [
        ("rotate_ah", &[0xD2, 0xD4, 0xC3][..]),       // rcl ah,cl
        ("rotate_cl", &[0xD2, 0xC9, 0xC3][..]),       // ror cl,cl
        ("rotate_cx", &[0x66, 0xD3, 0xD9, 0xC3][..]), // rcr cx,cl
        ("rotate_ecx", &[0xD3, 0xC1, 0xC3][..]),      // rol ecx,cl
        (
            "rotate_snapshot",
            &[0xD1, 0xD0, 0xEB, 0, 0xD1, 0xDA, 0x0F, 0xA4, 0xD0, 1, 0xC3][..],
        ),
    ] {
        declare(output, name, code, &mut modules);
    }
    include_str!("../verification_rotate.inc")
        .replace("/* MODULES */", &modules)
        .replace("/* CASES */", &cases)
        .replace("/* MEMORY_CASES */", &memory)
        .replace("/* FAULT_CASES */", &faults)
}

fn registers(output: &Path, op: &str, group: u8, modules: &mut String, cases: &mut String) {
    for (width, bits) in [("Byte", 8), ("Word", 16), ("Dword", 32)] {
        for mode in 0..12 {
            let name = format!("rotate_{op}_{bits}_{mode}").to_lowercase();
            let count = match mode {
                0 => None,
                1 => Some(1),
                _ => Some([0, 1, 8, 9, 16, 17, 31, 32, 33, 255][mode - 2]),
            };
            let opcode = match mode {
                0 => 0xD2,
                1 => 0xD0,
                _ => 0xC0,
            };
            let mut code = if bits == 16 { vec![0x66] } else { vec![] };
            code.extend_from_slice(&[opcode + u8::from(bits != 8), 0xC0 | group << 3]);
            if mode >= 2 {
                code.push(count.unwrap());
            }
            code.push(0xC3);
            modules.push_str("#[cfg(target_arch = \"x86_64\")]\n");
            declare(output, &name, &code, modules);
            case(cases, op, width, count, &name);
        }
    }
}

fn case(out: &mut String, op: &str, width: &str, count: Option<u8>, name: &str) {
    writeln!(out, "Case {{ op: Rotate::{op}, width: Width::{width}, count: {count:?}, run: {name}::run, image: include_bytes!(\"{name}.xbe\") }},").unwrap();
}

fn declare(output: &Path, name: &str, code: &[u8], modules: &mut String) {
    image(output, name, code);
    writeln!(modules, "mod {name} {{ include!(\"{name}.rs\"); }}").unwrap();
}
