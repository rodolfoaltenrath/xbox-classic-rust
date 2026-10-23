use std::path::Path;
use xbox_formats::fixtures::xbe_com_imports;
use xbox_recomp::{analysis, emit};
pub(super) fn sources(output: &Path) -> String {
    let mut code = vec![0xBA, 4, 0x25, 0, 0xFD, 0xB8, 1, 0, 0, 0, 0x89, 0x02];
    for (address, value) in [
        (0xFD003204u32, 0x100u32),
        (0xFD00322C, 0x111C),
        (0xFD003244, 4),
        (0xFD800040, 0x18C000),
    ] {
        code.extend([0xC7, 0x05]);
        code.extend(address.to_le_bytes());
        code.extend(value.to_le_bytes());
    }
    code.extend([0xFF, 0x15, 0, 0x20, 1, 0]);
    for (address, output) in [
        (0xFD003240u32, 0x12100u32),
        (0xFD800044, 0x12104),
        (0xFD00322C, 0x12108),
    ] {
        code.push(0xA1);
        code.extend(address.to_le_bytes());
        code.push(0xA3);
        code.extend(output.to_le_bytes());
    }
    code.push(0xC3);
    let image = xbe_com_imports(&code, &[1]).unwrap();
    let program = analysis::analyze(&image).unwrap();
    std::fs::write(
        output.join("src/pfifo_guest.rs"),
        emit::rust_source(&program),
    )
    .unwrap();
    std::fs::write(output.join("src/pfifo_guest.xbe"), image).unwrap();
    include_str!("../verification_pfifo.inc").to_owned()
}
