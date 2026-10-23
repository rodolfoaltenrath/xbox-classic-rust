use std::path::Path;
use xbox_formats::fixtures::xbe_com_imports;
use xbox_recomp::{analysis, emit};
fn store(code: &mut Vec<u8>, at: u32, value: u32) {
    code.extend([0xC7, 0x05]);
    code.extend(at.to_le_bytes());
    code.extend(value.to_le_bytes());
}
fn push(code: &mut Vec<u8>, value: u32) {
    code.push(0x68);
    code.extend(value.to_le_bytes());
}
fn call(code: &mut Vec<u8>, index: u32) {
    code.extend([0xFF, 0x15]);
    code.extend((0x12000 + index * 4).to_le_bytes());
}
fn scalar(code: &mut Vec<u8>, at: u32, load: bool) {
    code.extend([0xF3, 0x0F, if load { 0x10 } else { 0x11 }, 0x05]);
    code.extend(at.to_le_bytes());
}
#[derive(Clone, Copy)]
enum Scenario {
    Sleep,
    Deadlines,
    Quantum,
    Detach,
    CompetingWaits,
}
fn image(output: &Path, name: &str, scenario: Scenario) {
    let parent_sleep = matches!(scenario, Scenario::Deadlines);
    let counter = matches!(scenario, Scenario::Quantum);
    let detach = matches!(scenario, Scenario::Detach);
    let two_waits = matches!(scenario, Scenario::CompetingWaits);
    let mut code = Vec::new();
    if two_waits {
        for value in [0, 0, 0, 0x12048] {
            push(&mut code, value);
        }
        call(&mut code, 5);
    }

    for (at, value) in [
        (0x12200, (-20000i32).cast_unsigned()),
        (0x12204, u32::MAX),
        (0x12208, (-40000i32).cast_unsigned()),
        (0x1220C, u32::MAX),
        (0x12220, 2.5f32.to_bits()),
        (0x12224, 3.5f32.to_bits()),
    ] {
        store(&mut code, at, value);
    }
    code.push(0xBB);
    code.extend(0xAAAAu32.to_le_bytes());
    scalar(&mut code, 0x12220, true);
    code.extend([0xD9, 0xEE]);
    for value in [0x11200, 0, 0, 0, 0x11200, 0x12044, 0, 65536, 0, 0x12040] {
        push(&mut code, value);
    }
    call(&mut code, 0);
    store(&mut code, 0x12104, 1);
    code.extend([0xA1, 0, 0x21, 1, 0, 0xA3, 0x28, 0x21, 1, 0]);
    if parent_sleep {
        for value in [0x12208, 0, 1] {
            push(&mut code, value);
        }
        call(&mut code, 1);
    }
    if detach {
        code.extend([0xFF, 0x35, 0x40, 0x20, 1, 0]);
        call(&mut code, 4);
        code.extend([0xA3, 0x2C, 0x21, 1, 0]);
    } else {
        push(&mut code, 0);
        push(&mut code, 0);
        code.extend([0xFF, 0x35, 0x40, 0x20, 1, 0]);
        call(&mut code, 3);
    }
    code.extend([0x89, 0x1D, 0x18, 0x21, 1, 0]);
    scalar(&mut code, 0x12120, false);
    code.extend([0xD9, 0x1D, 0x24, 0x21, 1, 0, 0x31, 0xC0, 0xC3]);
    code.resize(0x200, 0xCC);
    if two_waits {
        store(&mut code, 0x12100, 1);
        push(&mut code, 0);
        push(&mut code, 0);
        code.extend([0xFF, 0x35, 0x48, 0x20, 1, 0]);
        call(&mut code, 3);
        store(&mut code, 0x12100, 2);
    } else if counter {
        code.push(0xB9);
        code.extend(10000u32.to_le_bytes());
        code.extend([0xFF, 0x05, 0, 0x21, 1, 0, 0x49, 0x75, 0xF7]);
    } else {
        code.push(0xBB);
        code.extend(0xBBBBu32.to_le_bytes());
        scalar(&mut code, 0x12224, true);
        code.extend([0xD9, 0xE8]);
        store(&mut code, 0x12100, 1);
        for value in [0x12200, 0, 1] {
            push(&mut code, value);
        }
        call(&mut code, 1);
        store(&mut code, 0x12100, 2);
        code.extend([0x89, 0x1D, 0x08, 0x21, 1, 0]);
        scalar(&mut code, 0x1210C, false);
        code.extend([0xD9, 0x1D, 0x10, 0x21, 1, 0]);
    }
    code.push(0xB8);
    code.extend(42u32.to_le_bytes());
    code.extend([0xC2, 8, 0]);
    let bytes = xbe_com_imports(&code, &[255, 99, 258, 233, 187, 189]).unwrap();
    let program = analysis::analyze(&bytes).unwrap();
    assert!(program.blocks.contains_key(&0x11200));
    std::fs::write(
        output.join(format!("src/{name}.rs")),
        emit::rust_source(&program),
    )
    .unwrap();
    std::fs::write(output.join(format!("src/{name}.xbe")), bytes).unwrap();
}
pub(super) fn sources(output: &Path) -> String {
    image(output, "scheduler_sleep", Scenario::Sleep);
    image(output, "scheduler_deadlines", Scenario::Deadlines);
    image(output, "scheduler_quantum", Scenario::Quantum);
    image(output, "scheduler_detach", Scenario::Detach);
    image(output, "scheduler_waits", Scenario::CompetingWaits);
    include_str!("../verification_scheduler.inc").to_owned()
}
