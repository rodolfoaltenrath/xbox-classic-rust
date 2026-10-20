use xbox_formats::fixtures::xbe_com_codigo;
use xbox_recomp::{
    analysis::analyze,
    ir::{Condition, Operand, Operation, Width},
};

#[test]
fn cmovcc_cobre_as_16_condicoes_e_larguras_de_registrador_e_memoria() {
    let conditions = [
        Condition::Overflow,
        Condition::NotOverflow,
        Condition::Below,
        Condition::AboveEqual,
        Condition::Equal,
        Condition::NotEqual,
        Condition::BelowEqual,
        Condition::Above,
        Condition::Sign,
        Condition::NotSign,
        Condition::Parity,
        Condition::NotParity,
        Condition::Less,
        Condition::GreaterEqual,
        Condition::LessEqual,
        Condition::Greater,
    ];
    for (cc, condition) in conditions.into_iter().enumerate() {
        for (prefix, width) in [(vec![], Width::Dword), (vec![0x66], Width::Word)] {
            for (modrm, memory) in [(0xC1, false), (0x06, true)] {
                let mut code = prefix.clone();
                code.extend_from_slice(&[0x0F, 0x40 + cc as u8, modrm, 0xC3]);
                let p = analyze(&xbe_com_codigo(&code).unwrap()).unwrap();
                let Operation::ConditionalMove {
                    condition: actual,
                    dst,
                    src,
                } = &p.instructions[&0x11000].operation
                else {
                    panic!("IR incorreta")
                };
                assert_eq!(*actual, condition);
                assert_eq!(dst.width(), Some(width));
                assert_eq!(src.width(), Some(width));
                assert_eq!(matches!(src, Operand::Mem { .. }), memory);
            }
        }
    }
}

#[test]
fn cmovcc_recusa_lock_rep_segmentos_e_enderecamento_16_nao_suportados() {
    for code in [
        &[0xF0, 0x0F, 0x45, 0xC1, 0xC3][..],
        &[0xF3, 0x0F, 0x45, 0xC1, 0xC3],
        &[0xF2, 0x0F, 0x45, 0xC1, 0xC3],
        &[0x65, 0x0F, 0x45, 0x06, 0xC3],
        &[0x67, 0x0F, 0x45, 0x06, 0, 0, 0xC3],
    ] {
        assert!(analyze(&xbe_com_codigo(code).unwrap()).is_err());
    }
}
