use std::path::Path;
use xbox_formats::fixtures::xbe_com_imports;
use xbox_recomp::{analysis, emit};
fn store(code: &mut Vec<u8>, at: u32, value: u32) {
    code.extend([0xC7, 0x05]);
    code.extend(at.to_le_bytes());
    code.extend(value.to_le_bytes());
}
fn image(output: &Path, name: &str, tail: &[u8], forever: bool) {
    let mut code = vec![0xFA];
    store(&mut code, 0x12100, 1);
    code.extend(tail);
    store(&mut code, 0x12100, 3);
    if forever {
        code.extend([0xEB, 0xFE]);
    } else {
        code.push(0xC3);
    }
    code.resize(0x100, 0xCC);
    // ISR records progress at the exact recognition boundary.
    code.extend([
        0xA1, 0, 0x21, 1, 0, 0xA3, 4, 0x21, 1, 0, 0x89, 0x0D, 8, 0x21, 1, 0,
    ]);
    store(&mut code, 0xFD600100, 1);
    code.extend([0xC2, 8, 0]);
    let bytes = xbe_com_imports(&code, &[109, 98]).unwrap();
    let p = analysis::analyze_with_entries(&bytes, &[0x11100]).unwrap();
    assert!(
        p.blocks.values().any(|block| block.len() > 3),
        "probe must exercise interruption inside a block"
    );
    std::fs::write(output.join(format!("src/{name}.rs")), emit::rust_source(&p)).unwrap();
    std::fs::write(output.join(format!("src/{name}.xbe")), bytes).unwrap();
}
pub(super) fn sources(output: &Path) -> String {
    let mut sti = vec![0xFB];
    store(&mut sti, 0x12100, 2);
    image(output, "irq_sti", &sti, false);
    image(output, "irq_sti_loop", &sti, true);
    let mut twice = vec![0xFB, 0xFB];
    store(&mut twice, 0x12100, 2);
    image(output, "irq_sti_twice", &twice, false);
    image(output, "irq_cli", &[], false);
    image(output, "irq_sti_cli", &[0xFB, 0xFA], false);
    image(output, "irq_sti_rep", &[0xFB, 0xF3, 0xAA], false);
    include_str!("../verification_preemption.inc").to_owned()
}
