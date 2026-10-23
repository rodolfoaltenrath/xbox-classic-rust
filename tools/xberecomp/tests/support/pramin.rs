use std::path::Path;
use xbox_formats::fixtures::xbe_com_imports;
use xbox_recomp::{analysis, emit};

pub(super) fn sources(output: &Path) -> String {
    let code = [
        0xBA, 0xA0, 0x10, 0x71, 0xFD, // EDX = instância em PRAMIN.
        0xC7, 0x02, 0xBE, 0xBA, 0xFE, 0xCA, // [EDX] = CAFEBABE.
        0xFF, 0x15, 0, 0x20, 1, 0, // AvGetSavedDataAddress: fronteira snapshot.
        0x8B, 0x02, 0xA3, 0, 0x21, 1, 0, // [EDX] -> output.
        0xC7, 0x02, 0xEF, 0xBE, 0xAD, 0xDE, // [EDX] = DEADBEEF.
        0x8B, 0x02, 0xA3, 4, 0x21, 1, 0, 0xC3,
    ];
    let image = xbe_com_imports(&code, &[1]).unwrap();
    let program = analysis::analyze(&image).unwrap();
    std::fs::write(
        output.join("src/pramin_guest.rs"),
        emit::rust_source(&program),
    )
    .unwrap();
    std::fs::write(output.join("src/pramin_guest.xbe"), image).unwrap();
    include_str!("../verification_pramin.inc").to_owned()
}
