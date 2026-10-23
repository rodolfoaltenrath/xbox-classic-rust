use std::path::Path;
use xbox_formats::fixtures::xbe_com_imports;
use xbox_recomp::{analysis, emit};
pub(super) fn sources(output: &Path) -> String {
    let mut code = Vec::new();
    for (address, value) in [
        (0x80000000u32, 0x20001u32),
        (0xFD7111C0, 0xB002),
        (0xFD7111C4, 0x03FFFFFF),
        (0xFD7111C8, 3),
        (0xFD00322C, 0x111C),
        (0xFD003204, 0x100),
        (0xFD002504, 1),
        (0xFD003200, 1),
        (0xFD003250, 1),
        (0xFD003220, 1),
        (0xFD800040, 0x20000),
    ] {
        code.extend([0xC7, 0x05]);
        code.extend(address.to_le_bytes());
        code.extend(value.to_le_bytes());
    }
    code.extend([
        0xFF, 0x15, 0, 0x20, 1, 0, 0xA1, 0x44, 0x32, 0, 0xFD, 0xA3, 0, 0x21, 1, 0, 0xC3,
    ]);
    let image = xbe_com_imports(&code, &[1]).unwrap();
    let program = analysis::analyze(&image).unwrap();
    std::fs::write(output.join("src/dma_guest.rs"), emit::rust_source(&program)).unwrap();
    std::fs::write(output.join("src/dma_guest.xbe"), image).unwrap();
    include_str!("../verification_dma.inc").to_owned()
}
