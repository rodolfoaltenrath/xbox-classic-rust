use std::fmt::Write;
use std::path::Path;

use super::image;

pub(super) fn sources(output: &Path) -> String {
    let mut modules = String::new();
    let mut cases = String::new();
    let mut memory = String::new();
    let mut faults = String::new();
    for (op, opcode) in [("Shld", 0xA4), ("Shrd", 0xAC)] {
        for (width, bits) in [("Word", 16), ("Dword", 32)] {
            registers(output, op, opcode, width, bits, &mut modules, &mut cases);
            let name = format!("double_mem_{op}_{bits}").to_lowercase();
            let mut code = prefix(bits);
            code.extend_from_slice(&[0x0F, opcode + 1, 0x15, 0x20, 0x20, 1, 0, 0xC3]);
            declare(output, &name, &code, &mut modules);
            case(&mut memory, op, width, None, &name);
            for (suffix, address) in [("readonly", 0x11000u32), ("unmapped", 0x5000_0000)] {
                let name = format!("double_{op}_{bits}_{suffix}").to_lowercase();
                let mut code = prefix(bits);
                code.extend_from_slice(&[0x0F, opcode + 1, 0x15]);
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
    }
    for (name, code) in [
        ("double_dst_ecx", &[0x0F, 0xA5, 0xC1, 0xC3][..]), // shld ecx,eax,cl
        ("double_src_ecx", &[0x0F, 0xAD, 0xC8, 0xC3][..]), // shrd eax,ecx,cl
        ("double_same", &[0x66, 0x0F, 0xA4, 0xC0, 16, 0xC3][..]), // shld ax,ax,16
        ("double_bad_imm", &[0x66, 0x0F, 0xAC, 0xD0, 17, 0xC3][..]),
        (
            "double_bad_unmapped",
            &[0x66, 0x0F, 0xA4, 0x15, 0, 0, 0, 0x50, 17, 0xC3][..],
        ),
    ] {
        declare(output, name, code, &mut modules);
    }
    include_str!("../verification_double.inc")
        .replace("/* MODULES */", &modules)
        .replace("/* CASES */", &cases)
        .replace("/* MEMORY_CASES */", &memory)
        .replace("/* FAULT_CASES */", &faults)
}

fn registers(
    output: &Path,
    op: &str,
    opcode: u8,
    width: &str,
    bits: u32,
    modules: &mut String,
    cases: &mut String,
) {
    for mode in 0..10 {
        let name = format!("double_{op}_{bits}_{mode}").to_lowercase();
        let count = if mode == 0 {
            None
        } else {
            Some([0, 1, 15, 16, 17, 31, 32, 33, 255][mode - 1])
        };
        let mut code = prefix(bits);
        code.extend_from_slice(&[0x0F, opcode + u8::from(mode == 0), 0xD0]); // destino EAX, fonte EDX
        if let Some(count) = count {
            code.push(count);
        }
        code.push(0xC3);
        modules.push_str("#[cfg(target_arch = \"x86_64\")]\n");
        declare(output, &name, &code, modules);
        case(cases, op, width, count, &name);
    }
}

fn prefix(bits: u32) -> Vec<u8> {
    if bits == 16 { vec![0x66] } else { vec![] }
}

fn case(out: &mut String, op: &str, width: &str, count: Option<u8>, name: &str) {
    writeln!(out, "Case {{ op: DoubleShift::{op}, width: Width::{width}, count: {count:?}, run: {name}::run, image: include_bytes!(\"{name}.xbe\") }},").unwrap();
}

fn declare(output: &Path, name: &str, code: &[u8], modules: &mut String) {
    image(output, name, code);
    writeln!(modules, "mod {name} {{ include!(\"{name}.rs\"); }}").unwrap();
}
