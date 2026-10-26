use std::path::Path;
use xbox_formats::fixtures::xbe_com_imports;
use xbox_recomp::{analysis, emit};
pub(super) fn sources(output: &Path) -> String {
    let mut code = Vec::new();
    for (slot, args) in [
        (
            0u32,
            vec![
                Some(0x12120),
                Some(0x80100080),
                Some(0x12100),
                Some(0x12130),
                Some(0),
                Some(0x80),
                Some(1),
                Some(1),
                Some(0x60),
            ],
        ),
        (
            4,
            vec![
                None,
                Some(0),
                Some(0),
                Some(0),
                Some(0x12130),
                Some(0x12400),
                Some(4),
                Some(0),
            ],
        ),
        (
            8,
            vec![None, Some(0x12130), Some(0x12440), Some(56), Some(34)],
        ),
        (
            4,
            vec![
                None,
                Some(0),
                Some(0),
                Some(0),
                Some(0x12130),
                Some(0x12408),
                Some(8),
                Some(0),
            ],
        ),
        (12, vec![None]),
    ] {
        for arg in args.into_iter().rev() {
            if let Some(arg) = arg {
                code.push(0x68);
                code.extend(u32::to_le_bytes(arg));
            } else {
                code.extend([0xFF, 0x35, 0x20, 0x21, 1, 0]);
            }
        }
        code.extend([0xFF, 0x15]);
        code.extend((0x12000 + slot).to_le_bytes());
    }
    code.extend([0xB8, 123, 0, 0, 0, 0xC3]);
    let mut image = xbe_com_imports(&code, &[190, 219, 211, 187]).unwrap();
    // A fixture original tem apenas 64 bytes raw de .rdata e uma página virtual.
    // Publicar os argumentos no arquivo exige estender raw_size, não só o Vec.
    image.resize(0x2300, 0);
    image[0x340..0x344].copy_from_slice(&0x1000u32.to_le_bytes());
    image[0x348..0x34C].copy_from_slice(&0x300u32.to_le_bytes());
    let path = b"\\Device\\CdRom0\\default.xbe";
    for (at, value) in [
        (0x2100, 0u32),
        (0x2104, 0x12110),
        (0x2108, 0x40),
        (0x2114, 0x12200),
    ] {
        image[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    image[0x2110..0x2112].copy_from_slice(&(path.len() as u16).to_le_bytes());
    image[0x2112..0x2114].copy_from_slice(&(path.len() as u16).to_le_bytes());
    image[0x2200..0x2200 + path.len()].copy_from_slice(path);
    std::fs::write(
        output.join("src/kernel_files.rs"),
        emit::rust_source(&analysis::analyze(&image).unwrap()),
    )
    .unwrap();
    std::fs::write(output.join("src/kernel_files.xbe"), image).unwrap();
    include_str!("../verification_files.inc").to_owned()
}
