use std::fmt::Write;
use std::path::Path;

use xbox_formats::fixtures::xbe_com_codigo;
use xbox_recomp::{analysis, emit};

mod bit_test;
mod blocks;
mod breakpoint;
mod conditional;
mod contiguous;
mod control;
mod critical;
mod delay;
mod display;
mod double_shifts;
mod encoder_filters;
mod exchange;
mod indirect;
mod interlocked;
mod irql;
mod kernel;
mod links;
mod msr;
mod muldiv;
mod nop;
mod nvram;
mod ordering;
mod pci;
mod rotations;
mod scan;
mod sha;
mod shifts;
mod sse;
mod strings;
mod system_time;
mod threads;
mod timing;
mod video;
mod virtual_memory;
mod x87;

fn image(output: &Path, name: &str, code: &[u8]) {
    let image = xbe_com_codigo(code).unwrap();
    let program = analysis::analyze(&image).unwrap();
    std::fs::write(
        output.join(format!("src/{name}.rs")),
        emit::rust_source(&program),
    )
    .unwrap();
    std::fs::write(output.join(format!("src/{name}.xbe")), image).unwrap();
}

/// Testes compilados no mesmo projeto que o código traduzido, incluindo uma
/// referência independente que executa a ALU da CPU `x86_64` física.
pub(super) fn extend_project(output: &Path, root: &Path, main: &mut String) {
    let manifest = output.join("Cargo.toml");
    let mut config = std::fs::read_to_string(&manifest).unwrap();
    let cpu_path = toml::Value::String(root.join("crates/xbox-cpu").to_string_lossy().into_owned());
    write!(
        config,
        "\n[dev-dependencies]\nxbox-cpu = {{ path = {cpu_path}, features = [\"isa-oracle\"] }}\n"
    )
    .unwrap();
    std::fs::write(manifest, config).unwrap();

    structural_images(output);
    carry_images(output);
    main.push_str(include_str!("../verification_more.inc"));
    main.push_str(include_str!("../verification_carry.inc"));
    main.push_str(&hardware_sources(output));
    main.push_str(&shifts::sources(output));
    main.push_str(&rotations::sources(output));
    main.push_str(&double_shifts::sources(output));
    main.push_str(&muldiv::sources(output));
    main.push_str(&strings::sources(output));
    main.push_str(&indirect::sources(output));
    main.push_str(&kernel::sources(output));
    main.push_str(&links::sources(output));
    main.push_str(&interlocked::sources(output));
    main.push_str(&irql::sources(output));
    main.push_str(&virtual_memory::sources(output));
    main.push_str(&contiguous::sources(output));
    main.push_str(&bit_test::sources(output));
    main.push_str(&delay::sources(output));
    main.push_str(&sse::sources(output));
    main.push_str(&x87::sources(output));
    main.push_str(&video::sources(output));
    main.push_str(&display::sources(output));
    main.push_str(&encoder_filters::sources(output));
    main.push_str(&system_time::sources(output));
    main.push_str(&critical::sources(output));
    main.push_str(&sha::sources(output));
    main.push_str(&nvram::sources(output));
    main.push_str(&pci::sources(output));
    main.push_str(&exchange::sources(output));
    main.push_str(&timing::sources(output));
    main.push_str(&threads::sources(output));
    main.push_str(&msr::sources(output));
    main.push_str(&control::sources(output));
    main.push_str(&nop::sources(output));
    main.push_str(&breakpoint::sources(output));
    main.push_str(&ordering::sources(output));
    main.push_str(&conditional::sources(output));
    main.push_str(&scan::sources(output));
    main.push_str(&blocks::sources(output));
}

fn structural_images(output: &Path) {
    for (name, code) in [
        (
            "parts",
            &[
                0xB8, 0x78, 0x56, 0x34, 0x12, // mov eax,0x12345678
                0xB4, 0xAB, 0xB0, 0xCD, // mov ah,0xab; mov al,0xcd
                0x66, 0xB8, 0x01, 0xEF, // mov ax,0xef01
                0x88, 0x25, 0x20, 0x20, 1, 0, // mov [0x12020],ah
                0x0F, 0xB6, 0x15, 0x20, 0x20, 1, 0, // movzx edx,byte [data]
                0x0F, 0xBE, 0x1D, 0x20, 0x20, 1, 0, // movsx ebx,byte [data]
                0x66, 0xA3, 0x22, 0x20, 1, 0, // mov [0x12022],ax
                0x0F, 0xB7, 0x35, 0x22, 0x20, 1, 0, // movzx esi,word [data+2]
                0x0F, 0xBF, 0x3D, 0x22, 0x20, 1, 0, // movsx edi,word [data+2]
                0x8D, 0x48, 8, // lea ecx,[eax+8]: endereço não mapeado
                0xC3,
            ][..],
        ),
        (
            "high_byte",
            &[0xB8, 0x56, 0x7F, 0x34, 0x12, 0xB1, 1, 0x00, 0xCC, 0xC3][..],
        ),
        (
            "frame",
            &[
                0x55, 0x89, 0xE5, 0x83, 0xEC, 8, // push ebp; mov ebp,esp; sub esp,8
                0xC7, 0x45, 0xFC, 42, 0, 0, 0, // mov dword [ebp-4],42
                0x8B, 0x45, 0xFC, 0xC9, 0xC3, // mov eax,[ebp-4]; leave; ret
            ][..],
        ),
        ("push_esp", &[0x89, 0xE2, 0x54, 0x58, 0xC3][..]),
        ("pop_esp", &[0x89, 0xE0, 0x50, 0x5C, 0xC3][..]),
        (
            "pop_mem_esp",
            &[
                0x83, 0xEC, 16, 0x6A, 5, // sub esp,16; push 5
                0x8F, 0x44, 0x24, 4, // pop [esp+4] (ESP depois do incremento)
                0x8B, 0x44, 0x24, 4, // mov eax,[esp+4]
                0x83, 0xC4, 16, 0xC3, // add esp,16; ret
            ][..],
        ),
        (
            "word_stack",
            &[0x66, 0x6A, 0x80, 0x66, 0x58, 0x0F, 0xBF, 0xC8, 0xC3][..],
        ),
        (
            "byte_inc",
            &[
                0xC6, 0x05, 0x20, 0x20, 1, 0, 0xFF, // mov byte [data],0xff
                0xFE, 0x05, 0x20, 0x20, 1, 0, // inc byte [data]
                0x0F, 0xB6, 0x05, 0x20, 0x20, 1, 0, // movzx eax,byte [data]
                0xC3,
            ][..],
        ),
    ] {
        image(output, name, code);
    }
}

fn carry_images(output: &Path) {
    for (name, code) in [
        // A fronteira de bloco separa a palavra baixa da palavra alta.
        ("adc_chain", &[0x01, 0xC8, 0xEB, 0, 0x11, 0xDA, 0xC3][..]),
        ("sbb_chain", &[0x29, 0xC8, 0xEB, 0, 0x19, 0xDA, 0xC3][..]),
        ("adc_high", &[0x10, 0xCC, 0xC3][..]),
        ("sbb_high", &[0x18, 0xCC, 0xC3][..]),
        // Imediato de byte e imediato com sinal estendido para word.
        (
            "adc_memory",
            &[0x80, 0x15, 0x20, 0x20, 1, 0, 0xFF, 0xC3][..],
        ),
        (
            "sbb_memory",
            &[0x66, 0x83, 0x1D, 0x20, 0x20, 1, 0, 0xFF, 0xC3][..],
        ),
        ("adc_load", &[0x13, 0x05, 0x20, 0x20, 1, 0, 0xC3][..]),
        ("sbb_load", &[0x1B, 0x05, 0x20, 0x20, 1, 0, 0xC3][..]),
        // A leitura de código RX funciona; a escrita deve falhar antes de FLAGS.
        ("adc_readonly", &[0x11, 0x0D, 0, 0x10, 1, 0, 0xC3][..]),
        ("sbb_readonly", &[0x19, 0x0D, 0, 0x10, 1, 0, 0xC3][..]),
        // CLC/STC/CMC com fronteira de bloco entre cada uma, para observar CF passo a
        // passo e provar que nenhuma outra flag é tocada.
        (
            "flag_ops",
            &[
                0xF9, 0xEB, 0, // stc; fronteira
                0xF8, 0xEB, 0, // clc; fronteira
                0xF5, 0xEB, 0, // cmc; fronteira
                0xF5, 0xC3, // cmc; ret
            ][..],
        ),
    ] {
        image(output, name, code);
    }
}

fn hardware_sources(output: &Path) -> String {
    let mut declarations = String::new();
    let mut alu_cases = String::new();
    for (name, variant, byte, wide, modrm) in [
        ("add", "Add", 0x00, 0x01, 0xC8),
        ("adc", "Adc", 0x10, 0x11, 0xC8),
        ("sub", "Sub", 0x28, 0x29, 0xC8),
        ("sbb", "Sbb", 0x18, 0x19, 0xC8),
        ("xor", "Xor", 0x30, 0x31, 0xC8),
        ("and", "And", 0x20, 0x21, 0xC8),
        ("or", "Or", 0x08, 0x09, 0xC8),
        ("cmp", "Cmp", 0x38, 0x39, 0xC8),
        ("test", "Test", 0x84, 0x85, 0xC8),
        ("inc", "Inc", 0xFE, 0xFF, 0xC0),
        ("dec", "Dec", 0xFE, 0xFF, 0xC8),
        ("neg", "Neg", 0xF6, 0xF7, 0xD8),
        ("not", "Not", 0xF6, 0xF7, 0xD0),
    ] {
        for (width, bits) in [("Byte", 8), ("Word", 16), ("Dword", 32)] {
            let module = format!("alu_{name}{bits}");
            let mut code = Vec::new();
            if bits == 16 {
                code.push(0x66);
            }
            code.extend_from_slice(&[if bits == 8 { byte } else { wide }, modrm, 0xC3]);
            image(output, &module, &code);
            writeln!(
                declarations,
                "mod {module} {{ include!(\"{module}.rs\"); }}"
            )
            .unwrap();
            writeln!(
                alu_cases,
                "(Alu::{variant}, Width::{width}, {module}::run, include_bytes!(\"{module}.xbe\")),"
            )
            .unwrap();
        }
    }

    let mut branch_cases = String::new();
    for condition in 0..16 {
        for near in [false, true] {
            let name = format!("jcc_{condition}_{}", u8::from(near));
            let mut code = vec![0x39, 0xC8]; // cmp eax,ecx
            if near {
                code.extend_from_slice(&[0x0F, 0x80 + condition, 6, 0, 0, 0]);
            } else {
                code.extend_from_slice(&[0x70 + condition, 6]);
            }
            code.extend_from_slice(&[0xB8, 0, 0, 0, 0, 0xC3, 0xB8, 1, 0, 0, 0, 0xC3]);
            image(output, &name, &code);
            writeln!(declarations, "mod {name} {{ include!(\"{name}.rs\"); }}").unwrap();
            writeln!(
                branch_cases,
                "({condition}, {name}::run, include_bytes!(\"{name}.xbe\")),"
            )
            .unwrap();
        }
    }
    include_str!("../verification_hardware.inc")
        .replace("/* MODULES */", &declarations)
        .replace("/* ALU_CASES */", &alu_cases)
        .replace("/* BRANCH_CASES */", &branch_cases)
}
