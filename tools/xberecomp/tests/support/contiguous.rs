use std::path::Path;
use xbox_formats::fixtures::xbe_com_imports;
use xbox_recomp::{analysis, emit};
pub(super) fn sources(output: &Path) -> String {
    let mut code = Vec::new();
    allocate(&mut code, [0x2001, 0x20000, 0x3FFFFFF, 0x1000, 0x404]);
    code.extend([0xA3, 0, 0x21, 1, 0]); // endereço virtual devolvido.
    code.extend([0xC7, 0x80, 0xFE, 0x0F, 0, 0, 0xBE, 0xBA, 0xFE, 0xCA]); // store entre páginas.
    code.extend([
        0x89, 0xC1, 0x81, 0xE1, 0xFF, 0xFF, 0xFF, 0x7F, 0x89, 0x0D, 4, 0x21, 1, 0,
    ]); // VA -> PA como nxdk.
    allocate(&mut code, [4096, 0x40000, 0x4FFFF, 0x10000, 4]);
    code.push(0xC3);
    let image = xbe_com_imports(&code, &[166]).unwrap();
    let program = analysis::analyze(&image).unwrap();
    std::fs::write(
        output.join("src/kernel_contiguous.rs"),
        emit::rust_source(&program),
    )
    .unwrap();
    std::fs::write(output.join("src/kernel_contiguous.xbe"), image).unwrap();
    include_str!("../verification_contiguous.inc").to_owned()
}
fn allocate(code: &mut Vec<u8>, args: [u32; 5]) {
    for arg in args.into_iter().rev() {
        code.push(0x68);
        code.extend(arg.to_le_bytes());
    }
    code.extend([0xFF, 0x15, 0, 0x20, 1, 0]);
}
