use std::path::Path;
use xbox_formats::fixtures::xbe_com_imports;
use xbox_recomp::{analysis, emit};

pub(super) fn sources(output: &Path) -> String {
    let mut code = Vec::new();
    allocate(&mut code);
    code.extend([0xC7, 0x00, 0xBE, 0xBA, 0xFE, 0xCA]); // mov [eax],CAFEBABE
    code.push(0x50); // push eax
    code.extend([0xFF, 0x15, 4, 0x20, 1, 0]); // MmFreeContiguousMemory
    allocate(&mut code);
    code.extend([0x8B, 0x10, 0xC3]); // mov edx,[eax]; ret
    let image = xbe_com_imports(&code, &[166, 171]).unwrap();
    let program = analysis::analyze(&image).unwrap();
    std::fs::write(
        output.join("src/kernel_free_contiguous.rs"),
        emit::rust_source(&program),
    )
    .unwrap();
    std::fs::write(output.join("src/kernel_free_contiguous.xbe"), image).unwrap();
    include_str!("../verification_free_contiguous.inc").to_owned()
}
fn allocate(code: &mut Vec<u8>) {
    for arg in [8192u32, 0x20000, 0x21FFF, 4096, 4].into_iter().rev() {
        code.push(0x68);
        code.extend(arg.to_le_bytes());
    }
    code.extend([0xFF, 0x15, 0, 0x20, 1, 0]);
}
