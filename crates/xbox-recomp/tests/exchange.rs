use xbox_formats::fixtures::xbe_com_codigo;
use xbox_recomp::{
    analysis::analyze,
    ir::{Operation, Width},
};

#[test]
fn cmpxchg_larguras_lock_e_setcc_sao_ir_explicita() {
    for (prefix, opcode, width) in [
        (vec![], 0xB0, Width::Byte),
        (vec![0x66], 0xB1, Width::Word),
        (vec![], 0xB1, Width::Dword),
    ] {
        for locked in [false, true] {
            let mut code = prefix.clone();
            if locked {
                code.push(0xF0);
            }
            code.extend_from_slice(&[0x0F, opcode, 0x15, 0x20, 0x20, 1, 0, 0xC3]);
            let p = analyze(&xbe_com_codigo(&code).unwrap()).unwrap();
            assert!(
                matches!(&p.instructions[&0x11000].operation, Operation::CompareExchange { dst, locked: actual, .. } if dst.width() == Some(width) && *actual == locked)
            );
        }
    }
    for condition in 0..16 {
        let p = analyze(&xbe_com_codigo(&[0x0F, 0x90 + condition, 0xC4, 0xC3]).unwrap()).unwrap();
        assert!(matches!(
            p.instructions[&0x11000].operation,
            Operation::SetCondition { .. }
        ));
    }
}

#[test]
fn prefixos_invalidos_lock_registrador_e_cmpxchg8b_continuam_recusados() {
    for code in [
        &[0xF0, 0x0F, 0xB1, 0xD1, 0xC3][..],
        &[0xF3, 0x0F, 0xB1, 0xD1, 0xC3][..],
        &[0xF0, 0x0F, 0x94, 0x00, 0xC3][..],
        &[0x67, 0x0F, 0xB1, 0x00, 0xC3][..],
        &[0x0F, 0xC7, 0x08, 0xC3][..],
        &[0x65, 0x0F, 0xB1, 0x00, 0xC3][..],
    ] {
        assert!(
            analyze(&xbe_com_codigo(code).unwrap()).is_err(),
            "{code:02X?}"
        );
    }
}
