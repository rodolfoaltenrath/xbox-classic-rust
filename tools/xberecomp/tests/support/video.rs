use std::path::Path;
use xbox_formats::fixtures::xbe_com_imports;
use xbox_recomp::{analysis, emit};
pub(super) fn sources(output: &Path) -> String {
    let mut code = Vec::new();
    for args in [
        [0xFD000000u32, 6, 0, 0x12100],
        [0xFD000000, 9, 1, 0],
        [0xFD000000, 9, 0, 0],
    ] {
        for arg in args.into_iter().rev() {
            code.push(0x68);
            code.extend(arg.to_le_bytes());
        }
        code.extend([0xFF, 0x15, 0, 0x20, 1, 0]);
    }
    code.extend([0xFF, 0x15, 4, 0x20, 1, 0, 0xA3, 8, 0x21, 1, 0]); // AvGetSavedDataAddress.
    code.extend([0xA1, 0, 0x21, 1, 0, 0xC3]);
    let bytes = xbe_com_imports(&code, &[2, 1]).unwrap();
    let program = analysis::analyze(&bytes).unwrap();
    std::fs::write(
        output.join("src/kernel_video.rs"),
        emit::rust_source(&program),
    )
    .unwrap();
    std::fs::write(output.join("src/kernel_video.xbe"), bytes).unwrap();
    include_str!("../verification_video.inc").to_owned()
}
