use xbox_formats::fixtures::xbe_com_codigo;
use xbox_recomp::{analysis::analyze, ir::Operation};

#[test]
fn cli_hlt_tem_ir_propria_e_continuacao_traduzida() {
    let p = analyze(&xbe_com_codigo(&[0xFA, 0xF4, 0xEB, 0xFD]).unwrap()).unwrap();
    assert_eq!(
        p.instructions[&0x11000].operation,
        Operation::ClearInterrupts
    );
    assert_eq!(p.instructions[&0x11001].operation, Operation::Halt);
    assert_eq!(p.instructions[&0x11002].operation, Operation::Jump(0x11001));
    assert!(p.blocks.contains_key(&0x11002));
    assert_eq!(p.blocks[&0x11001].len(), 1);
}

#[test]
fn hlt_nao_esconde_instrucao_faltante_na_continuacao() {
    let err = analyze(&xbe_com_codigo(&[0xF4, 0x0F, 0x0B]).unwrap()).unwrap_err();
    assert!(matches!(
        err,
        xbox_recomp::Error::Analysis { pc: 0x11001, .. }
    ));
}

#[test]
fn cli_hlt_recusam_lock_e_rep() {
    for instruction in [0xFA, 0xF4] {
        for prefix in [0xF0, 0xF2, 0xF3] {
            assert!(analyze(&xbe_com_codigo(&[prefix, instruction, 0xC3]).unwrap()).is_err());
        }
    }
}
