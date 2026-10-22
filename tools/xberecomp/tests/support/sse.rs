use std::path::Path;
use xbox_formats::fixtures::xbe_com_codigo;
use xbox_recomp::{analysis, emit};

const SS: &[u8] = &[0xF3, 0x0F];

/// Instrução com operando de memória absoluto (`mod=00, rm=101`).
fn mem(code: &mut Vec<u8>, prefix: &[u8], opcode: u8, reg: u8, address: u32) {
    code.extend(prefix);
    code.push(opcode);
    code.push(reg << 3 | 5);
    code.extend(address.to_le_bytes());
}

/// Instrução registrador↔registrador (`mod=11`).
fn reg(code: &mut Vec<u8>, prefix: &[u8], opcode: u8, dst: u8, src: u8) {
    code.extend(prefix);
    code.push(opcode);
    code.push(0xC0 | dst << 3 | src);
}

fn imm(code: &mut Vec<u8>, address: u32, value: u32) {
    code.extend([0xC7, 0x05]); // mov dword [address], imm32
    code.extend(address.to_le_bytes());
    code.extend(value.to_le_bytes());
}

fn store_eax(code: &mut Vec<u8>, address: u32) {
    code.push(0xA3); // mov [address], eax
    code.extend(address.to_le_bytes());
}

fn set_byte(code: &mut Vec<u8>, condition: u8, address: u32) {
    code.extend([0x0F, condition, 0x05]); // setcc [address]
    code.extend(address.to_le_bytes());
}

/// Programa x86 escrito à mão, byte a byte, para cada instrução SSE implementada.
///
/// Os resultados vão para memória guest em `0x123xx`, que o teste lê depois. As constantes
/// são gravadas pelo próprio programa porque `.rdata` da fixture só tem 0x40 bytes.
pub(super) fn sources(output: &Path) -> String {
    let (a, b, neg, inf, nan) = (0x12200, 0x12204, 0x12208, 0x1220C, 0x12210);
    let mut c = Vec::new();
    imm(&mut c, a, 6.0f32.to_bits());
    imm(&mut c, b, 4.0f32.to_bits());
    imm(&mut c, neg, (-7.9f32).to_bits());
    imm(&mut c, inf, f32::INFINITY.to_bits());
    imm(&mut c, nan, 0x7FC0_0000);
    mem(&mut c, SS, 0x10, 1, b); // movss xmm1,[b]

    // Aritmética: xmm0 = A (6) op xmm1 (4).
    for (slot, opcode) in [
        (0x12300, 0x58), // addss  -> 10
        (0x12304, 0x5C), // subss  -> 2
        (0x12308, 0x59), // mulss  -> 24
        (0x1230C, 0x5E), // divss  -> 1.5
        (0x12314, 0x5D), // minss  -> 4
        (0x12318, 0x5F), // maxss  -> 6
    ] {
        mem(&mut c, SS, 0x10, 0, a); // movss xmm0,[a]
        reg(&mut c, SS, opcode, 0, 1); // op xmm0,xmm1
        mem(&mut c, SS, 0x11, 0, slot); // movss [slot],xmm0
    }
    reg(&mut c, SS, 0x51, 0, 1); // sqrtss xmm0,xmm1 -> 2
    mem(&mut c, SS, 0x11, 0, 0x12310);

    // Origem em memória (mulss xmm0,[b]) em vez de registrador.
    mem(&mut c, SS, 0x10, 0, a);
    mem(&mut c, SS, 0x59, 0, b);
    mem(&mut c, SS, 0x11, 0, 0x12340);

    // Inteiro -> float.
    c.extend([0xB8, 7, 0, 0, 0]); // mov eax,7
    reg(&mut c, SS, 0x2A, 2, 0); // cvtsi2ss xmm2,eax
    mem(&mut c, SS, 0x11, 2, 0x1231C);

    // Float -> inteiro: trunca (-7), arredonda ao par (-8), indefinido para infinito.
    mem(&mut c, SS, 0x10, 0, neg);
    reg(&mut c, SS, 0x2C, 0, 0); // cvttss2si eax,xmm0
    store_eax(&mut c, 0x12320);
    reg(&mut c, SS, 0x2D, 1, 0); // cvtss2si ecx,xmm0
    c.extend([0x89, 0x0D]); // mov [0x12324],ecx
    c.extend(0x12324u32.to_le_bytes());
    mem(&mut c, SS, 0x10, 0, inf);
    reg(&mut c, SS, 0x2C, 0, 0);
    store_eax(&mut c, 0x12328);

    // Comparação: 6 ? 4, 4 ? 6, 6 ? 6, 6 ? NaN.
    mem(&mut c, SS, 0x10, 0, a);
    mem(&mut c, SS, 0x10, 5, nan);
    c.extend([0x0F, 0x2E, 0xC1]); // ucomiss xmm0,xmm1
    set_byte(&mut c, 0x97, 0x1232C); // seta
    set_byte(&mut c, 0x92, 0x12330); // setb
    c.extend([0x0F, 0x2E, 0xC8]); // ucomiss xmm1,xmm0
    set_byte(&mut c, 0x92, 0x12334); // setb
    c.extend([0x0F, 0x2E, 0xC0]); // ucomiss xmm0,xmm0
    set_byte(&mut c, 0x94, 0x12338); // sete
    c.extend([0x0F, 0x2E, 0xC5]); // ucomiss xmm0,xmm5 (NaN)
    set_byte(&mut c, 0x9A, 0x1233C); // setp

    // 128 bits: cinco palavras em 0x12240.. (0x12240 alinhado a 16).
    for (i, v) in [
        0x1111_1111u32,
        0x2222_2222,
        0x3333_3333,
        0x4444_4444,
        0x5555_5555,
    ]
    .into_iter()
    .enumerate()
    {
        imm(&mut c, 0x12240 + 4 * i as u32, v);
    }
    mem(&mut c, &[0x0F], 0x28, 3, 0x12240); // movaps xmm3,[0x12240]
    mem(&mut c, &[0x0F], 0x29, 3, 0x12350); // movaps [0x12350],xmm3
    reg(&mut c, &[0x0F], 0x57, 3, 3); // xorps xmm3,xmm3
    mem(&mut c, &[0x0F], 0x29, 3, 0x12360); // movaps [0x12360],xmm3 -> zeros
    // MOVSS reg,reg preserva as palavras altas; a carga da memória as zera.
    mem(&mut c, &[0x0F], 0x28, 4, 0x12240); // movaps xmm4,[0x12240]
    reg(&mut c, SS, 0x10, 4, 1); // movss xmm4,xmm1
    mem(&mut c, &[0x0F], 0x29, 4, 0x12370);
    mem(&mut c, &[0x0F], 0x28, 4, 0x12240);
    mem(&mut c, SS, 0x10, 4, a); // movss xmm4,[a] -> zera altas
    mem(&mut c, &[0x0F], 0x29, 4, 0x12380);
    // XORPS com máscara de sinal em memória alinhada.
    imm(&mut c, 0x12280, 0x8000_0000);
    imm(&mut c, 0x12284, 0);
    imm(&mut c, 0x12288, 0);
    imm(&mut c, 0x1228C, 0);
    mem(&mut c, SS, 0x10, 0, a);
    mem(&mut c, &[0x0F], 0x57, 0, 0x12280); // xorps xmm0,[0x12280]
    mem(&mut c, SS, 0x11, 0, 0x12390);
    // MOVUPS aceita endereço desalinhado: palavras 1..4 de 0x12240.
    mem(&mut c, &[0x0F], 0x10, 6, 0x12244); // movups xmm6,[0x12244]
    mem(&mut c, &[0x0F], 0x11, 6, 0x123A0); // movups [0x123A0],xmm6
    c.push(0xC3);

    let image = xbe_com_codigo(&c).unwrap();
    let program = analysis::analyze(&image).unwrap();
    std::fs::write(output.join("src/sse_ok.rs"), emit::rust_source(&program)).unwrap();
    std::fs::write(output.join("src/sse_ok.xbe"), image).unwrap();

    // MOVAPS desalinhado: #GP no hardware.
    let mut falta = Vec::new();
    mem(&mut falta, &[0x0F], 0x28, 0, 0x12244); // movaps xmm0,[0x12244]
    falta.push(0xC3);
    let image = xbe_com_codigo(&falta).unwrap();
    let program = analysis::analyze(&image).unwrap();
    std::fs::write(output.join("src/sse_falta.rs"), emit::rust_source(&program)).unwrap();
    std::fs::write(output.join("src/sse_falta.xbe"), image).unwrap();
    include_str!("../verification_sse.inc").to_owned()
}
