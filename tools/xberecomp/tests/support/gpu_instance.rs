use std::path::Path;
use xbox_formats::fixtures::xbe_com_imports;
use xbox_recomp::{analysis, emit};
pub(super) fn sources(out: &Path) -> String {
    let mut code = Vec::new();
    query(&mut code);
    code.extend([0xA3, 0x10, 0x21, 1, 0]);
    code.extend([0xC7, 0x05, 0, 0xB0, 0xFE, 0x83, 0xBE, 0xBA, 0xFE, 0xCA]);
    query(&mut code);
    code.push(0xC3);
    let image = xbe_com_imports(&code, &[168]).unwrap();
    let p = analysis::analyze(&image).unwrap();
    std::fs::write(out.join("src/gpu_instance.rs"), emit::rust_source(&p)).unwrap();
    std::fs::write(out.join("src/gpu_instance.xbe"), image).unwrap();
    include_str!("../verification_gpu_instance.inc").to_owned()
}
fn query(code: &mut Vec<u8>) {
    code.extend([0x68, 0, 0x21, 1, 0, 0x6A, 0xFF, 0xFF, 0x15, 0, 0x20, 1, 0]);
}
