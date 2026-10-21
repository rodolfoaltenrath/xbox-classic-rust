use std::path::Path;
use xbox_formats::fixtures::xbe_com_imports;
use xbox_recomp::{analysis, emit};

pub(super) fn sources(output: &Path) -> String {
    let mut code = Vec::new();
    // Init, Enter, TryEnter recursivo, Leave, Leave, TryEnter livre, Leave.
    // Salva cada EAX na RAM para verificar BOOLEAN e limpeza stdcall pelo guest.
    for (i, slot) in [0u32, 1, 3, 2, 2, 3, 2].into_iter().enumerate() {
        code.push(0x68);
        code.extend(0x12100u32.to_le_bytes());
        code.extend([0xFF, 0x15]);
        code.extend((0x12000u32 + slot * 4).to_le_bytes());
        code.push(0xA3);
        code.extend((0x12200u32 + i as u32 * 4).to_le_bytes());
    }
    code.push(0xC3);
    let image = xbe_com_imports(&code, &[291, 277, 294, 306]).unwrap();
    let program = analysis::analyze(&image).unwrap();
    std::fs::write(
        output.join("src/kernel_critical.rs"),
        emit::rust_source(&program),
    )
    .unwrap();
    std::fs::write(output.join("src/kernel_critical.xbe"), image).unwrap();
    include_str!("../verification_critical.inc").to_owned()
}
