use xbox_formats::fixtures::xbe_com_codigo;
use xbox_recomp::analysis::{Binary, Operand, Operation, analyze};
use xbox_recomp::{DEMO_CODE, Error};

#[test]
fn descobre_loop_call_e_blocos_sem_decodificar_dados_inalcancaveis() {
    let mut code = DEMO_CODE.to_vec();
    code.extend_from_slice(&[0x0F, 0x0B]); // UD2 depois do último ret: não alcançável.
    let program = analyze(&xbe_com_codigo(&code).unwrap()).unwrap();
    assert_eq!(program.instructions.len(), 10);
    assert_eq!(program.blocks.len(), 5);
    assert_eq!(program.blocks[&0x11007].len(), 3);
}

#[test]
fn rejeita_fluxo_invalido_e_instrucoes_sem_suporte_com_endereco() {
    for code in [
        &[0x0F, 0x0B][..],                   // UD2
        &[0xB8, 1][..],                      // mov truncado
        &[0xEB, 0x7F][..],                   // alvo fora do código
        &[0x67, 0x8B, 0x00, 0xC3][..],       // endereço de 16 bits (BX+SI)
        &[0xF0, 0x01, 0x08, 0xC3][..],       // LOCK add [eax],ecx
        &[0x65, 0xA1, 0, 0, 0, 0, 0xC3][..], // GS
    ] {
        assert!(matches!(
            analyze(&xbe_com_codigo(code).unwrap()),
            Err(Error::Analysis {
                pc: 0x11000 | 0x11081,
                ..
            })
        ));
    }
}

#[test]
fn rejeita_branch_no_meio_de_instrucao() {
    let bytes = xbe_com_codigo(&[0xB8, 0, 0, 0, 0, 0xEB, 0xFA]).unwrap();
    let error = analyze(&bytes).unwrap_err();
    assert!(error.to_string().contains("meio de outra instrução"));
}

#[test]
fn strings_rep_criam_blocos_retomaveis_e_prefixos_invalidos_falham() {
    use xbox_recomp::ir::{Repeat, StringOp, Width};
    let bytes = xbe_com_codigo(&[0x90, 0x67, 0x66, 0xF3, 0xA5, 0x90, 0xC3]).unwrap();
    let program = analyze(&bytes).unwrap();
    assert_eq!(program.blocks.len(), 3);
    assert_eq!(program.blocks[&0x11000].len(), 1);
    assert_eq!(program.blocks[&0x11001].len(), 1);
    assert_eq!(
        program.instructions[&0x11001].operation,
        Operation::String {
            kind: StringOp::Movs,
            width: Width::Word,
            address: Width::Word,
            repeat: Repeat::Always,
        }
    );
    for code in [
        &[0xF2, 0xA4, 0xC3][..],       // REPNE MOVS não documentado.
        &[0x64, 0xF3, 0xA4, 0xC3][..], // FS em string ainda não suportado.
        &[0xF0, 0xA4, 0xC3][..],
        &[0xF3, 0xE2, 0, 0xC3][..],
        &[0x66, 0xE2, 0, 0xC3][..], // branch16 ainda não suportado.
        &[0xF3, 0x6C, 0xC3][..],    // INS exige portas de I/O.
    ] {
        assert!(matches!(
            analyze(&xbe_com_codigo(code).unwrap()),
            Err(Error::Analysis { pc: 0x11000, .. })
        ));
    }
    for opcode in [0xE0, 0xE1, 0xE2, 0xE3] {
        // Alvo no interior do MOV deve falhar também nas novas branches.
        let code = [0xB8, 0, 0, 0, 0, opcode, 0xFA, 0xC3];
        assert!(analyze(&xbe_com_codigo(&code).unwrap()).is_err());
    }
}

#[test]
fn imediato_de_oito_bits_e_estendido_com_sinal_para_32() {
    let bytes = xbe_com_codigo(&[0x83, 0xC0, 0xFF, 0xC3]).unwrap();
    let p = analyze(&bytes).unwrap();
    assert_eq!(
        p.instructions[&0x11000].operation,
        Operation::Binary(Binary::Add, Operand::Reg(0), Operand::Imm(u32::MAX))
    );
}

#[test]
fn adc_sbb_estendem_imediato_com_sinal_na_largura_do_destino() {
    use xbox_recomp::ir::Width;
    for (code, op, dst, value) in [
        (
            &[0x83, 0xD0, 0xFF, 0xC3][..],
            Binary::Adc,
            Operand::Reg(0),
            u32::MAX,
        ),
        (
            &[0x66, 0x83, 0xD8, 0xFF, 0xC3][..],
            Binary::Sbb,
            Operand::Part {
                index: 0,
                shift: 0,
                width: Width::Word,
            },
            0xFFFF,
        ),
    ] {
        let program = analyze(&xbe_com_codigo(code).unwrap()).unwrap();
        assert_eq!(
            program.instructions[&0x11000].operation,
            Operation::Binary(op, dst, Operand::Imm(value))
        );
    }
}

#[test]
fn ramo_compartilhado_divide_um_bloco_previamente_descoberto() {
    // xor eax,eax; jz target; nop; target: mov eax,7; ret
    let bytes = xbe_com_codigo(&[0x31, 0xC0, 0x74, 1, 0x90, 0xB8, 7, 0, 0, 0, 0xC3]).unwrap();
    let p = analyze(&bytes).unwrap();
    assert_eq!(p.blocks[&0x11004].len(), 1);
    assert_eq!(p.blocks[&0x11005].len(), 2);
}

#[test]
fn shifts_aceitam_cl_imediato_e_um_implicito_com_largura_explicita() {
    use xbox_recomp::ir::{Shift, Width};
    for (op, group) in [(Shift::Shl, 4), (Shift::Shr, 5), (Shift::Sar, 7)] {
        for width in [Width::Byte, Width::Word, Width::Dword] {
            for (byte_opcode, immediate) in [(0xD0, Some(1)), (0xD2, None), (0xC0, Some(255))] {
                let mut code = if width == Width::Word {
                    vec![0x66]
                } else {
                    vec![]
                };
                code.extend_from_slice(&[
                    byte_opcode + u8::from(width != Width::Byte),
                    0xC0 | (group << 3),
                ]);
                if byte_opcode == 0xC0 {
                    code.push(255);
                }
                code.push(0xC3);
                let program = analyze(&xbe_com_codigo(&code).unwrap()).unwrap();
                let Operation::Shift(actual, dst, count) =
                    &program.instructions[&0x11000].operation
                else {
                    panic!("shift não foi reconhecido");
                };
                assert_eq!(*actual, op);
                assert_eq!(dst.width(), Some(width));
                assert_eq!(
                    *count,
                    immediate.map_or(
                        Operand::Part {
                            index: 1,
                            shift: 0,
                            width: Width::Byte
                        },
                        Operand::Imm,
                    )
                );
            }
        }
    }
}

#[test]
fn movimentos_de_bits_rejeitam_prefixos_e_endereco_de_16_bits() {
    for code in [
        &[0xF0, 0xD1, 0x20, 0xC3][..],       // lock shl [eax],1
        &[0xF3, 0xD1, 0xE0, 0xC3][..],       // rep shl eax,1
        &[0x65, 0xD1, 0x20, 0xC3][..],       // shl gs:[eax],1
        &[0x67, 0xD1, 0x20, 0xC3][..],       // shl [bx+si],1
        &[0xF0, 0xD1, 0x10, 0xC3][..],       // lock rcl [eax],1
        &[0x65, 0xD1, 0x00, 0xC3][..],       // rol gs:[eax],1
        &[0xF3, 0x0F, 0xA5, 0xC8, 0xC3][..], // rep shld eax,ecx,cl
        &[0x67, 0x0F, 0xAD, 0x08, 0xC3][..], // shrd [bx+si],ecx,cl
    ] {
        assert!(matches!(
            analyze(&xbe_com_codigo(code).unwrap()),
            Err(Error::Analysis { pc: 0x11000, .. })
        ));
    }
}

#[test]
fn layout_sobreposto_nao_pode_divergir_do_loader() {
    let mut bytes = xbe_com_codigo(&[0xC3]).unwrap();
    bytes[0x338 + 4..0x338 + 8].copy_from_slice(&0x11000u32.to_le_bytes());
    assert!(analyze(&bytes).is_err());
}

#[test]
fn mutacoes_e_truncamentos_retornam_sem_panico_ou_loop() {
    let original = xbe_com_codigo(DEMO_CODE).unwrap();
    for len in (0..original.len()).step_by(13) {
        let _ = analyze(&original[..len]);
    }
    for position in 0..DEMO_CODE.len() {
        for value in [0, 0x66, 0x67, 0xF0, 0xFF] {
            let mut bytes = original.clone();
            bytes[0x1000 + position] = value;
            let _ = analyze(&bytes);
        }
    }
}

#[test]
fn extensao_da_isa_nao_aceita_segmentos_address16_ou_calls16() {
    for code in [
        &[0x8C, 0xC0, 0xC3][..],       // mov ax,es
        &[0x67, 0xA1, 0, 0, 0xC3][..], // mov eax,[disp16]
        &[0x66, 0xE8, 0, 0, 0xC3][..], // call rel16
        &[0x66, 0xC3][..],             // ret de 16 bits
        &[0x0F, 0xA3, 0xC8, 0xC3][..], // bt eax,ecx
    ] {
        assert!(matches!(
            analyze(&xbe_com_codigo(code).unwrap()),
            Err(Error::Analysis { pc: 0x11000, .. })
        ));
    }
}

#[test]
fn al_ah_ax_e_memoria_tem_largura_explicita_na_ir() {
    use xbox_recomp::ir::Width;
    let bytes = xbe_com_codigo(&[
        0xB0, 1, 0xB4, 2, 0x66, 0xB8, 3, 0, 0xC6, 0x05, 0x20, 0x20, 1, 0, 4, 0xC3,
    ])
    .unwrap();
    let p = analyze(&bytes).unwrap();
    for (pc, index, shift, width) in [
        (0x11000, 0, 0, Width::Byte),
        (0x11002, 0, 8, Width::Byte),
        (0x11004, 0, 0, Width::Word),
    ] {
        assert!(
            matches!(&p.instructions[&pc].operation, Operation::Binary(Binary::Mov, dst, _)
            if *dst == Operand::Part { index, shift, width })
        );
    }
    assert!(matches!(
        &p.instructions[&0x11008].operation,
        Operation::Binary(
            Binary::Mov,
            Operand::Mem {
                width: Width::Byte,
                ..
            },
            _
        )
    ));
}
