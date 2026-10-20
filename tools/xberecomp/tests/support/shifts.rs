use std::fmt::Write;
use std::path::Path;

use super::image;

pub(super) fn sources(output: &Path) -> String {
    let mut declarations = String::new();
    let mut cases = String::new();
    let mut memory_cases = String::new();
    let mut fault_cases = String::new();
    for (op, group) in [("Shl", 4), ("Shr", 5), ("Sar", 7)] {
        for (width, bits) in [("Byte", 8), ("Word", 16), ("Dword", 32)] {
            for mode in 0..10 {
                let name = format!("shift_{op}_{bits}_{mode}").to_lowercase();
                let mut code = if bits == 16 { vec![0x66] } else { vec![] };
                let count = match mode {
                    0 => None,
                    1 => Some(1), // D0/D1: contagem implícita
                    _ => Some([0, 1, 8, 16, 31, 32, 33, 255][mode - 2]),
                };
                let opcode = match mode {
                    0 => 0xD2,
                    1 => 0xD0,
                    _ => 0xC0,
                };
                code.extend_from_slice(&[opcode + u8::from(bits != 8), 0xC0 | (group << 3)]);
                if mode >= 2 {
                    code.push(count.unwrap());
                }
                code.push(0xC3);
                declarations.push_str("#[cfg(target_arch = \"x86_64\")]\n");
                declare(output, &name, &code, &mut declarations);
                writeln!(
                    cases,
                    "Case {{ op: Shift::{op}, width: Width::{width}, count: {count:?}, run: {name}::run, image: include_bytes!(\"{name}.xbe\") }},"
                )
                .unwrap();
            }
            let name = format!("shift_mem_{op}_{bits}").to_lowercase();
            let mut code = if bits == 16 { vec![0x66] } else { vec![] };
            code.extend_from_slice(&[
                0xD2 + u8::from(bits != 8),
                0x05 | (group << 3),
                0x20,
                0x20,
                1,
                0,
                0xC3,
            ]);
            declare(output, &name, &code, &mut declarations);
            writeln!(
                memory_cases,
                "Case {{ op: Shift::{op}, width: Width::{width}, count: None, run: {name}::run, image: include_bytes!(\"{name}.xbe\") }},"
            )
            .unwrap();
        }
        for (suffix, address) in [("readonly", 0x11000u32), ("unmapped", 0x5000_0000)] {
            let name = format!("shift_{op}_{suffix}").to_lowercase();
            let mut code = vec![0xD3, 0x05 | (group << 3)];
            code.extend_from_slice(&address.to_le_bytes());
            code.push(0xC3);
            declare(output, &name, &code, &mut declarations);
            writeln!(
                fault_cases,
                "({name}::run as Run, include_bytes!(\"{name}.xbe\") as &[u8]),"
            )
            .unwrap();
        }
    }
    for (name, code) in [
        ("shift_ah", &[0xD2, 0xE4, 0xC3][..]),       // shl ah,cl
        ("shift_cl", &[0xD2, 0xE9, 0xC3][..]),       // shr cl,cl
        ("shift_cx", &[0x66, 0xD3, 0xF9, 0xC3][..]), // sar cx,cl
        ("shift_ecx", &[0xD3, 0xE1, 0xC3][..]),      // shl ecx,cl
        (
            "shift_snapshot",
            &[0xD1, 0xE0, 0xEB, 0, 0x83, 0xD2, 0, 0xC3][..], // shl eax,1; jmp next; adc edx,0; ret
        ),
    ] {
        declare(output, name, code, &mut declarations);
    }
    include_str!("../verification_shift.inc")
        .replace("/* MODULES */", &declarations)
        .replace("/* CASES */", &cases)
        .replace("/* MEMORY_CASES */", &memory_cases)
        .replace("/* FAULT_CASES */", &fault_cases)
}

fn declare(output: &Path, name: &str, code: &[u8], declarations: &mut String) {
    image(output, name, code);
    writeln!(declarations, "mod {name} {{ include!(\"{name}.rs\"); }}").unwrap();
}
