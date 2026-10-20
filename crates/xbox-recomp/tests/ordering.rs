use xbox_formats::fixtures::xbe_com_codigo;
use xbox_recomp::{analysis::analyze, ir::Operation};

#[test]
fn sfence_tem_ir_propria_e_nao_interrompe_a_descoberta() {
    let p = analyze(&xbe_com_codigo(&[0x0F, 0xAE, 0xF8, 0xC3]).unwrap()).unwrap();
    assert_eq!(p.instructions[&0x11000].operation, Operation::StoreFence);
    assert_eq!(p.instructions[&0x11000].next, 0x11003);
    assert_eq!(p.instructions[&0x11003].operation, Operation::Ret(0));
}

#[test]
fn sfence_nao_habilita_outros_opcodes_ou_prefixos() {
    for code in [
        &[0xF0, 0x0F, 0xAE, 0xF8][..],
        &[0xF2, 0x0F, 0xAE, 0xF8],
        &[0xF3, 0x0F, 0xAE, 0xF8],
        &[0x0F, 0xAE, 0xE8],
        &[0x0F, 0xAE, 0xF0],
        &[0x0F, 0xAE, 0x38],
    ] {
        assert!(analyze(&xbe_com_codigo(code).unwrap()).is_err());
    }
}
