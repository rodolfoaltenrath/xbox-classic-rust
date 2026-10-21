use std::path::Path;
use xbox_formats::fixtures::xbe_com_imports;
use xbox_recomp::{analysis, emit};

pub(super) fn sources(output: &Path) -> String {
    let mut code = Vec::new();
    for (option, parameter) in [(11u32, 5u32), (14, 0), (11, 7), (14, 1)] {
        for arg in [0, parameter, option, 0xFD000000] {
            code.push(0x68);
            code.extend(arg.to_le_bytes());
        }
        code.extend([0xFF, 0x15, 0, 0x20, 1, 0]);
    }
    code.push(0xC3);
    let image = xbe_com_imports(&code, &[2]).unwrap();
    let program = analysis::analyze(&image).unwrap();
    std::fs::write(
        output.join("src/kernel_encoder_filters.rs"),
        emit::rust_source(&program),
    )
    .unwrap();
    std::fs::write(output.join("src/kernel_encoder_filters.xbe"), image).unwrap();
    include_str!("../verification_encoder_filters.inc").to_owned()
}
