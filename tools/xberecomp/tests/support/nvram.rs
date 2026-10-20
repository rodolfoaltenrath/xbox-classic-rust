use std::path::Path;
use xbox_formats::fixtures::xbe_com_imports;
use xbox_recomp::{analysis, emit};

pub(super) fn sources(output: &Path) -> String {
    let mut code = Vec::new();
    for (slot, args) in [
        (0, vec![0xFFFFu32, 0x12100, 0x12200, 256, 0x12104]),
        (1, vec![0x12400]),
        (2, vec![0x12400, 0x12200, 256]),
        (3, vec![0x12400, 0x12500]),
    ] {
        for word in args.into_iter().rev() {
            code.push(0x68);
            code.extend(word.to_le_bytes());
        }
        code.extend([0xFF, 0x15]);
        code.extend((0x12000u32 + slot * 4).to_le_bytes());
    }
    code.push(0xC3);
    let image = xbe_com_imports(&code, &[24, 335, 336, 337]).unwrap();
    let program = analysis::analyze(&image).unwrap();
    std::fs::write(
        output.join("src/kernel_nvram.rs"),
        emit::rust_source(&program),
    )
    .unwrap();
    std::fs::write(output.join("src/kernel_nvram.xbe"), image).unwrap();
    include_str!("../verification_nvram.inc").to_owned()
}
