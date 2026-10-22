use std::path::Path;
use xbox_formats::fixtures::xbe_com_imports;
use xbox_recomp::{analysis, emit};

pub(super) fn sources(output: &Path) -> String {
    let code = [
        0xA1, 0, 2, 0x10, 0xFD, 0xA3, 0, 0x21, 1, 0, // CFG0 -> output.
        0xA1, 4, 2, 0x10, 0xFD, 0xA3, 4, 0x21, 1, 0, // CFG1 -> output.
        0xFF, 0x15, 0, 0x20, 1, 0, // AvGetSavedDataAddress: fronteira snapshot.
        0xBA, 0, 2, 0x10, 0xFD, // EDX = CFG0.
        0xC7, 0x02, 0xEF, 0xBE, 0xAD, 0xDE, 0x8B, 0x02, 0xA3, 8, 0x21, 1, 0, 0xC3,
    ];
    let image = xbe_com_imports(&code, &[1]).unwrap();
    let program = analysis::analyze(&image).unwrap();
    std::fs::write(output.join("src/pfb_guest.rs"), emit::rust_source(&program)).unwrap();
    std::fs::write(output.join("src/pfb_guest.xbe"), image).unwrap();
    include_str!("../verification_pfb.inc").to_owned()
}
