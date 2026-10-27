use std::path::Path;
use xbox_formats::fixtures::xbe_com_imports;
use xbox_recomp::{analysis, emit};
fn push(code: &mut Vec<u8>, v: u32) {
    code.push(0x68);
    code.extend(v.to_le_bytes());
}
fn call(code: &mut Vec<u8>, i: u32) {
    code.extend([0xFF, 0x15]);
    code.extend((0x12000 + i * 4).to_le_bytes());
}
fn store(code: &mut Vec<u8>, at: u32, v: u32) {
    code.extend([0xC7, 0x05]);
    code.extend(at.to_le_bytes());
    code.extend(v.to_le_bytes());
}
fn push_handle(code: &mut Vec<u8>) {
    code.extend([0xFF, 0x35, 0, 0x21, 1, 0]);
}
pub(super) fn sources(out: &Path) -> String {
    let mut code = Vec::new();
    for v in [1, 0, 0, 0x12100] {
        push(&mut code, v);
    }
    call(&mut code, 0);
    for v in [0x11200, 0, 0, 0, 0x11200, 0, 0, 65536, 0, 0x12104] {
        push(&mut code, v);
    }
    call(&mut code, 1);
    push(&mut code, 0);
    push(&mut code, 0);
    push_handle(&mut code);
    call(&mut code, 3);
    store(&mut code, 0x12114, 1);
    push_handle(&mut code);
    call(&mut code, 5);
    code.extend([0x31, 0xC0, 0xC3]);
    code.resize(0x200, 0xCC);
    for v in [0x12200, 0, 1] {
        push(&mut code, v);
    }
    call(&mut code, 2);
    push(&mut code, 0x12108);
    push(&mut code, 1);
    push_handle(&mut code);
    call(&mut code, 4);
    store(&mut code, 0x12110, 1);
    code.extend([0xB8, 42, 0, 0, 0, 0xC2, 8, 0]);
    let image = xbe_com_imports(&code, &[193, 255, 99, 233, 222, 187]).unwrap();
    let p = analysis::analyze(&image).unwrap();
    assert!(p.blocks.contains_key(&0x11200));
    std::fs::write(out.join("src/kernel_semaphores.rs"), emit::rust_source(&p)).unwrap();
    std::fs::write(out.join("src/kernel_semaphores.xbe"), image).unwrap();
    include_str!("../verification_semaphores.inc").to_owned()
}
