use std::path::Path;
use xbox_formats::fixtures::xbe_com_codigo;
use xbox_recomp::{analysis, emit};

fn mov_imm(code: &mut Vec<u8>, register: u8, value: u32) {
    code.push(0xB8 + register); // mov r32, imm32
    code.extend(value.to_le_bytes());
}

fn store_eax(code: &mut Vec<u8>, address: u32) {
    code.push(0xA3);
    code.extend(address.to_le_bytes());
}

/// `setcc byte [address]`.
fn set_byte(code: &mut Vec<u8>, condition: u8, address: u32) {
    code.extend([0x0F, condition, 0x05]);
    code.extend(address.to_le_bytes());
}

/// BT/BTS/BTR/BTC com origem registrador: `0F <op> /r`, destino registrador (`mod=11`).
fn bit_reg(code: &mut Vec<u8>, opcode: u8, source: u8, destination: u8) {
    code.extend([0x0F, opcode, 0xC0 | source << 3 | destination]);
}

/// Idem, com destino em memória absoluta.
fn bit_mem(code: &mut Vec<u8>, opcode: u8, source: u8, address: u32) {
    code.extend([0x0F, opcode, source << 3 | 5]);
    code.extend(address.to_le_bytes());
}

/// Programa x86 escrito à mão para BT/BTS/BTR/BTC. Saídas em `0x125xx`.
pub(super) fn sources(output: &Path) -> String {
    const CF: u8 = 0x92; // setc/setb
    const EAX: u8 = 0;
    const EBX: u8 = 3;
    let mut c = Vec::new();

    mov_imm(&mut c, 0, 0); // eax = 0
    mov_imm(&mut c, 3, 5); // ebx = 5
    bit_reg(&mut c, 0xAB, EBX, EAX); // bts eax,ebx -> 0x20
    set_byte(&mut c, CF, 0x12504); // CF = bit antigo = 0
    store_eax(&mut c, 0x12500);
    c.extend([0x0F, 0xBA, 0xE0, 0x05]); // bt eax,5 (imediato)
    set_byte(&mut c, CF, 0x12508); // CF = 1
    bit_reg(&mut c, 0xB3, EBX, EAX); // btr eax,ebx -> 0
    set_byte(&mut c, CF, 0x1250C); // CF = 1 (bit antigo)
    store_eax(&mut c, 0x12510);
    c.extend([0x0F, 0xBA, 0xF8, 0x1F]); // btc eax,31 -> 0x80000000
    set_byte(&mut c, CF, 0x12514); // CF = 0
    store_eax(&mut c, 0x12518);
    // Imediato usa só os 5 bits baixos: bt eax,63 testa o bit 31.
    c.extend([0x0F, 0xBA, 0xE0, 0x3F]);
    set_byte(&mut c, CF, 0x1251C); // CF = 1

    // Memória com registrador: o deslocamento anda pela memória.
    mov_imm(&mut c, 3, 70); // ebx = 70 -> palavra +2, bit 6
    bit_mem(&mut c, 0xAB, EBX, 0x12400); // bts [0x12400],ebx -> [0x12408] = 0x40
    set_byte(&mut c, CF, 0x12520); // CF = 0
    mov_imm(&mut c, 3, 0xFFFF_FFFF); // ebx = -1 -> palavra -1, bit 31
    bit_mem(&mut c, 0xBB, EBX, 0x12404); // btc [0x12404],ebx -> [0x12400] = 0x80000000
    // Memória com imediato: o endereço NÃO anda, só o bit (33 -> bit 1).
    c.extend([0x0F, 0xBA, 0x2D]); // bts dword [0x12410], 33
    c.extend(0x12410u32.to_le_bytes());
    c.push(33);

    // WBINVD não tem efeito observável na memória coerente do guest.
    c.extend([0x0F, 0x09]);
    // BT preserva ZF: ligar ZF com XOR e conferir depois.
    c.extend([0x31, 0xC9]); // xor ecx,ecx  (ZF = 1)
    c.extend([0x0F, 0xBA, 0xE0, 0x00]); // bt eax,0 (bit 0 de 0x80000000 = 0)
    set_byte(&mut c, 0x94, 0x12524); // sete -> 1: ZF intocado
    c.push(0xC3);

    let image = xbe_com_codigo(&c).unwrap();
    let program = analysis::analyze(&image).unwrap();
    std::fs::write(output.join("src/bit_test.rs"), emit::rust_source(&program)).unwrap();
    std::fs::write(output.join("src/bit_test.xbe"), image).unwrap();
    include_str!("../verification_bit_test.inc").to_owned()
}
