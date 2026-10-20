use std::fmt::Write;
use std::path::Path;

use super::image;

pub(super) fn sources(output: &Path) -> String {
    let mut modules = String::new();
    let mut cases = String::new();
    let mut count_cases = String::new();
    for (op, opcode) in [
        ("Movs", 0xA4),
        ("Stos", 0xAA),
        ("Lods", 0xAC),
        ("Cmps", 0xA6),
        ("Scas", 0xAE),
    ] {
        for (width, bits) in [("Byte", 8), ("Word", 16), ("Dword", 32)] {
            for address16 in [false, true] {
                for repeat in ["None", "Always", "Equal", "NotEqual"] {
                    let compare = matches!(op, "Cmps" | "Scas");
                    if (compare && repeat == "Always")
                        || (!compare && matches!(repeat, "Equal" | "NotEqual"))
                    {
                        continue;
                    }
                    let name = format!("str_{op}_{bits}_{address16}_{repeat}").to_lowercase();
                    let mut code = Vec::new();
                    if address16 {
                        code.push(0x67);
                    }
                    if bits == 16 {
                        code.push(0x66);
                    }
                    match repeat {
                        "Always" | "Equal" => code.push(0xF3),
                        "NotEqual" => code.push(0xF2),
                        _ => {}
                    }
                    code.extend_from_slice(&[opcode + u8::from(bits != 8), 0xC3]);
                    declare(output, &name, &code, &mut modules);
                    writeln!(cases, "Case {{ op: StringOp::{op}, width: Width::{width}, repeat: Repeat::{repeat}, address16: {address16}, run: {name}::run, image: include_bytes!(\"{name}.xbe\") }},").unwrap();
                }
            }
        }
    }
    for (op, opcode) in [
        ("Zero", 0xE3),
        ("Loop", 0xE2),
        ("Equal", 0xE1),
        ("NotEqual", 0xE0),
    ] {
        for address16 in [false, true] {
            let name = format!("count_{op}_{address16}").to_lowercase();
            let mut code = if address16 { vec![0x67] } else { vec![] };
            code.extend_from_slice(&[opcode, 6, 0xB8, 0, 0, 0, 0, 0xC3, 0xB8, 1, 0, 0, 0, 0xC3]);
            declare(output, &name, &code, &mut modules);
            writeln!(count_cases, "(\"{op}\", {address16}, {name}::run as Run, include_bytes!(\"{name}.xbe\").as_slice()),").unwrap();
        }
    }
    for (name, code) in [
        ("count_forever", &[0xE2, 0xFE, 0xC3][..]),
        ("direction", &[0xFD, 0xEB, 0, 0xFC, 0xC3][..]),
        ("string_setup", &[0x90, 0xF3, 0xA4, 0x90, 0xC3][..]),
    ] {
        declare(output, name, code, &mut modules);
    }
    include_str!("../verification_strings.inc")
        .replace("/* MODULES */", &modules)
        .replace("/* CASES */", &cases)
        .replace("/* COUNT_CASES */", &count_cases)
}

fn declare(output: &Path, name: &str, code: &[u8], modules: &mut String) {
    image(output, name, code);
    writeln!(
        modules,
        "#[cfg(test)] mod {name} {{ include!(\"{name}.rs\"); }}"
    )
    .unwrap();
}
