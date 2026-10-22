use std::path::Path;
use xbox_formats::fixtures::xbe_com_imports;
use xbox_recomp::{analysis, emit};

// Ordem dos imports = índice do slot na thunk table (`0x12000 + 4 * índice`).
const IMPORTS: [u32; 8] = [
    44,  // HalGetInterruptVector
    107, // KeInitializeDpc
    109, // KeInitializeInterrupt
    98,  // KeConnectInterrupt
    119, // KeInsertQueueDpc
    205, // NtPulseEvent
    189, // NtCreateEvent
    233, // NtWaitForSingleObject
];
const HAL_VECTOR: u32 = 0;
const INIT_DPC: u32 = 1;
const INIT_INTERRUPT: u32 = 2;
const CONNECT: u32 = 3;
const INSERT_DPC: u32 = 4;
const PULSE: u32 = 5;
const CREATE_EVENT: u32 = 6;
const WAIT: u32 = 7;

// Memória guest do teste (a página `.rdata` da fixture é gravável inteira).
const KINT: u32 = 0x12800;
const DPC: u32 = 0x12900;
const ISR_COUNT: u32 = 0x12A00;
const DPC_COUNT: u32 = 0x12A04;
const ISR_CONTEXT: u32 = 0x12A08;
const DPC_CONTEXT: u32 = 0x12A0C;
const ISR_ARG0: u32 = 0x12A10;
const DPC_ARG0: u32 = 0x12A14;
const ISR_SEQ: u32 = 0x12A18;
const DPC_SEQ: u32 = 0x12A1C;
const SEQ: u32 = 0x12A30;
const IRQL_OUT: u32 = 0x12B00;
const EVENT: u32 = 0x12B10;
const WAIT_STATUS: u32 = 0x12B20;
const VECTOR: u32 = 0x12B24;
const CONNECT_STATUS: u32 = 0x12B28;

fn slot(index: u32) -> u32 {
    0x12000 + 4 * index
}

fn push_imm(c: &mut Vec<u8>, value: u32) {
    c.push(0x68);
    c.extend(value.to_le_bytes());
}
fn push_mem(c: &mut Vec<u8>, address: u32) {
    c.extend([0xFF, 0x35]);
    c.extend(address.to_le_bytes());
}
fn call(c: &mut Vec<u8>, index: u32) {
    c.extend([0xFF, 0x15]);
    c.extend(slot(index).to_le_bytes());
}
fn store_imm(c: &mut Vec<u8>, address: u32, value: u32) {
    c.extend([0xC7, 0x05]);
    c.extend(address.to_le_bytes());
    c.extend(value.to_le_bytes());
}
fn store_eax(c: &mut Vec<u8>, address: u32) {
    c.push(0xA3);
    c.extend(address.to_le_bytes());
}
fn load_eax(c: &mut Vec<u8>, address: u32) {
    c.push(0xA1);
    c.extend(address.to_le_bytes());
}
fn inc(c: &mut Vec<u8>, address: u32) {
    c.extend([0xFF, 0x05]);
    c.extend(address.to_le_bytes());
}
/// `mov eax, [esp + disp8]`.
fn load_arg(c: &mut Vec<u8>, disp: u8) {
    c.extend([0x8B, 0x44, 0x24, disp]);
}

/// ISR `stdcall(Interrupt*, ServiceContext)`: conta, desliga o INTA e enfileira o DPC.
fn isr() -> Vec<u8> {
    let mut c = Vec::new();
    inc(&mut c, ISR_COUNT);
    load_arg(&mut c, 4);
    store_eax(&mut c, ISR_ARG0);
    load_arg(&mut c, 8);
    store_eax(&mut c, ISR_CONTEXT);
    inc(&mut c, SEQ);
    load_eax(&mut c, SEQ);
    store_eax(&mut c, ISR_SEQ);
    store_imm(&mut c, 0xFD00_0140, 0); // PMC_INTR_EN_0 = desabilitado
    push_imm(&mut c, 0);
    push_imm(&mut c, 0);
    push_imm(&mut c, DPC);
    call(&mut c, INSERT_DPC);
    c.extend([0xB8, 1, 0, 0, 0]); // mov eax,1 (TRUE)
    c.extend([0xC2, 8, 0]); // ret 8
    c
}

/// DPC `stdcall(Dpc*, Context, Arg1, Arg2)`: conta, limpa o vblank, pulsa o evento e
/// religa o INTA.
fn dpc() -> Vec<u8> {
    let mut c = Vec::new();
    inc(&mut c, DPC_COUNT);
    load_arg(&mut c, 4);
    store_eax(&mut c, DPC_ARG0);
    load_arg(&mut c, 8);
    store_eax(&mut c, DPC_CONTEXT);
    inc(&mut c, SEQ);
    load_eax(&mut c, SEQ);
    store_eax(&mut c, DPC_SEQ);
    store_imm(&mut c, 0xFD60_0100, 1); // PCRTC_INTR: limpa o vblank
    push_imm(&mut c, 0);
    push_mem(&mut c, EVENT);
    call(&mut c, PULSE);
    store_imm(&mut c, 0xFD00_0140, 1); // PMC_INTR_EN_0 = hardware
    c.extend([0xC2, 16, 0]); // ret 16
    c
}

/// O `main`: instala o ISR e o DPC como o `pb_install_gpu_interrupt`, habilita o vblank e
/// espera um evento que só o DPC pulsa.
fn main_code(isr_address: u32, dpc_address: u32, arm: bool) -> Vec<u8> {
    let mut c = Vec::new();
    push_imm(&mut c, IRQL_OUT);
    push_imm(&mut c, 3);
    call(&mut c, HAL_VECTOR);
    store_eax(&mut c, VECTOR);
    push_imm(&mut c, 0xD1);
    push_imm(&mut c, dpc_address);
    push_imm(&mut c, DPC);
    call(&mut c, INIT_DPC);
    push_imm(&mut c, 1); // ShareVector
    push_imm(&mut c, 0); // LevelSensitive
    c.extend([0x0F, 0xB6, 0x05]); // movzx eax, byte [IRQL_OUT]
    c.extend(IRQL_OUT.to_le_bytes());
    c.push(0x50); // push eax
    push_mem(&mut c, VECTOR);
    push_imm(&mut c, 0x15);
    push_imm(&mut c, isr_address);
    push_imm(&mut c, KINT);
    call(&mut c, INIT_INTERRUPT);
    push_imm(&mut c, KINT);
    call(&mut c, CONNECT);
    store_eax(&mut c, CONNECT_STATUS);
    if arm {
        store_imm(&mut c, 0xFD60_0100, 1);
        store_imm(&mut c, 0xFD60_0140, 1);
        store_imm(&mut c, 0xFD00_0140, 1);
    }
    push_imm(&mut c, 0);
    push_imm(&mut c, 0);
    push_imm(&mut c, 0); // NotificationEvent
    push_imm(&mut c, EVENT);
    // NtCreateEvent(PHANDLE, POBJECT_ATTRIBUTES, EVENT_TYPE, InitialState): empilha ao
    // contrário. Os quatro `push` acima já estão na ordem certa: EVENT no topo.
    call(&mut c, CREATE_EVENT);
    push_imm(&mut c, 0);
    push_imm(&mut c, 0);
    push_mem(&mut c, EVENT);
    call(&mut c, WAIT);
    store_eax(&mut c, WAIT_STATUS);
    c.push(0xC3);
    c
}

fn publish(output: &Path, name: &str, main: impl Fn(u32, u32) -> Vec<u8>, isr: &[u8], dpc: &[u8]) {
    let base = 0x11000u32;
    let length = main(0, 0).len() as u32;
    let isr_address = base + length;
    let dpc_address = isr_address + isr.len() as u32;
    let mut code = main(isr_address, dpc_address);
    code.extend(isr);
    code.extend(dpc);
    let image = xbe_com_imports(&code, &IMPORTS).unwrap();
    let program = analysis::analyze_with_entries(&image, &[isr_address, dpc_address]).unwrap();
    std::fs::write(
        output.join(format!("src/{name}.rs")),
        emit::rust_source(&program),
    )
    .unwrap();
    std::fs::write(output.join(format!("src/{name}.xbe")), image).unwrap();
}

/// Programas x86 escritos à mão para o ciclo completo: vblank → ISR → DPC → pulso.
pub(super) fn sources(output: &Path) -> String {
    publish(
        output,
        "interrupt_ok",
        |i, d| main_code(i, d, true),
        &isr(),
        &dpc(),
    );
    // Sem habilitar o vblank, nenhuma interrupção pode sinalizar o evento.
    publish(
        output,
        "interrupt_sem_fonte",
        |i, d| main_code(i, d, false),
        &isr(),
        &dpc(),
    );
    // ISR que nunca retorna: `jmp $`.
    publish(
        output,
        "interrupt_presa",
        |i, d| main_code(i, d, true),
        &[0xEB, 0xFE],
        &dpc(),
    );
    include_str!("../verification_interrupt.inc").to_owned()
}
