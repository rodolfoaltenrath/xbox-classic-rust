use std::path::Path;
use xbox_formats::fixtures::xbe_com_imports;
use xbox_recomp::{analysis, emit};
pub(super) fn sources(output: &Path) -> String {
    let mut code = Vec::new();
    for (slot, args, store) in [
        (0u32, [0x12120u32, 0x12110], Some(0x12180u32)),
        (4, [0x12100, 0x12108], Some(0x12184)),
        (0, [0x12120, 0x12110], None),
    ] {
        for arg in args.into_iter().rev() {
            code.push(0x68);
            code.extend(arg.to_le_bytes());
        }
        code.extend([0xFF, 0x15]);
        code.extend((0x12000 + slot).to_le_bytes());
        if let Some(at) = store {
            code.push(0xA3);
            code.extend(at.to_le_bytes());
        }
    }
    code.extend([
        0x8B, 0x1D, 0x20, 0x21, 1, 0, 0x53, 0xFF, 0x15, 8, 0x20, 1, 0,
    ]); // close(handle).
    code.extend([0xB8, 123, 0, 0, 0, 0xC3]);
    let image = xbe_com_imports(&code, &[203, 67, 187]).unwrap();
    let program = analysis::analyze(&image).unwrap();
    std::fs::write(
        output.join("src/kernel_links.rs"),
        emit::rust_source(&program),
    )
    .unwrap();
    std::fs::write(output.join("src/kernel_links.xbe"), image).unwrap();
    include_str!("../verification_links.inc").to_owned()
}
