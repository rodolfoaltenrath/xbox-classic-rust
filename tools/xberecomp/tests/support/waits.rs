use std::path::Path;
use xbox_formats::fixtures::xbe_com_imports;
use xbox_recomp::{analysis, emit};

// Imports (índice do slot em 0x12000 + 4 * índice).
const CREATE_EVENT: u32 = 0;
const WAIT_EX: u32 = 1;
const WAIT_MULTIPLE: u32 = 2;
const CREATE_THREAD: u32 = 3;
const DELAY: u32 = 4;
const SET_EVENT: u32 = 5;
const IMPORTS: [u32; 6] = [189, 234, 235, 255, 99, 225];

const E0: u32 = 0x12100; // evento de sincronização, nunca sinalizado
const E1: u32 = 0x12104; // evento de notificação, sinalizado pela filha
const THREAD: u32 = 0x12108;
const LIST: u32 = 0x121A0;
const CHILD: u32 = 0x11300;

fn push(code: &mut Vec<u8>, value: u32) {
    code.push(0x68);
    code.extend(value.to_le_bytes());
}
fn push_mem(code: &mut Vec<u8>, at: u32) {
    code.extend([0xFF, 0x35]);
    code.extend(at.to_le_bytes());
}
fn call(code: &mut Vec<u8>, slot: u32) {
    code.extend([0xFF, 0x15]);
    code.extend((0x12000 + slot * 4).to_le_bytes());
}
fn store_eax(code: &mut Vec<u8>, at: u32) {
    code.push(0xA3);
    code.extend(at.to_le_bytes());
}
fn load_eax(code: &mut Vec<u8>, at: u32) {
    code.push(0xA1);
    code.extend(at.to_le_bytes());
}
/// `mov dword [at], value`.
fn store(code: &mut Vec<u8>, at: u32, value: u32) {
    code.extend([0xC7, 0x05]);
    code.extend(at.to_le_bytes());
    code.extend(value.to_le_bytes());
}
/// Grava um `LARGE_INTEGER` negativo (intervalo relativo em unidades de 100 ns).
fn interval(code: &mut Vec<u8>, at: u32, hundred_ns: i64) {
    let value = hundred_ns.cast_unsigned();
    store(code, at, value as u32);
    store(code, at + 4, (value >> 32) as u32);
}
fn create_event(code: &mut Vec<u8>, output: u32, kind: u32) {
    for value in [0, kind, 0, output] {
        push(code, value);
    }
    call(code, CREATE_EVENT);
}

/// Programa x86 escrito à mão: esperas com timeout e por vários objetos no scheduler.
pub(super) fn sources(out: &Path) -> String {
    let mut c = Vec::new();
    create_event(&mut c, E0, 1);
    create_event(&mut c, E1, 0);
    interval(&mut c, 0x12150, -20_000); // 2 ms
    interval(&mut c, 0x12160, -10_000); // 1 ms (a filha dorme)
    interval(&mut c, 0x12170, -1_000_000); // 100 ms
    // Espera 1: ninguém sinaliza E0; vence o timeout de 2 ms.
    for value in [0x12150, 0, 1] {
        push(&mut c, value);
    }
    push_mem(&mut c, E0);
    call(&mut c, WAIT_EX);
    store_eax(&mut c, 0x12190);
    // A filha dorme 1 ms e sinaliza E1.
    for value in [CHILD, 0, 0, 0, CHILD, 0, 0, 65536, 0, THREAD] {
        push(&mut c, value);
    }
    call(&mut c, CREATE_THREAD);
    load_eax_from(&mut c, E0, LIST);
    load_eax_from(&mut c, E1, LIST + 4);
    // Espera 2: qualquer de [E0, E1], 100 ms; a filha sinaliza E1 (índice 1) antes.
    for value in [0x12170, 0, 1, 1, LIST, 2] {
        push(&mut c, value);
    }
    call(&mut c, WAIT_MULTIPLE);
    store_eax(&mut c, 0x12194);
    // Espera 3: todos de [E0, E1] sem esperar (timeout zero): E0 nunca sinalizou.
    for value in [0x12180, 0, 1, 0, LIST, 2] {
        push(&mut c, value);
    }
    call(&mut c, WAIT_MULTIPLE);
    store_eax(&mut c, 0x12198);
    c.extend([0x31, 0xC0, 0xC3]);

    c.resize(0x300, 0xCC);
    for value in [0x12160, 0, 1] {
        push(&mut c, value);
    }
    call(&mut c, DELAY);
    push(&mut c, 0);
    push_mem(&mut c, E1);
    call(&mut c, SET_EVENT);
    c.extend([0xB8, 42, 0, 0, 0, 0xC2, 8, 0]);

    let image = xbe_com_imports(&c, &IMPORTS).unwrap();
    let program = analysis::analyze(&image).unwrap();
    assert!(program.blocks.contains_key(&CHILD));
    std::fs::write(out.join("src/kernel_waits.rs"), emit::rust_source(&program)).unwrap();
    std::fs::write(out.join("src/kernel_waits.xbe"), image).unwrap();
    include_str!("../verification_waits.inc").to_owned()
}

/// `mov eax, [from]` ; `mov [to], eax`.
fn load_eax_from(code: &mut Vec<u8>, from: u32, to: u32) {
    load_eax(code, from);
    store_eax(code, to);
}
