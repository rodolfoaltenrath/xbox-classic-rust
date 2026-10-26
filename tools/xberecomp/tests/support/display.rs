use std::path::Path;
use xbox_formats::fixtures::xbe_com_imports;
use xbox_recomp::{analysis, emit};
pub(super) fn sources(output: &Path) -> String {
    let mut code = Vec::new();
    for arg in [0x404u32, 4096, 0x3FFFFFF, 0x20000, 0x12C000] {
        push(&mut code, arg);
    }
    code.extend([0xFF, 0x15, 0, 0x20, 1, 0]);
    code.extend([
        0xC7, 0, 0xCC, 0x33, 0, 0xFF, 0xC7, 0x80, 0, 0x0A, 0, 0, 0, 0x55, 0xAA, 0xFF,
    ]); // dois pixels guest.
    code.extend([0x89, 0xC1, 0x81, 0xE1, 0xFF, 0xFF, 0xFF, 0x7F, 0x51]); // push PA.
    for arg in [2560, 0x12, 0x04010101, 0, 0xFD000000] {
        push(&mut code, arg);
    }
    code.extend([0xFF, 0x15, 4, 0x20, 1, 0]);
    for arg in [0, 0, 9, 0xFD000000] {
        push(&mut code, arg);
    }
    code.extend([0xFF, 0x15, 8, 0x20, 1, 0, 0xC3]);
    let image = xbe_com_imports(&code, &[166, 3, 2]).unwrap();
    let program = analysis::analyze(&image).unwrap();
    std::fs::write(
        output.join("src/kernel_display.rs"),
        emit::rust_source(&program),
    )
    .unwrap();
    std::fs::write(output.join("src/kernel_display.xbe"), image).unwrap();
    code.pop(); // Variante independente: programa DAC em x86 depois de habilitar vídeo.
    for index in [0u8, 0x33, 0x55, 0xAA, 0xCC] {
        for (address, value) in [
            (0xFD6813C8u32, index),
            (0xFD6813C9, 255 - index),
            (0xFD6813C9, index / 2),
            (0xFD6813C9, index ^ 0x55),
        ] {
            code.extend([0xC6, 0x05]);
            code.extend(address.to_le_bytes());
            code.push(value);
        }
    }
    code.push(0xC3);
    let image = xbe_com_imports(&code, &[166, 3, 2]).unwrap();
    std::fs::write(
        output.join("src/kernel_gamma.rs"),
        emit::rust_source(&analysis::analyze(&image).unwrap()),
    )
    .unwrap();
    std::fs::write(output.join("src/kernel_gamma.xbe"), image).unwrap();
    [
        include_str!("../verification_display.inc"),
        include_str!("../verification_gamma.inc"),
    ]
    .concat()
}
fn push(code: &mut Vec<u8>, arg: u32) {
    code.push(0x68);
    code.extend(arg.to_le_bytes());
}
