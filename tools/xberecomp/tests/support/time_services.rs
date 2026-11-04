use std::path::Path;
use xbox_formats::fixtures::xbe_com_imports;
use xbox_recomp::{analysis, emit};

// Imports (índice do slot em 0x12000 + 4 * índice).
const COUNTER: u32 = 0;
const FREQUENCY: u32 = 1;
const FIELDS_TO_TIME: u32 = 2;
const TIME_TO_FIELDS: u32 = 3;
const IMPORTS: [u32; 4] = [126, 127, 304, 305];

const ENTRADA: u32 = 0x12600; // TIME_FIELDS de entrada
const TEMPO: u32 = 0x12620; // LARGE_INTEGER
const SAIDA: u32 = 0x12640; // TIME_FIELDS de saída

fn push(code: &mut Vec<u8>, value: u32) {
    code.push(0x68);
    code.extend(value.to_le_bytes());
}
fn call(code: &mut Vec<u8>, slot: u32) {
    code.extend([0xFF, 0x15]);
    code.extend((0x12000 + slot * 4).to_le_bytes());
}
fn store(code: &mut Vec<u8>, at: u32, value: u32) {
    code.extend([0xC7, 0x05]);
    code.extend(at.to_le_bytes());
    code.extend(value.to_le_bytes());
}
fn store_eax(code: &mut Vec<u8>, at: u32) {
    code.push(0xA3);
    code.extend(at.to_le_bytes());
}
fn store_edx(code: &mut Vec<u8>, at: u32) {
    code.extend([0x89, 0x15]);
    code.extend(at.to_le_bytes());
}

/// Programa x86 escrito à mão: contador e frequência de performance (64 bits em EDX:EAX) e
/// a ida e volta de `TIME_FIELDS` (ADR-0101).
pub(super) fn sources(out: &Path) -> String {
    let mut c = Vec::new();
    call(&mut c, FREQUENCY);
    store_eax(&mut c, 0x12500);
    store_edx(&mut c, 0x12504);
    call(&mut c, COUNTER);
    store_eax(&mut c, 0x12508);
    store_edx(&mut c, 0x1250C);
    // 2000-03-01 12:34:56.789 (o dia da semana de entrada é ignorado).
    store(&mut c, ENTRADA, 0x0003_07D0);
    store(&mut c, ENTRADA + 4, 0x000C_0001);
    store(&mut c, ENTRADA + 8, 0x0038_0022);
    store(&mut c, ENTRADA + 12, 0x0000_0315);
    push(&mut c, TEMPO);
    push(&mut c, ENTRADA);
    call(&mut c, FIELDS_TO_TIME);
    store_eax(&mut c, 0x12510);
    push(&mut c, SAIDA);
    push(&mut c, TEMPO);
    call(&mut c, TIME_TO_FIELDS);
    c.extend([0x31, 0xC0, 0xC3]);

    let image = xbe_com_imports(&c, &IMPORTS).unwrap();
    let program = analysis::analyze(&image).unwrap();
    std::fs::write(
        out.join("src/kernel_time_services.rs"),
        emit::rust_source(&program),
    )
    .unwrap();
    std::fs::write(out.join("src/kernel_time_services.xbe"), image).unwrap();
    include_str!("../verification_time_services.inc").to_owned()
}
