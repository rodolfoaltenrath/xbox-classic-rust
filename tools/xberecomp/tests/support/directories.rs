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
                Some(0x100001),
                Some(0x12100),
                Some(0x12130),
                Some(1),
                Some(0x21),
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
                Some(128),
                Some(1),
                Some(0x12140),
                Some(1),
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
                Some(128),
                Some(1),
                Some(0xdeadbeef),
                Some(0),
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
                Some(128),
                Some(1),
                Some(0xdeadbeef),
                Some(1),
            ],
        ),
        (8, vec![None]),
    ] {
        for arg in args.into_iter().rev() {
            if let Some(arg) = arg {
                code.push(0x68);
                code.extend(u32::to_le_bytes(arg));
            } else {
                code.extend([0xff, 0x35, 0x20, 0x21, 1, 0]);
            }
        }
        code.extend([0xff, 0x15]);
        code.extend((0x12000 + slot).to_le_bytes());
    }
    code.extend([0xb8, 123, 0, 0, 0, 0xc3]);
    let mut image = xbe_com_imports(&code, &[202, 207, 187]).unwrap();
    image.resize(0x2300, 0);
    image[0x340..0x344].copy_from_slice(&0x1000u32.to_le_bytes());
    image[0x348..0x34c].copy_from_slice(&0x300u32.to_le_bytes());
    let path = b"\\Device\\CdRom0\\";
    for (at, v) in [
        (0x2100, 0u32),
        (0x2104, 0x12110),
        (0x2108, 0x40),
        (0x2114, 0x12200),
        (0x2144, 0x12240),
    ] {
        image[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    image[0x2110..0x2112].copy_from_slice(&(path.len() as u16).to_le_bytes());
    image[0x2112..0x2114].copy_from_slice(&(path.len() as u16).to_le_bytes());
    image[0x2200..0x2200 + path.len()].copy_from_slice(path);
    image[0x2140..0x2144].copy_from_slice(&[5, 0, 5, 0]);
    image[0x2240..0x2245].copy_from_slice(b"*.xbe");
    std::fs::write(
        output.join("src/kernel_directories.rs"),
        emit::rust_source(&analysis::analyze(&image).unwrap()),
    )
    .unwrap();
    std::fs::write(output.join("src/kernel_directories.xbe"), image).unwrap();
    include_str!("../verification_directories.inc").to_owned()
}
