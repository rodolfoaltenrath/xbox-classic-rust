use std::path::Path;
use xbox_formats::fixtures::xbe_com_imports;
use xbox_recomp::{analysis, emit};

pub(super) fn sources(output: &Path) -> String {
    let mut code = Vec::new();
    call(&mut code, 0, &[0x12100, 0, 0x12104, 0x3000, 4]);
    code.extend([0x8B, 0x1D, 0, 0x21, 1, 0]); // EBX = allocated base
    code.extend([0x68, 0, 0x22, 1, 0, 0x53, 0xFF, 0x15, 4, 0x20, 1, 0]); // query(ebx,0x12200)
    code.extend([0xC7, 0x05, 4, 0x21, 1, 0, 0, 0, 0, 0]); // RegionSize = 0
    call(&mut code, 8, &[0x12100, 0x12104, 0x8000]);
    code.push(0xC3);
    let image = xbe_com_imports(&code, &[184, 217, 199]).unwrap();
    let program = analysis::analyze(&image).unwrap();
    std::fs::write(
        output.join("src/kernel_free_query_virtual.rs"),
        emit::rust_source(&program),
    )
    .unwrap();
    std::fs::write(output.join("src/kernel_free_query_virtual.xbe"), image).unwrap();
    include_str!("../verification_free_query_virtual.inc").to_owned()
}
fn call(code: &mut Vec<u8>, offset: u8, args: &[u32]) {
    for arg in args.iter().rev() {
        code.push(0x68);
        code.extend(arg.to_le_bytes());
    }
    code.extend([0xFF, 0x15, offset, 0x20, 1, 0]);
}
