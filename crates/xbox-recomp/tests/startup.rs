use xbox_formats::{
    fixtures::{xbe_com_codigo, xbe_com_imports},
    xbe::SectionFlags,
};
use xbox_recomp::{
    analysis,
    ir::{Binary, Operand, Operation},
};

#[test]
fn fs_e_explicito_na_ir_e_lea_infere_offset_sem_base_de_segmento() {
    let mut code = vec![0x64, 0x8D, 5, 0x10, 0x10, 1, 0, 0xFF, 0xD0, 0xC3];
    code.resize(16, 0x90);
    code.push(0xC3);
    let p = analysis::analyze(&xbe_com_codigo(&code).unwrap()).unwrap();
    assert!(p.blocks.contains_key(&0x11010));
    let p = analysis::analyze(&xbe_com_codigo(&[0x64, 0xA1, 4, 0, 0, 0, 0xC3]).unwrap()).unwrap();
    assert!(matches!(
        &p.instructions[&0x11000].operation,
        Operation::Binary(Binary::Mov, _, Operand::Mem { fs: true, .. })
    ));
}

#[test]
fn fs_nao_pode_ser_confundido_com_ponteiro_absoluto_immutavel() {
    let mut code = vec![0x64, 0xFF, 0x15, 0x20, 0x20, 1, 0, 0xC3];
    code.resize(16, 0x90);
    code.push(0xC3);
    let mut image = xbe_com_codigo(&code).unwrap();
    image[0x338..0x33C].copy_from_slice(&SectionFlags::PRELOAD.to_le_bytes());
    image[0x2020..0x2024].copy_from_slice(&0x11010u32.to_le_bytes());
    let p = analysis::analyze(&image).unwrap();
    assert!(!p.blocks.contains_key(&0x11010));
}

#[test]
fn imports_duplicados_preservam_tabela_mas_nao_duplicam_despacho() {
    let p = analysis::analyze(&xbe_com_imports(&[0xC3], &[8, 165, 8]).unwrap()).unwrap();
    assert_eq!(p.kernel_imports.len(), 2);
    let source = xbox_recomp::emit::rust_source(&p);
    assert_eq!(source.matches("0x7FFE0040 =>").count(), 1);
}
