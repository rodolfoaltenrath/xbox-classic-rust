use std::path::Path;
use xbox_formats::fixtures::xbe_com_imports;
use xbox_recomp::{analysis, emit};

// Imports (índice do slot em 0x12000 + 4 * índice).
const AV_SET: u32 = 0;
const AV_GET: u32 = 1;
const DBG_PRINT: u32 = 2;
const REGISTER: u32 = 3;
const RETURN_TO_FIRMWARE: u32 = 4;
const IMPORTS: [u32; 5] = [4, 1, 8, 47, 49];

const REG_BAIXA: u32 = 0x12400; // prioridade 1
const REG_ALTA: u32 = 0x12420; // prioridade 5
const FORMATO: u32 = 0x12600;
const SEQUENCIA: u32 = 0x12700;
const ROTINA_BAIXA: u32 = 0x11300;
const ROTINA_ALTA: u32 = 0x11340;

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
fn load_eax(code: &mut Vec<u8>, at: u32) {
    code.push(0xA1);
    code.extend(at.to_le_bytes());
}
fn inc(code: &mut Vec<u8>, at: u32) {
    code.extend([0xFF, 0x05]);
    code.extend(at.to_le_bytes());
}

/// Notificação de desligamento `VOID NTAPI f(PHAL_SHUTDOWN_REGISTRATION)`: anota a ordem em
/// que foi chamada (1 = primeira) no endereço `slot`.
fn notification(code: &mut Vec<u8>, slot: u32) {
    inc(code, SEQUENCIA);
    load_eax(code, SEQUENCIA);
    store_eax(code, slot);
    code.extend([0xC2, 4, 0]); // ret 4
}

/// Programa x86 escrito à mão: `AvSet/GetSavedDataAddress`, `DbgPrint` e
/// `HalReturnToFirmware` com duas notificações de desligamento (ADR-0100).
pub(super) fn sources(out: &Path) -> String {
    let mut c = Vec::new();
    push(&mut c, 0x1234);
    call(&mut c, AV_SET);
    call(&mut c, AV_GET);
    store_eax(&mut c, 0x12510);
    store(&mut c, REG_BAIXA, ROTINA_BAIXA);
    store(&mut c, REG_BAIXA + 4, 1);
    store(&mut c, REG_ALTA, ROTINA_ALTA);
    store(&mut c, REG_ALTA + 4, 5);
    for registration in [REG_BAIXA, REG_ALTA] {
        push(&mut c, 1); // Register = TRUE
        push(&mut c, registration);
        call(&mut c, REGISTER);
    }
    // DbgPrint("n=%d", 7) é cdecl: quem chama limpa os dois argumentos.
    store(&mut c, FORMATO, 0x6425_3D6E); // "n=%d"
    store(&mut c, FORMATO + 4, 0);
    push(&mut c, 7);
    push(&mut c, FORMATO);
    call(&mut c, DBG_PRINT);
    c.extend([0x83, 0xC4, 8]); // add esp, 8
    store_eax(&mut c, 0x12514);
    push(&mut c, 1); // HalRebootRoutine
    call(&mut c, RETURN_TO_FIRMWARE);
    inc(&mut c, 0x12518); // nunca deve rodar: a chamada não retorna
    c.extend([0x31, 0xC0, 0xC3]);

    c.resize(0x300, 0xCC);
    notification(&mut c, 0x12500); // a de prioridade baixa
    c.resize(0x340, 0xCC);
    notification(&mut c, 0x12504); // a de prioridade alta

    let image = xbe_com_imports(&c, &IMPORTS).unwrap();
    let program = analysis::analyze_with_entries(&image, &[ROTINA_BAIXA, ROTINA_ALTA]).unwrap();
    std::fs::write(
        out.join("src/kernel_firmware.rs"),
        emit::rust_source(&program),
    )
    .unwrap();
    std::fs::write(out.join("src/kernel_firmware.xbe"), image).unwrap();
    include_str!("../verification_firmware.inc").to_owned()
}
