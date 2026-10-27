use std::path::Path;
use xbox_formats::fixtures::xbe_com_codigo;
use xbox_recomp::{analysis, emit};

/// Instrução x87 com operando de memória absoluto: `opcode`, `/reg`, disp32.
fn mem(code: &mut Vec<u8>, opcode: u8, reg: u8, address: u32) {
    code.push(opcode);
    code.push(reg << 3 | 5);
    code.extend(address.to_le_bytes());
}

fn imm(code: &mut Vec<u8>, address: u32, value: u32) {
    code.extend([0xC7, 0x05]);
    code.extend(address.to_le_bytes());
    code.extend(value.to_le_bytes());
}

fn publish(output: &Path, name: &str, code: &[u8]) {
    let image = xbe_com_codigo(code).unwrap();
    let program = analysis::analyze(&image).unwrap();
    std::fs::write(
        output.join(format!("src/{name}.rs")),
        emit::rust_source(&program),
    )
    .unwrap();
    std::fs::write(output.join(format!("src/{name}.xbe")), image).unwrap();
}

/// Programas x86 escritos à mão para a carga e o armazenamento x87.
pub(super) fn sources(output: &Path) -> String {
    let mut c = Vec::new();
    // FILD m64 de 2^53 + 1 (não cabe em f64): FSTP m64 arredonda uma vez, ao par.
    imm(&mut c, 0x12600, 1);
    imm(&mut c, 0x12604, 0x0020_0000);
    mem(&mut c, 0xDF, 5, 0x12600); // fild qword
    mem(&mut c, 0xDD, 3, 0x12700); // fstp qword
    // FILD m32 de -5 -> FSTP m32.
    imm(&mut c, 0x12608, (-5i32).cast_unsigned());
    mem(&mut c, 0xDB, 0, 0x12608); // fild dword
    mem(&mut c, 0xD9, 3, 0x12710); // fstp dword
    // FILD m16 de -2 (palavra de 16 bits): `66 C7 05 disp imm16`.
    c.extend([0x66, 0xC7, 0x05]);
    c.extend(0x1260Cu32.to_le_bytes());
    c.extend(0xFFFEu16.to_le_bytes());
    mem(&mut c, 0xDF, 0, 0x1260C); // fild word
    mem(&mut c, 0xD9, 3, 0x12718); // fstp dword
    // FLDZ sobrescreve memória pré-preenchida com 1s.
    imm(&mut c, 0x12720, u32::MAX);
    imm(&mut c, 0x12724, u32::MAX);
    c.extend([0xD9, 0xEE]); // fldz
    mem(&mut c, 0xDD, 3, 0x12720); // fstp qword
    // FLD1 -> FSTP m80.
    c.extend([0xD9, 0xE8]); // fld1
    mem(&mut c, 0xDB, 7, 0x12730); // fstp tbyte
    // FLD m32 de 1,5; FST m64 não retira, FSTP m32 retira.
    imm(&mut c, 0x12610, 1.5f32.to_bits());
    mem(&mut c, 0xD9, 0, 0x12610); // fld dword
    mem(&mut c, 0xDD, 2, 0x12740); // fst qword
    mem(&mut c, 0xD9, 3, 0x12748); // fstp dword
    // 0,1 (f64) por m80 e de volta: ida e volta sem perda.
    let decimo = 0.1f64.to_bits();
    imm(&mut c, 0x12618, decimo as u32);
    imm(&mut c, 0x1261C, (decimo >> 32) as u32);
    mem(&mut c, 0xDD, 0, 0x12618); // fld qword
    mem(&mut c, 0xDB, 7, 0x12750); // fstp tbyte
    mem(&mut c, 0xDB, 5, 0x12750); // fld tbyte
    mem(&mut c, 0xDD, 3, 0x12760); // fstp qword
    c.push(0xC3);
    publish(output, "x87_ok", &c);

    // FSTP com a pilha vazia: #IS, sem escrever na memória.
    let mut vazia = Vec::new();
    imm(&mut vazia, 0x12700, 0xAAAA_AAAA);
    mem(&mut vazia, 0xDD, 3, 0x12700); // fstp qword
    vazia.push(0xC3);
    publish(output, "x87_vazia", &vazia);

    // Nove FLD1: o nono estoura a pilha de 8.
    let mut cheia = Vec::new();
    for _ in 0..9 {
        cheia.extend([0xD9, 0xE8]);
    }
    cheia.push(0xC3);
    publish(output, "x87_cheia", &cheia);

    // Deixa dois valores na pilha, para o snapshot.
    publish(output, "x87_deixa", &[0xD9, 0xE8, 0xD9, 0xEE, 0xC3]);

    // Aritmética do mesh (ADR-0088): FMULP, FDIVRP, FSQRT, FSIN e FCOS.
    let mut a = Vec::new();
    imm(&mut a, 0x12800, 1.5f32.to_bits());
    imm(&mut a, 0x12804, 2.0f32.to_bits());
    imm(&mut a, 0x12808, 3.0f32.to_bits());
    imm(&mut a, 0x1280C, 1.0f32.to_bits());
    imm(&mut a, 0x12810, 0.5f32.to_bits());
    mem(&mut a, 0xD9, 0, 0x12800); // fld 1,5 → ST(1)
    mem(&mut a, 0xD9, 0, 0x12804); // fld 2 → ST(0)
    a.extend([0xDE, 0xC9]); // fmulp st(1), st → 3
    mem(&mut a, 0xDD, 3, 0x12900); // fstp qword
    mem(&mut a, 0xD9, 0, 0x12808); // fld 3 → ST(1)
    mem(&mut a, 0xD9, 0, 0x1280C); // fld 1 → ST(0)
    a.extend([0xDE, 0xF1]); // fdivrp st(1), st → ST(0)/ST(1) = 1/3
    mem(&mut a, 0xDB, 7, 0x12910); // fstp tbyte
    mem(&mut a, 0xD9, 0, 0x12804); // fld 2
    a.extend([0xD9, 0xFA]); // fsqrt
    mem(&mut a, 0xDB, 7, 0x12920); // fstp tbyte
    mem(&mut a, 0xD9, 0, 0x12810); // fld 0,5
    a.extend([0xD9, 0xFE]); // fsin
    mem(&mut a, 0xDD, 3, 0x12930); // fstp qword
    mem(&mut a, 0xD9, 0, 0x12810); // fld 0,5
    a.extend([0xD9, 0xFF]); // fcos
    mem(&mut a, 0xD9, 3, 0x12938); // fstp dword
    a.push(0xC3);
    publish(output, "x87_aritmetica", &a);

    // FSIN fora do recorte (|x| ≥ 2^62): recusa sem alterar a pilha.
    let mut fora = Vec::new();
    let enorme = 1e19f64.to_bits();
    imm(&mut fora, 0x12A00, enorme as u32);
    imm(&mut fora, 0x12A04, (enorme >> 32) as u32);
    mem(&mut fora, 0xDD, 0, 0x12A00); // fld qword
    fora.extend([0xD9, 0xFE]); // fsin
    fora.push(0xC3);
    publish(output, "x87_trig_fora", &fora);
    include_str!("../verification_x87.inc").to_owned()
}
