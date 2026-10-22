use std::path::Path;
use xbox_formats::fixtures::xbe_com_imports;
use xbox_recomp::{analysis, emit};

pub(super) fn sources(output: &Path) -> String {
    let code = [
        0xA1, 0, 5, 0x68, 0xFD, 0xA3, 0, 0x21, 1, 0, // reset -> output.
        0xFF, 0x15, 0, 0x20, 1, 0, // AvGetSavedDataAddress: fronteira para snapshot.
        0xBA, 0, 5, 0x68, 0xFD, // EDX = PLL.
        0xC7, 0x02, 2, 9, 1, 0, // [EDX] = 0x00010902.
        0x8B, 0x02, 0xA3, 4, 0x21, 1, 0, 0xC3,
    ];
    let image = xbe_com_imports(&code, &[1]).unwrap();
    let program = analysis::analyze(&image).unwrap();
    std::fs::write(
        output.join("src/pramdac_guest.rs"),
        emit::rust_source(&program),
    )
    .unwrap();
    std::fs::write(output.join("src/pramdac_guest.xbe"), image).unwrap();
    include_str!("../verification_pramdac.inc").to_owned()
}
