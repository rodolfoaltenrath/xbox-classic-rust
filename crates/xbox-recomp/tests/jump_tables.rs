use xbox_formats::fixtures::xbe_com_codigo;
use xbox_recomp::analysis::analyze;
fn image(body: &[u8], readonly: bool) -> Vec<u8> {
    let mut code = body.to_vec();
    code.resize(0x60, 0xCC);
    for at in [0x30, 0x40, 0x50] {
        code[at] = 0xC3;
    }
    let mut bytes = xbe_com_codigo(&code).unwrap();
    if readonly {
        bytes[0x338..0x33C].copy_from_slice(&0u32.to_le_bytes());
    }
    for (at, pc) in [(0x2020, 0x11040u32), (0x2024, 0x11050)] {
        bytes[at..at + 4].copy_from_slice(&pc.to_le_bytes());
    }
    bytes
}
fn guarded(extra: &[u8], branch: u8) -> Vec<u8> {
    let mut code = vec![0x89, 0x44, 0x24, 0x1C, 0x83, 0xE8, 1]; // save index; SUB eax,1
    code.extend([branch, 0x27]);
    code.extend(extra);
    code.extend([0x8B, 0x44, 0x24, 0x1C, 0xFF, 0x24, 0x85, 0x20, 0x20, 1, 0]);
    code
}
fn cases(bytes: &[u8]) -> [bool; 2] {
    let p = analyze(bytes).unwrap();
    [
        p.blocks.contains_key(&0x11040),
        p.blocks.contains_key(&0x11050),
    ]
}
#[test]
fn sub_ja_limita_indice_recuperado_da_pilha_e_descobre_tabela_readonly() {
    let bytes = image(&guarded(&[], 0x77), true);
    assert_eq!(cases(&bytes), [true, true]);
    let p = analyze(&bytes).unwrap();
    assert!(!p.instructions.contains_key(&0x11020));
}
#[test]
fn cmp_ja_e_indice_em_registrador_tem_mesma_prova_sem_slot_de_pilha() {
    let code = [
        0x83, 0xF8, 1, 0x77, 0x2B, 0xFF, 0x24, 0x85, 0x20, 0x20, 1, 0,
    ];
    assert_eq!(cases(&image(&code, true)), [true, true]);
}
#[test]
fn sem_guarda_signed_ou_tabela_gravavel_nao_autorizam_destinos() {
    assert_eq!(cases(&image(&guarded(&[], 0x77), false)), [false, false]);
    let mut no_guard = guarded(&[], 0x77);
    no_guard[7] = 0x90;
    no_guard[8] = 0x90;
    assert_eq!(cases(&image(&no_guard, true)), [false, false]);
    assert_eq!(cases(&image(&guarded(&[], 0x7F), true)), [false, false]);
}
#[test]
fn sobrescrita_do_slot_ou_de_parte_dele_invalida_alias_do_indice_protegido() {
    for overwrite in [&[0x89, 0x4C, 0x24, 0x1C][..], &[0x88, 0x4C, 0x24, 0x1D][..]] {
        assert_eq!(
            cases(&image(&guarded(overwrite, 0x77), true)),
            [false, false]
        );
    }
}
#[test]
fn flags_alteradas_e_join_com_caminho_sem_guarda_nao_emprestam_limite() {
    let code = [
        0x83, 0xF8, 1, 0x83, 0xC2, 0, 0x77, 0x28, 0xFF, 0x24, 0x85, 0x20, 0x20, 1, 0,
    ];
    assert_eq!(cases(&image(&code, true)), [false, false]);
    let code = [
        0x85, 0xC9, 0x74, 5, 0x83, 0xF8, 1, 0x77, 0x27, 0xFF, 0x24, 0x85, 0x20, 0x20, 1, 0,
    ];
    assert_eq!(cases(&image(&code, true)), [false, false]);
}
#[test]
fn limite_de_tabela_e_bytes_fora_da_secao_recusam_descoberta_parcial() {
    let mut code = guarded(&[], 0x77);
    code[6] = 16;
    let bytes = image(&code, true);
    assert_eq!(cases(&bytes), [false, false]);
    let mut bytes = image(&guarded(&[], 0x77), true);
    bytes[0x344..0x348].copy_from_slice(&0x26u32.to_le_bytes());
    assert_eq!(cases(&bytes), [false, false]);
}

fn walk_image(boolean: bool) -> Vec<u8> {
    let mut code = vec![
        0x55, 0x89, 0xE5, 0x83, 0xEC, 0x10, 0x8D, 0x05, 0x20, 0x20, 1, 0, 0x89, 0x45, 0xFC,
    ];
    let head = code.len();
    let upper;
    let branch;
    if boolean {
        code.extend([
            0x31, 0xC0, 0x8D, 0x0D, 0x28, 0x20, 1, 0, 0x39, 0x4D, 0xFC, 0x88, 0x45, 0xF7,
        ]);
        upper = code.len();
        code.extend([0x73, 0]);
        code.extend([0x83, 0x7D, 0xF8, 0, 0x0F, 0x94, 0xC0, 0x88, 0x45, 0xF7]);
        let join = code.len();
        code[upper + 1] = (join - upper - 2) as u8;
        code.extend([0x8A, 0x45, 0xF7, 0xA8, 1, 0x75, 2]);
        branch = code.len();
        code.extend([0xEB, 0]);
    } else {
        code.extend([0x8D, 0x05, 0x28, 0x20, 1, 0, 0x39, 0x45, 0xFC]);
        upper = code.len();
        code.extend([0x73, 0]);
        branch = upper;
    }
    code.extend([0x8B, 0x45, 0xFC, 0xFF, 0x10]);
    code.extend([0x8B, 0x45, 0xFC, 0x83, 0xC0, 4, 0x89, 0x45, 0xFC, 0xEB]);
    let next = code.len() + 1;
    code.push((head as u8).wrapping_sub(next as u8));
    code[branch + 1] = (0x70 - branch - 2) as u8;
    code.resize(0xA0, 0xCC);
    code[0x70] = 0xC9;
    code[0x71] = 0xC3;
    code[0x80] = 0xC3;
    code[0x90] = 0xC3;
    let mut bytes = xbe_com_codigo(&code).unwrap();
    bytes[0x338..0x33C].copy_from_slice(&0u32.to_le_bytes());
    bytes[0x2020..0x2024].copy_from_slice(&0x11080u32.to_le_bytes());
    bytes[0x2024..0x2028].copy_from_slice(&0x11090u32.to_le_bytes());
    bytes
}
#[test]
fn cursor_local_com_inicio_fim_passo_e_guarda_direta_ou_booleana_descobre_callbacks() {
    for boolean in [false, true] {
        let p = analyze(&walk_image(boolean)).unwrap();
        assert!(p.blocks.contains_key(&0x11080));
        assert!(p.blocks.contains_key(&0x11090));
    }
}
#[test]
fn cursor_sem_passo_canonico_guarda_unsigned_ou_readonly_nao_autoriza_tabela() {
    for variant in 0..3 {
        let mut bytes = walk_image(false);
        match variant {
            0 => {
                let at = bytes.windows(3).position(|v| v == [0x83, 0xC0, 4]).unwrap();
                bytes[at + 2] = 8;
            }
            1 => {
                let at = bytes[0x1000..0x1070]
                    .iter()
                    .position(|&v| v == 0x73)
                    .unwrap()
                    + 0x1000;
                bytes[at] = 0x7D;
            }
            _ => bytes[0x338..0x33C].copy_from_slice(&1u32.to_le_bytes()),
        }
        let p = analyze(&bytes).unwrap();
        assert!(!p.blocks.contains_key(&0x11080));
        assert!(!p.blocks.contains_key(&0x11090));
    }
}
#[test]
fn callback_escapado_pode_ser_traduzido_mas_ram_gravavel_continua_dinamica() {
    let code = [
        0x8D, 0x05, 0x40, 0x10, 1, 0, 0xA3, 0, 0x20, 1, 0, 0x8B, 0x01, 0xFF, 0xD0, 0xC3,
    ];
    let p = analyze(&image(&code, false)).unwrap();
    assert!(p.blocks.contains_key(&0x11040));
    let no_escape = [
        0x8D, 0x05, 0x40, 0x10, 1, 0, 0xA1, 0x04, 0x20, 1, 0, 0xFF, 0xD0, 0xC3,
    ];
    assert!(
        !analyze(&image(&no_escape, false))
            .unwrap()
            .blocks
            .contains_key(&0x11040)
    );
}

#[test]
fn escapes_para_enderecos_absolutos_distintos_nao_autorizam_alias_falso() {
    let code = [
        0x8D, 0x05, 0x40, 0x10, 1, 0, 0xA3, 0, 0x20, 1, 0, 0xA1, 4, 0x20, 1, 0, 0xFF, 0xD0, 0xC3,
    ];
    assert!(
        !analyze(&image(&code, false))
            .unwrap()
            .blocks
            .contains_key(&0x11040)
    );
}

#[test]
fn callback_carregado_de_memoria_sobrevive_movss_entre_carga_e_chamada() {
    let code = [
        0xC7, 0x05, 0x20, 0x20, 1, 0, 0x40, 0x10, 1, 0, // endereço escapa para RAM.
        0x8B, 0x01, // callback dinâmico vindo de [ecx].
        0xF3, 0x0F, 0x10, 0x05, 0x24, 0x20, 1, 0, // movss xmm0,[0x12024].
        0xF3, 0x0F, 0x11, 0x04, 0x24, // movss [esp],xmm0.
        0xFF, 0xD0, 0xC3,
    ];
    assert!(
        analyze(&image(&code, false))
            .unwrap()
            .blocks
            .contains_key(&0x11040)
    );
}

#[test]
fn conversao_sse_que_sobrescreve_gpr_invalida_candidato_de_callback() {
    let code = [
        0xC7, 0x05, 0x20, 0x20, 1, 0, 0x40, 0x10, 1, 0, 0x8B, 0x01, 0xF3, 0x0F, 0x2C,
        0xC0, // cvttss2si eax,xmm0 escreve eax.
        0xFF, 0xD0, 0xC3,
    ];
    assert!(
        !analyze(&image(&code, false))
            .unwrap()
            .blocks
            .contains_key(&0x11040)
    );
}
