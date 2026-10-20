use xbox_formats::fixtures::xbe_com_codigo;
use xbox_recomp::{
    analysis::analyze,
    ir::{BitScan, Operation, Width},
};

#[test]
fn bsf_bsr_tem_ir_propria_em_word_dword_registrador_e_memoria() {
    for (opcode, expected) in [(0xBC, BitScan::Forward), (0xBD, BitScan::Reverse)] {
        for (prefix, width) in [(vec![0x66], Width::Word), (vec![], Width::Dword)] {
            for modrm in [0xC1, 0x06] {
                let mut code = prefix.clone();
                code.extend_from_slice(&[0x0F, opcode, modrm, 0xC3]);
                let p = analyze(&xbe_com_codigo(&code).unwrap()).unwrap();
                let Operation::BitScan { kind, dst, src } = &p.instructions[&0x11000].operation
                else {
                    panic!("IR incorreta")
                };
                assert_eq!(*kind, expected);
                assert_eq!(dst.width(), Some(width));
                assert_eq!(src.width(), Some(width));
            }
        }
    }
}

#[test]
fn bsf_bsr_nao_habilitam_lock_rep_lzcnt_tzcnt_ou_enderecamento_16() {
    for opcode in [0xBC, 0xBD] {
        for prefix in [0xF0, 0xF2, 0xF3, 0x65] {
            assert!(
                analyze(&xbe_com_codigo(&[prefix, 0x0F, opcode, 0x06, 0xC3]).unwrap()).is_err()
            );
        }
        assert!(
            analyze(&xbe_com_codigo(&[0x67, 0x0F, opcode, 0x06, 0, 0, 0xC3]).unwrap()).is_err()
        );
    }
}
