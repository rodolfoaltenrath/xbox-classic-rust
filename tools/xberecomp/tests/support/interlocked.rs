use std::path::Path;
use xbox_formats::fixtures::xbe_com_imports;
use xbox_recomp::{analysis, emit};

pub(super) fn sources(output: &Path) -> String {
    let mut code = Vec::new();
    for (value, result) in [(1u32, 0x12200u32), (0xAABBCCDD, 0x12204)] {
        arguments(&mut code, value);
        code.extend([0xFF, 0x15, 0, 0x20, 1, 0]);
        code.push(0xA3);
        code.extend(result.to_le_bytes());
    }
    arguments(&mut code, 0xCAFEBABE);
    code.extend([0xFF, 0x25, 0, 0x20, 1, 0]); // tail-call de kernel; retorno direto ao host.
    let image = xbe_com_imports(&code, &[54]).unwrap();
    let program = analysis::analyze(&image).unwrap();
    std::fs::write(
        output.join("src/kernel_interlocked.rs"),
        emit::rust_source(&program),
    )
    .unwrap();
    std::fs::write(output.join("src/kernel_interlocked.xbe"), image).unwrap();
    include_str!("../verification_interlocked.inc").to_owned()
}

fn arguments(code: &mut Vec<u8>, value: u32) {
    code.push(0xB9);
    code.extend(0x12100u32.to_le_bytes());
    code.push(0xBA);
    code.extend(value.to_le_bytes());
}
