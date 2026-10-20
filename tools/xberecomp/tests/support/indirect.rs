use std::fmt::Write;
use std::path::Path;
use xbox_formats::fixtures::xbe_com_codigo;
use xbox_recomp::{analysis, emit};

pub(super) fn sources(output: &Path) -> String {
    let mut modules = String::new();
    let mut registers = String::new();
    for register in 0..8u8 {
        for call in [false, true] {
            if call && register == 4 {
                continue;
            }
            let name = format!("ind_reg_{register}_{call}");
            let target = if call { 0x11008u32 } else { 0x11009 };
            let mut code = vec![0xB8 + register];
            code.extend_from_slice(&target.to_le_bytes());
            code.extend_from_slice(&[0xFF, if call { 0xD0 } else { 0xE0 } + register]);
            if call {
                code.push(0xC3);
            } else {
                code.extend_from_slice(&[0x0F, 0x0B]);
            }
            if register == 4 {
                code.extend_from_slice(&[0xBC, 0xFC, 0xFF, 0, 0x70]);
            }
            code.extend_from_slice(&[0xB8, 42, 0, 0, 0, 0xC3]);
            declare(output, &name, &code, &[], false, &mut modules);
            writeln!(registers, "({register}, {call}, {name}::run as Run, include_bytes!(\"{name}.xbe\").as_slice()),").unwrap();
        }
    }
    for (name, start, entries, readonly) in [
        (
            "ind_dynamic",
            vec![0xFF, 0xD0, 0xC3],
            vec![0x11008, 0x1100E],
            false,
        ),
        ("ind_jump", vec![0xFF, 0xE0], vec![0x11008, 0x1100E], false),
        (
            "ind_memory",
            vec![0xFF, 0x15, 0x20, 0x20, 1, 0, 0xC3],
            vec![0x11008, 0x1100E],
            false,
        ),
        (
            "ind_sib",
            vec![0xFF, 0x54, 0x8B, 0xFC, 0xC3],
            vec![0x11008],
            false,
        ),
        (
            "ind_table",
            vec![0xFF, 0x24, 0x8D, 0x20, 0x20, 1, 0],
            vec![0x11008, 0x1100E],
            false,
        ),
        (
            "ind_stack",
            vec![0xFF, 0x14, 0x24, 0x83, 0xC4, 4, 0xC3],
            vec![0x11008],
            false,
        ),
        (
            "ind_stack_alias",
            vec![0xFF, 0x54, 0x24, 0xFC, 0xC3],
            vec![0x11008],
            false,
        ),
        (
            "ind_readonly",
            vec![0xFF, 0x15, 0x20, 0x20, 1, 0, 0xC3],
            vec![],
            true,
        ),
        ("ind_return", vec![0xC3], vec![0x11008], false),
    ] {
        let mut code = start;
        code.resize(8, 0xCC);
        code.extend_from_slice(&[0xB8, 42, 0, 0, 0, 0xC3, 0xB8, 99, 0, 0, 0, 0xC3]);
        declare(output, name, &code, &entries, readonly, &mut modules);
    }
    include_str!("../verification_indirect.inc")
        .replace("/* MODULES */", &modules)
        .replace("/* REGISTERS */", &registers)
}

fn declare(
    output: &Path,
    name: &str,
    code: &[u8],
    entries: &[u32],
    readonly: bool,
    modules: &mut String,
) {
    let mut image = xbe_com_codigo(code).unwrap();
    if readonly {
        image[0x338..0x33C].copy_from_slice(&0u32.to_le_bytes());
        image[0x2020..0x2024].copy_from_slice(&0x11008u32.to_le_bytes());
    }
    let program = analysis::analyze_with_entries(&image, entries).unwrap();
    std::fs::write(
        output.join(format!("src/{name}.rs")),
        emit::rust_source(&program),
    )
    .unwrap();
    std::fs::write(output.join(format!("src/{name}.xbe")), image).unwrap();
    writeln!(
        modules,
        "#[cfg(test)] mod {name} {{ include!(\"{name}.rs\"); }}"
    )
    .unwrap();
}
