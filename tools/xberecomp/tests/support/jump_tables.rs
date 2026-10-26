use std::path::Path;
use xbox_formats::fixtures::xbe_com_codigo;
use xbox_recomp::{analysis, emit};
fn write(output: &Path, name: &str, bytes: Vec<u8>) {
    let program = analysis::analyze(&bytes).unwrap();
    for at in [0x11040, 0x11050] {
        assert!(program.blocks.contains_key(&at));
    }
    std::fs::write(
        output.join(format!("src/{name}.rs")),
        emit::rust_source(&program),
    )
    .unwrap();
    std::fs::write(output.join(format!("src/{name}.xbe")), bytes).unwrap();
}
pub(super) fn sources(output: &Path) -> String {
    let mut code = vec![
        0x83, 0xF8, 1, 0x77, 0x2B, 0xFF, 0x24, 0x85, 0x20, 0x20, 1, 0,
    ];
    code.resize(0x60, 0xCC);
    for (at, value) in [(0x30, 99u32), (0x40, 11), (0x50, 22)] {
        code[at] = 0xB8;
        code[at + 1..at + 5].copy_from_slice(&value.to_le_bytes());
        code[at + 5] = 0xC3;
    }
    let mut bytes = xbe_com_codigo(&code).unwrap();
    bytes[0x338..0x33C].copy_from_slice(&0u32.to_le_bytes());
    bytes[0x2020..0x2024].copy_from_slice(&0x11040u32.to_le_bytes());
    bytes[0x2024..0x2028].copy_from_slice(&0x11050u32.to_le_bytes());
    write(output, "jump_table_guest", bytes);
    let mut code = vec![
        0x8D, 0x05, 0x40, 0x10, 1, 0, 0xA3, 0x20, 0x20, 1, 0, 0x8D, 0x05, 0x50, 0x10, 1, 0, 0xA3,
        0x20, 0x20, 1, 0, 0xFF, 0x15, 0x20, 0x20, 1, 0, 0xC3,
    ];
    code.resize(0x70, 0xCC);
    for (at, value) in [(0x40, 11u32), (0x50, 22)] {
        code[at] = 0xB8;
        code[at + 1..at + 5].copy_from_slice(&value.to_le_bytes());
        code[at + 5] = 0xC3;
    }
    code[0x60] = 0xC3;
    write(
        output,
        "callback_flow_guest",
        xbe_com_codigo(&code).unwrap(),
    );
    let mut sse = code[..22].to_vec(); // Dois escapes, mantendo a fixture anterior.
    sse.extend([0xB9, 0x20, 0x20, 1, 0, 0x83, 0xEC, 4, 0x8B, 0x01]);
    sse.extend([
        0xF3, 0x0F, 0x10, 0x05, 0x24, 0x20, 1, 0, 0xF3, 0x0F, 0x11, 0x04, 0x24,
    ]);
    sse.extend([0xFF, 0xD0, 0x83, 0xC4, 4, 0xC3]);
    sse.resize(0x70, 0xCC);
    sse[0x40..].copy_from_slice(&code[0x40..]);
    write(output, "callback_sse_guest", xbe_com_codigo(&sse).unwrap());
    include_str!("../verification_jump_tables.inc").to_owned()
}
