use std::path::Path;
use xbox_formats::fixtures::xbe_com_imports;
use xbox_recomp::{analysis, emit};

pub(super) fn sources(output: &Path) -> String {
    let code = [
        0x68, 1, 0x10, 0, 0, // push 4097
        0xFF, 0x15, 0, 0x20, 1, 0, // allocate simple
        0x89, 0xC3, // mov ebx,eax
        0xC7, 0, 0xBE, 0xBA, 0xFE, 0xCA, // mov [eax],CAFEBABE
        0x50, 0xFF, 0x15, 4, 0x20, 1, 0, // query size
        0x89, 0xC2, // mov edx,eax
        0x53, 0xFF, 0x15, 8, 0x20, 1, 0, // free
        0xC3,
    ];
    let image = xbe_com_imports(&code, &[165, 180, 171]).unwrap();
    let program = analysis::analyze(&image).unwrap();
    std::fs::write(
        output.join("src/kernel_simple_contiguous.rs"),
        emit::rust_source(&program),
    )
    .unwrap();
    std::fs::write(output.join("src/kernel_simple_contiguous.xbe"), image).unwrap();
    include_str!("../verification_simple_contiguous.inc").to_owned()
}
