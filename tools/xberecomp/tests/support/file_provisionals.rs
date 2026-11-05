use std::path::Path;
use xbox_formats::fixtures::xbe_com_imports;
use xbox_recomp::{analysis, emit};

// Imports (índice do slot em 0x12000 + 4 * índice).
const CREATE: u32 = 0;
const QUERY_FULL: u32 = 1;
const SET_INFO: u32 = 2;
const WRITE: u32 = 3;
const IMPORTS: [u32; 4] = [190, 210, 226, 236];

const ATTR: u32 = 0x12100;
const STR: u32 = 0x12120;
const NAME: u32 = 0x12200;
const HANDLE: u32 = 0x12140;
const IO: u32 = 0x12150;
const INFO: u32 = 0x12300;
const FULL: u32 = 0x12400;
const POSICAO: u32 = 0x12380;
const NOME: &[u8] = b"\\Device\\CdRom0\\default.xbe";

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

/// Programa x86 escrito à mão: abre `default.xbe`, consulta os atributos pelo nome, define a
/// posição, tenta escrever (mídia somente leitura) e usa handle inválido (ADR-0102).
pub(super) fn sources(out: &Path) -> String {
    let mut c = Vec::new();
    let mut padded = NOME.to_vec();
    padded.resize(NOME.len().div_ceil(4) * 4, 0);
    for (i, chunk) in padded.chunks(4).enumerate() {
        store(
            &mut c,
            NAME + i as u32 * 4,
            u32::from_le_bytes(chunk.try_into().unwrap()),
        );
    }
    let len = NOME.len() as u32;
    store(&mut c, STR, len | (len << 16));
    store(&mut c, STR + 4, NAME);
    store(&mut c, ATTR, 0);
    store(&mut c, ATTR + 4, STR);
    store(&mut c, ATTR + 8, 0x40);
    // NtCreateFile(&HANDLE, 0x80100080, ATTR, IO, 0, 0x80, 1, 1, 0x60)
    for arg in [0x60, 1, 1, 0x80, 0, IO, ATTR, 0x8010_0080, HANDLE] {
        push(&mut c, arg);
    }
    call(&mut c, CREATE);
    store_eax(&mut c, 0x12500);
    // NtQueryFullAttributesFile(ATTR, FULL)
    push(&mut c, FULL);
    push(&mut c, ATTR);
    call(&mut c, QUERY_FULL);
    store_eax(&mut c, 0x12504);
    // Posição 5 e NtSetInformationFile(handle, IO, &POSICAO, 8, 14)
    store(&mut c, POSICAO, 5);
    store(&mut c, POSICAO + 4, 0);
    for arg in [14, 8, POSICAO, IO] {
        push(&mut c, arg);
    }
    c.extend([0xFF, 0x35]); // push dword [HANDLE]
    c.extend(HANDLE.to_le_bytes());
    call(&mut c, SET_INFO);
    store_eax(&mut c, 0x12508);
    // NtWriteFile(handle, 0, 0, 0, IO, INFO, 4, 0)
    for arg in [0, 4, INFO, IO, 0, 0, 0] {
        push(&mut c, arg);
    }
    c.extend([0xFF, 0x35]);
    c.extend(HANDLE.to_le_bytes());
    call(&mut c, WRITE);
    store_eax(&mut c, 0x1250C);
    // NtWriteFile com handle inválido (99)
    for arg in [0, 4, INFO, IO, 0, 0, 0, 99] {
        push(&mut c, arg);
    }
    call(&mut c, WRITE);
    store_eax(&mut c, 0x12510);
    c.extend([0x31, 0xC0, 0xC3]);

    let image = xbe_com_imports(&c, &IMPORTS).unwrap();
    let program = analysis::analyze(&image).unwrap();
    std::fs::write(
        out.join("src/kernel_file_provisionals.rs"),
        emit::rust_source(&program),
    )
    .unwrap();
    std::fs::write(out.join("src/kernel_file_provisionals.xbe"), image).unwrap();
    include_str!("../verification_file_provisionals.inc").to_owned()
}
