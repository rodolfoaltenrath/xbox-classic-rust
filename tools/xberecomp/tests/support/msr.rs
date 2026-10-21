use std::{fmt::Write, path::Path};

pub(super) fn sources(output: &Path) -> String {
    let mut modules = String::new();
    for (name, code) in [
        ("msr_direct", &[0x0F, 0x32, 0xEB, 0, 0xC3][..]),
        ("msr_store", &[0x0F, 0x32, 0xA3, 0x20, 0x20, 1, 0, 0xC3][..]),
        (
            "msr_tsc",
            &[
                0x0F, 0x31, // rdtsc
                0x89, 0xC3, 0x89, 0xD6, // mov ebx,eax; mov esi,edx
                0xB9, 0x10, 0, 0, 0, 0xEB, 0, // mov ecx,0x10; jmp next
                0x0F, 0x32, // rdmsr
                0xA3, 0x20, 0x20, 1, 0, 0x89, 0x15, 0x24, 0x20, 1, 0, // save EDX:EAX
                0xC3,
            ][..],
        ),
    ] {
        super::image(output, name, code);
        writeln!(modules, "mod {name} {{ include!(\"{name}.rs\"); }}").unwrap();
    }
    include_str!("../verification_msr.inc").replace("/* MODULES */", &modules)
}
