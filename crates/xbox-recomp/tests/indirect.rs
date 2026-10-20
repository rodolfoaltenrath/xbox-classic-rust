use xbox_formats::fixtures::xbe_com_codigo;
use xbox_recomp::Error;
use xbox_recomp::analysis::{analyze, analyze_with_entries};

#[test]
fn constantes_locais_e_ponteiros_readonly_descobrem_rotinas_sem_varrer_dados() {
    for setup in [
        vec![0xB8, 0x10, 0x10, 1, 0],             // mov eax,0x11010
        vec![0xB9, 0x10, 0x10, 1, 0, 0x89, 0xC8], // mov ecx,target; mov eax,ecx
        vec![0x8D, 0x05, 0x10, 0x10, 1, 0],       // lea eax,[target]
    ] {
        let mut code = setup;
        code.extend_from_slice(&[0xFF, 0xD0, 0xC3]);
        code.resize(16, 0xCC);
        code.push(0xC3);
        let p = analyze(&xbe_com_codigo(&code).unwrap()).unwrap();
        assert!(p.blocks.contains_key(&0x11010));
        assert!(!p.instructions.values().any(|i| i.pc == 0x1100F));
    }
    for memory in [
        vec![0xFF, 0x15, 0x20, 0x20, 1, 0, 0xC3],       // call [abs]
        vec![0xA1, 0x20, 0x20, 1, 0, 0xFF, 0xD0, 0xC3], // mov eax,[abs]; call eax
        vec![0xBB, 0x20, 0x20, 1, 0, 0xFF, 0x13, 0xC3], // mov ebx,abs; call [ebx]
    ] {
        let mut code = memory;
        code.resize(16, 0xCC);
        code.push(0xC3);
        let mut bytes = xbe_com_codigo(&code).unwrap();
        bytes[0x2020..0x2024].copy_from_slice(&0x11010u32.to_le_bytes());
        // Dados graváveis não autorizam descoberta pelo valor inicial.
        assert!(!analyze(&bytes).unwrap().blocks.contains_key(&0x11010));
        bytes[0x338..0x33C].copy_from_slice(&0u32.to_le_bytes());
        assert!(analyze(&bytes).unwrap().blocks.contains_key(&0x11010));
    }
}

#[test]
fn inferencia_nao_preserva_constantes_por_call_join_ou_escrita_parcial() {
    for body in [
        vec![0xB8, 0x20, 0x10, 1, 0, 0xB4, 0, 0xFF, 0xD0, 0xC3],
        vec![0xB8, 0x20, 0x10, 1, 0, 0xEB, 0, 0xFF, 0xD0, 0xC3],
        vec![
            0xB8, 0x20, 0x10, 1, 0, 0xE8, 1, 0, 0, 0, 0xC3, 0xFF, 0xD0, 0xC3,
        ],
    ] {
        let mut code = body;
        code.resize(32, 0xCC);
        code.push(0xC3);
        assert!(
            !analyze(&xbe_com_codigo(&code).unwrap())
                .unwrap()
                .blocks
                .contains_key(&0x11020)
        );
    }
}

#[test]
fn entradas_adicionais_sao_validadas_e_jump_indireto_nao_tem_fallthrough() {
    let code = [0xFF, 0xE0, 0x0F, 0x0B, 0xC3];
    let bytes = xbe_com_codigo(&code).unwrap();
    assert_eq!(analyze(&bytes).unwrap().instructions.len(), 1);
    let p = analyze_with_entries(&bytes, &[0x11004, 0x11004]).unwrap();
    assert_eq!(p.instructions.len(), 2);
    for target in [0x11001, 0x11002, 0x12000, 0x70000000, 0xFFFFFFFF] {
        assert!(analyze_with_entries(&bytes, &[target]).is_err());
    }
    assert!(analyze_with_entries(&bytes, &vec![0x11000; 65_537]).is_err());
}

#[test]
fn descoberta_iterativa_acha_cadeias_e_divide_destino_ja_decodificado() {
    let code = [
        0xB8, 8, 0x10, 1, 0, 0xFF, 0xE0, 0xCC, 0xB8, 16, 0x10, 1, 0, 0xFF, 0xE0, 0xCC, 0xC3,
    ];
    let p = analyze(&xbe_com_codigo(&code).unwrap()).unwrap();
    assert!(p.blocks.contains_key(&0x11008));
    assert!(p.blocks.contains_key(&0x11010));
    assert!(p.instructions.keys().all(|pc| p.blocks.contains_key(pc)));
    // Zero informação dinâmica ainda admite todos os inícios já decodificados.
    let bytes = xbe_com_codigo(&[0x90, 0xFF, 0xE0]).unwrap();
    let p = analyze(&bytes).unwrap();
    assert_eq!(p.blocks.len(), 2);
}

#[test]
fn ponteiros_parciais_headers_e_padding_nao_sao_fontes_de_descoberta() {
    for address in [0x1203Cu32, 0x1203E, 0x12040, 0x10100, 0xFFFFFFFE] {
        let mut code = vec![0xFF, 0x15];
        code.extend_from_slice(&address.to_le_bytes());
        code.push(0xC3);
        code.resize(16, 0xCC);
        code.push(0xC3);
        let mut image = xbe_com_codigo(&code).unwrap();
        image[0x338..0x33C].copy_from_slice(&0u32.to_le_bytes());
        image[0x203C..0x2040].copy_from_slice(&0x11010u32.to_le_bytes());
        let p = analyze(&image).unwrap();
        assert_eq!(p.blocks.contains_key(&0x11010), address == 0x1203C);
    }
}

#[test]
fn indiretas_16_far_prefixadas_e_cadeias_excessivas_falham_explicitamente() {
    for code in [
        &[0x66, 0xFF, 0xD0, 0xC3][..],
        &[0x66, 0xFF, 0xE0][..],
        &[0xFF, 0x18, 0xC3][..],
        &[0xFF, 0x28][..],
        &[0x67, 0xFF, 0x10, 0xC3][..],
        &[0x64, 0xFF, 0x10, 0xC3][..],
        &[0xF3, 0xFF, 0xD0, 0xC3][..],
    ] {
        assert!(matches!(
            analyze(&xbe_com_codigo(code).unwrap()),
            Err(Error::Analysis { pc: 0x11000, .. })
        ));
    }
    let mut code = Vec::new();
    for index in 0..260u32 {
        code.push(0xB8);
        code.extend_from_slice(&(0x11000 + (index + 1) * 7).to_le_bytes());
        code.extend_from_slice(&[0xFF, 0xE0]);
    }
    code.push(0xC3);
    assert!(
        analyze(&xbe_com_codigo(&code).unwrap())
            .unwrap_err()
            .to_string()
            .contains("256 rodadas")
    );
}
