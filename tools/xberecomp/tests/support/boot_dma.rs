use std::path::Path;
use xbox_formats::fixtures::xbe_com_imports;
use xbox_recomp::{analysis, emit};
pub(super) fn sources(out: &Path) -> String {
    let code = [
        0xB8, 1, 0x50, 0x34, 0x12, 0xA3, 0, 0, 0, 0x80, 0x0F, 0x09, 0xFF, 0x15, 0, 0x20, 1, 0,
        0xA1, 0, 0, 0, 0x80, 0xA3, 0, 0x21, 1, 0, 0xC7, 0x05, 0, 0, 0, 0x80, 0xFF, 0xFF, 0xFF,
        0xFF, 0xA1, 0, 0, 0, 0x80, 0xA3, 4, 0x21, 1, 0, 0xC3,
    ];
    let image = xbe_com_imports(&code, &[1]).unwrap();
    let p = analysis::analyze(&image).unwrap();
    std::fs::write(out.join("src/boot_dma_guest.rs"), emit::rust_source(&p)).unwrap();
    std::fs::write(out.join("src/boot_dma_guest.xbe"), image).unwrap();
    include_str!("../verification_boot_dma.inc").to_owned()
}
