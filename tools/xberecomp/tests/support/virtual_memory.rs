use std::path::Path;
use xbox_formats::fixtures::xbe_com_imports;
use xbox_recomp::{analysis, emit};

pub(super) fn sources(output: &Path) -> String {
    let mut code = Vec::new();
    allocate(&mut code);
    code.extend([0xA3, 0x20, 0x21, 1, 0]); // primeiro NTSTATUS.
    code.extend([0x8B, 0x1D, 0, 0x21, 1, 0]); // EBX = base alocada.
    code.extend([0xC7, 0x83, 0xFE, 0x0F, 0, 0, 0xBE, 0xBA, 0xFE, 0xCA]); // dword atravessa páginas.
    code.extend([0xC7, 0x05, 0, 0x21, 1, 0, 0, 0, 0, 0]); // BaseAddress = NULL.
    code.extend([0xC7, 0x05, 4, 0x21, 1, 0, 0, 0x10, 0, 0]); // RegionSize = 4096.
    allocate(&mut code);
    code.push(0xC3);
    let image = xbe_com_imports(&code, &[184]).unwrap();
    let program = analysis::analyze(&image).unwrap();
    std::fs::write(
        output.join("src/kernel_virtual_memory.rs"),
        emit::rust_source(&program),
    )
    .unwrap();
    std::fs::write(output.join("src/kernel_virtual_memory.xbe"), image).unwrap();
    include_str!("../verification_virtual_memory.inc").to_owned()
}
fn allocate(code: &mut Vec<u8>) {
    for arg in [4u32, 0x3000, 0x12104, 0, 0x12100] {
        code.push(0x68);
        code.extend(arg.to_le_bytes());
    }
    code.extend([0xFF, 0x15, 0, 0x20, 1, 0]);
}
