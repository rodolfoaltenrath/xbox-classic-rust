use std::fmt::Write;
use std::path::Path;

use super::image;

#[derive(Default)]
struct Sources {
    modules: String,
    core: String,
    memory: String,
    faults: String,
    truncated: String,
}

pub(super) fn sources(output: &Path) -> String {
    let mut s = Sources::default();
    for (op, signed, divide, group) in [
        ("mul", false, false, 4),
        ("imul", true, false, 5),
        ("div", false, true, 6),
        ("idiv", true, true, 7),
    ] {
        for (width, bits) in [("Byte", 8), ("Word", 16), ("Dword", 32)] {
            let name = format!("md_{op}{bits}");
            let mut code = prefix(bits);
            code.extend_from_slice(&[0xF6 + u8::from(bits != 8), 0xC1 | group << 3, 0xC3]);
            declare(output, &name, &code, &mut s.modules);
            writeln!(s.core, "Core {{ signed: {signed}, divide: {divide}, width: Width::{width}, run: {name}::run, image: include_bytes!(\"{name}.xbe\") }},").unwrap();
            for (suffix, address) in [("memory", 0x12020u32), ("unmapped", 0x5000_0000)] {
                let name = format!("md_{op}{bits}_{suffix}");
                let mut code = prefix(bits);
                code.extend_from_slice(&[0xF6 + u8::from(bits != 8), 0x05 | group << 3]);
                code.extend_from_slice(&address.to_le_bytes());
                code.push(0xC3);
                declare(output, &name, &code, &mut s.modules);
                if suffix == "memory" {
                    writeln!(s.memory, "Core {{ signed: {signed}, divide: {divide}, width: Width::{width}, run: {name}::run, image: include_bytes!(\"{name}.xbe\") }},").unwrap();
                } else {
                    writeln!(
                        s.faults,
                        "({name}::run as Run, include_bytes!(\"{name}.xbe\") as &[u8]),"
                    )
                    .unwrap();
                }
            }
        }
    }
    truncated(output, &mut s);
    extras(output, &mut s);
    include_str!("../verification_muldiv.inc")
        .replace(
            "/* HARDWARE */",
            include_str!("../verification_muldiv_hardware.inc"),
        )
        .replace("/* MODULES */", &s.modules)
        .replace("/* CORE */", &s.core)
        .replace("/* MEMORY */", &s.memory)
        .replace("/* FAULTS */", &s.faults)
        .replace("/* TRUNCATED */", &s.truncated)
}

fn truncated(output: &Path, s: &mut Sources) {
    for (width, bits) in [("Word", 16), ("Dword", 32)] {
        for mode in 0..6 {
            let name = format!("md_truncated{bits}_{mode}");
            let mut code = prefix(bits);
            let factor = match mode {
                0 => None,
                1 => Some(0xFFFF_FF80u32),
                2 => Some(u32::MAX),
                3 => Some(0),
                4 => Some(127),
                _ => Some(1 << (bits - 1)),
            };
            if mode == 0 {
                code.extend_from_slice(&[0x0F, 0xAF, 0xC1]);
            } else if mode == 5 {
                code.extend_from_slice(&[0x69, 0xC1]);
                let immediate = factor.unwrap().to_le_bytes();
                code.extend_from_slice(&immediate[..(bits / 8) as usize]);
            } else {
                code.extend_from_slice(&[0x6B, 0xC1, factor.unwrap() as u8]);
            }
            code.push(0xC3);
            s.modules.push_str("#[cfg(target_arch = \"x86_64\")]\n");
            declare(output, &name, &code, &mut s.modules);
            writeln!(s.truncated, "Truncated {{ width: Width::{width}, factor: {factor:?}, run: {name}::run, image: include_bytes!(\"{name}.xbe\") }},").unwrap();
        }
    }
}

fn extras(output: &Path, s: &mut Sources) {
    for (name, code) in [
        ("md_mul_ah", &[0xF6, 0xE4, 0xC3][..]),
        ("md_imul_edx", &[0xF7, 0xEA, 0xC3][..]),
        ("md_idiv_ah", &[0xF6, 0xFC, 0xC3][..]),
        ("md_div_eax", &[0xF7, 0xF0, 0xC3][..]),
        ("md_idiv_edx", &[0xF7, 0xFA, 0xC3][..]),
        (
            "md_imul_mem2",
            &[0x0F, 0xAF, 0x05, 0x20, 0x20, 1, 0, 0xC3][..],
        ),
        (
            "md_imul_mem3",
            &[0x6B, 0x05, 0x20, 0x20, 1, 0, 0xFF, 0xC3][..],
        ),
        (
            "md_imul_fault2",
            &[0x0F, 0xAF, 0x05, 0, 0, 0, 0x50, 0xC3][..],
        ),
        (
            "md_imul_fault3",
            &[0x6B, 0x05, 0, 0, 0, 0x50, 0xFF, 0xC3][..],
        ),
        ("md_mul_readonly", &[0xF6, 0x25, 0, 0x10, 1, 0, 0xC3][..]),
        ("md_div_readonly", &[0xF6, 0x35, 0, 0x10, 1, 0, 0xC3][..]),
        ("md_cbw", &[0x66, 0x98, 0xC3][..]),
        ("md_cwde", &[0x98, 0xC3][..]),
        ("md_cwd", &[0x66, 0x99, 0xC3][..]),
        ("md_cdq", &[0x99, 0xC3][..]),
        ("md_snapshot", &[0xF7, 0xE1, 0xEB, 0, 0xF7, 0xF3, 0xC3][..]),
        (
            "md_signed_snapshot",
            &[0xF7, 0xE9, 0xEB, 0, 0xF7, 0xFB, 0xC3][..],
        ),
    ] {
        declare(output, name, code, &mut s.modules);
    }
}

fn prefix(bits: u32) -> Vec<u8> {
    if bits == 16 { vec![0x66] } else { vec![] }
}
fn declare(output: &Path, name: &str, code: &[u8], modules: &mut String) {
    image(output, name, code);
    writeln!(modules, "mod {name} {{ include!(\"{name}.rs\"); }}").unwrap();
}
