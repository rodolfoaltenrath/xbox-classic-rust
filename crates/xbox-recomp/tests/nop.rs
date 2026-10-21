use xbox_formats::fixtures::xbe_com_codigo;
use xbox_recomp::{analysis::analyze, ir::Operation};

#[test]
fn nops_multibyte_tem_comprimento_correto_sem_operando_de_memoria_na_ir() {
    for bytes in [
        &[0x0F, 0x1F, 0xC0][..], // registrador
        &[0x0F, 0x1F, 0],        // [eax]
        &[0x0F, 0x1F, 0x40, 0],
        &[0x0F, 0x1F, 0x44, 0, 0],
        &[0x66, 0x0F, 0x1F, 0x44, 0, 0],
        &[0x0F, 0x1F, 0x80, 0, 0, 0, 0],
        &[0x0F, 0x1F, 0x84, 0, 0, 0, 0, 0], // forma do alinhamento real em 0x13C68
        &[0x66, 0x0F, 0x1F, 0x84, 0, 0, 0, 0, 0],
        &[0x64, 0x0F, 0x1F, 0], // FS não produz acesso
        &[0x67, 0x0F, 0x1F, 0], // endereço 16 não precisa ser calculado
    ] {
        let mut code = bytes.to_vec();
        code.push(0xC3);
        let program = analyze(&xbe_com_codigo(&code).unwrap()).unwrap();
        assert_eq!(program.instructions[&0x11000].operation, Operation::Nop);
        let next = 0x11000 + bytes.len() as u32;
        assert_eq!(program.instructions[&0x11000].next, next);
        assert_eq!(program.instructions[&next].operation, Operation::Ret(0));
    }
}

#[test]
fn suporte_nop_nao_habilita_lock_rep_pause_ou_instrucao_truncada() {
    for bytes in [
        &[0xF0, 0x0F, 0x1F, 0, 0xC3][..],
        &[0xF2, 0x0F, 0x1F, 0, 0xC3],
        &[0xF3, 0x0F, 0x1F, 0, 0xC3],
        &[0xF3, 0x90, 0xC3],          // PAUSE permanece sem modelo
        &[0x65, 0x0F, 0x1F, 0, 0xC3], // GS continua fora do perfil de segmentos
        &[0x0F, 0x1F, 0x84],
    ] {
        assert!(analyze(&xbe_com_codigo(bytes).unwrap()).is_err());
    }
}

#[test]
fn nop_de_memoria_nao_apaga_alvo_indireto_descoberto() {
    let mut code = vec![
        0xB8, 0x20, 0x10, 1, 0, 0x0F, 0x1F, 0x44, 0, 0, 0xFF, 0xD0, 0xC3,
    ];
    code.resize(32, 0xCC);
    code.push(0xC3);
    let program = analyze(&xbe_com_codigo(&code).unwrap()).unwrap();
    assert_eq!(program.instructions[&0x11020].operation, Operation::Ret(0));
}
