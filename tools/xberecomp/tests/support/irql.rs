use std::path::Path;
use xbox_formats::fixtures::xbe_com_imports;
use xbox_recomp::{analysis, emit};

pub(super) fn sources(output: &Path) -> String {
    let mut code = Vec::new();
    for output in [0x12100u32, 0x12104] {
        code.extend([0xFF, 0x15, 0, 0x20, 1, 0]);
        code.push(0xA3);
        code.extend(output.to_le_bytes());
        code.push(0xFA); // CLI: muda IF sem mudar IRQL.
    }
    code.extend([0xFF, 0x25, 0, 0x20, 1, 0]); // tail-call stdcall/0.
    let image = xbe_com_imports(&code, &[103]).unwrap();
    let program = analysis::analyze(&image).unwrap();
    std::fs::write(
        output.join("src/kernel_irql.rs"),
        emit::rust_source(&program),
    )
    .unwrap();
    std::fs::write(output.join("src/kernel_irql.xbe"), image).unwrap();
    include_str!("../verification_irql.inc").to_owned()
}
