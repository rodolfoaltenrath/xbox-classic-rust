use std::path::Path;
use xbox_formats::fixtures::xbe_com_imports;
use xbox_recomp::{analysis, emit};

// Imports (índice do slot em 0x12000 + 4 * índice).
const YIELD: u32 = 0;
const RESUME: u32 = 1;
const SUSPEND: u32 = 2;
const CREATE: u32 = 3;
const WAIT: u32 = 4;
const IMPORTS: [u32; 5] = [238, 224, 231, 255, 233];

const THREAD_HANDLE: u32 = 0x12104;
const CHILD: u32 = 0x11200;

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
fn inc(code: &mut Vec<u8>, at: u32) {
    code.extend([0xFF, 0x05]);
    code.extend(at.to_le_bytes());
}

/// `PsCreateSystemThreadEx(handle→0x12104, ..., CreateSuspended, ...)`: argumentos
/// empilhados do último para o primeiro, como o Rust gerado do semáforo faz.
fn create(code: &mut Vec<u8>, suspended: u32) {
    for value in [CHILD, 0, suspended, 0, CHILD, 0, 0, 65536, 0, THREAD_HANDLE] {
        push(code, value);
    }
    call(code, CREATE);
}

/// `NtResumeThread(handle, &previous)` / `NtSuspendThread(handle, &previous)`.
fn control(code: &mut Vec<u8>, slot: u32, previous: u32) {
    push(code, previous);
    push_mem(code, THREAD_HANDLE);
    call(code, slot);
}

/// A filha: conta que rodou, suspende a si mesma e, retomada, marca e retorna 42.
fn child(code: &mut Vec<u8>) {
    code.resize(0x200, 0xCC);
    inc(code, 0x12120); // rodou uma vez
    control(code, SUSPEND, 0x1210C);
    store_eax(code, 0x12134); // status da autossuspensão (devolvido ao ser retomada)
    inc(code, 0x12128); // continuou depois de retomada
    code.extend([0xB8, 42, 0, 0, 0, 0xC2, 8, 0]); // mov eax,42 ; ret 8
}

fn publish(out: &Path, name: &str, code: &[u8]) {
    let image = xbe_com_imports(code, &IMPORTS).unwrap();
    let program = analysis::analyze(&image).unwrap();
    assert!(program.blocks.contains_key(&CHILD));
    std::fs::write(
        out.join(format!("src/{name}.rs")),
        emit::rust_source(&program),
    )
    .unwrap();
    std::fs::write(out.join(format!("src/{name}.xbe")), image).unwrap();
}

/// Programas x86 escritos à mão para suspensão, retomada e cessão da CPU (ADR-0098).
pub(super) fn sources(out: &Path) -> String {
    // Pai: cria suspensa, cede (ninguém para ceder), retoma (a filha roda e se suspende),
    // observa, cede de novo (filha suspensa), retoma, cede (agora cede de verdade), espera.
    let mut parent = Vec::new();
    create(&mut parent, 1);
    call(&mut parent, YIELD);
    store_eax(&mut parent, 0x12110); // NO_YIELD: a filha nunca iniciou
    control(&mut parent, RESUME, 0x12108); // anterior = 1; a filha inicia e se suspende
    store_eax(&mut parent, 0x12114);
    load_eax(&mut parent, 0x12120);
    store_eax(&mut parent, 0x12118); // a filha rodou exatamente uma vez
    call(&mut parent, YIELD);
    store_eax(&mut parent, 0x1211C); // NO_YIELD: a filha está suspensa
    control(&mut parent, RESUME, 0x12124); // anterior = 1; a filha fica elegível
    call(&mut parent, YIELD);
    store_eax(&mut parent, 0x1212C); // SUCCESS: cedeu para a filha
    push(&mut parent, 0);
    push(&mut parent, 0);
    push_mem(&mut parent, THREAD_HANDLE);
    call(&mut parent, WAIT);
    store_eax(&mut parent, 0x12130);
    parent.extend([0x31, 0xC0, 0xC3]);
    child(&mut parent);
    publish(out, "kernel_thread_control", &parent);

    // Pai espera a filha, que se suspende: nada pode rodar, e isso é erro, não laço.
    let mut deadlock = Vec::new();
    create(&mut deadlock, 0);
    push(&mut deadlock, 0);
    push(&mut deadlock, 0);
    push_mem(&mut deadlock, THREAD_HANDLE);
    call(&mut deadlock, WAIT);
    deadlock.extend([0x31, 0xC0, 0xC3]);
    child(&mut deadlock);
    publish(out, "kernel_thread_deadlock", &deadlock);
    include_str!("../verification_thread_control.inc").to_owned()
}
