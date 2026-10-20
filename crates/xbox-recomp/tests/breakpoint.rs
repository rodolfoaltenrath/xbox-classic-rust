use xbox_formats::fixtures::xbe_com_codigo;
use xbox_recomp::{Error, analysis, ir::Operation};

#[test]
fn int3_e_uma_fronteira_terminal_com_pc_seguinte_explicito() {
    let bytes = xbe_com_codigo(&[0xCC, 0xFB, 0xC3]).unwrap();
    let p = analysis::analyze(&bytes).unwrap();
    assert_eq!(p.instructions.len(), 1);
    assert_eq!(p.instructions[&0x11000].operation, Operation::Breakpoint);
    assert_eq!(p.instructions[&0x11000].next, 0x11001);
    assert_eq!(p.blocks[&0x11000].len(), 1);
    // A recusa de STI não é escondida se esse PC é uma raiz legítima.
    assert!(matches!(
        analysis::analyze_with_entries(&bytes, &[0x11001]),
        Err(Error::Analysis { pc: 0x11001, .. })
    ));
    assert!(analysis::analyze(&xbe_com_codigo(&[0xCC]).unwrap()).is_ok());
}

#[test]
fn int3_nao_descarta_outros_caminhos_alcancaveis() {
    let p = analysis::analyze(&xbe_com_codigo(&[0x74, 1, 0xCC, 0xC3]).unwrap()).unwrap();
    assert_eq!(p.instructions.len(), 3);
    assert_eq!(p.instructions[&0x11002].operation, Operation::Breakpoint);
    assert_eq!(p.instructions[&0x11003].operation, Operation::Ret(0));
}

#[test]
fn apenas_o_opcode_int3_curto_e_suportado() {
    for code in [
        &[0xF0, 0xCC][..],
        &[0xF2, 0xCC],
        &[0xF3, 0xCC],
        &[0xCD, 3],
        &[0xCD, 0x80],
        &[0xF1],
        &[0xCE],
        &[0x0F, 0x0B],
    ] {
        assert!(analysis::analyze(&xbe_com_codigo(code).unwrap()).is_err());
    }
}
