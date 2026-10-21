use std::path::Path;
use xbox_formats::fixtures::xbe_com_imports;
use xbox_recomp::{analysis, emit};

pub(super) fn sources(output: &Path) -> String {
    let mut code = Vec::new();
    for pointer in [0x12101u32, 0x12110] {
        code.push(0x68);
        code.extend(pointer.to_le_bytes());
        code.extend([0xFF, 0x15, 0, 0x20, 1, 0]);
    }
    code.push(0xC3);
    let image = xbe_com_imports(&code, &[128]).unwrap();
    let program = analysis::analyze(&image).unwrap();
    std::fs::write(
        output.join("src/kernel_system_time.rs"),
        emit::rust_source(&program),
    )
    .unwrap();
    std::fs::write(output.join("src/kernel_system_time.xbe"), image).unwrap();
    include_str!("../verification_system_time.inc").to_owned()
}
