//! Janela virtual de MMIO do NV2A (ADR-0051).
//!
//! O contrato: a janela existe como identidade sobre o físico, é RW e nunca executável,
//! o bus **registra** o que o guest toca, e nada vaza para a RAM. O dispositivo não tem
//! modelo — escritas são descartadas e leituras devolvem zero como placeholder declarado.

use xbox_formats::fixtures::xbe_com_codigo;
use xbox_kernel::loader::Image;
use xbox_kernel::loader::mmio::{NV2A_BASE, NV2A_SIZE};
use xbox_mem::{Acesso, Bus, Dispositivo, GuestPhys, GuestVirt, Operacao};

fn bus() -> Bus {
    Image::prepare_aot(&xbe_com_codigo(&[0xC3]).unwrap())
        .unwrap()
        .image
        .bus
}

#[test]
fn janela_e_identidade_sobre_o_fisico_em_toda_a_extensao() {
    let bus = bus();
    // Os dois extremos e um ponto de cada quarto de 4 MiB: identidade em tudo.
    for va in [
        NV2A_BASE,
        NV2A_BASE + 0x0040_0000,
        NV2A_BASE + 0x0080_0000,
        NV2A_BASE + 0x00C0_0000,
        NV2A_BASE + NV2A_SIZE - 4,
        // O registrador que o `XVideoSetMode` do nxdk escreve primeiro.
        0xFD68_13C8,
    ] {
        let pa = bus
            .traduzir(GuestVirt(va), Acesso::Leitura)
            .unwrap_or_else(|e| panic!("0x{va:08X} sem mapeamento: {e}"));
        assert_eq!(pa, GuestPhys(va), "virtual e físico divergem em 0x{va:08X}");
    }
}

#[test]
fn janela_nao_vaza_para_alem_dos_limites() {
    let bus = bus();
    // Um byte antes e um depois da janela continuam sem mapeamento: nenhum alias
    // acidental com a flash (0xFF000000) nem com o resto do espaço.
    assert!(
        bus.traduzir(GuestVirt(NV2A_BASE - 1), Acesso::Leitura)
            .is_err()
    );
    assert!(
        bus.traduzir(GuestVirt(NV2A_BASE + NV2A_SIZE), Acesso::Leitura)
            .is_err()
    );
}

#[test]
fn janela_e_leitura_e_escrita_mas_nunca_executavel() {
    let bus = bus();
    let reg = GuestVirt(0xFD68_13C8);
    assert!(bus.traduzir(reg, Acesso::Leitura).is_ok());
    assert!(bus.traduzir(reg, Acesso::Escrita).is_ok());
    // Registrador não é código: executar ali tem de ser falta de página.
    let erro = bus.traduzir(reg, Acesso::Execucao).unwrap_err();
    assert!(erro.e_falha_de_pagina(), "{erro}");
}

#[test]
fn escrita_em_registrador_e_registrada_e_descartada_sem_tocar_a_ram() {
    let mut bus = bus();
    let antes = bus.arena().bytes().to_vec();

    bus.escrever_u8(GuestVirt(0xFD68_13C8), 0).unwrap();
    for _ in 0..768 {
        bus.escrever_u8(GuestVirt(0xFD68_13C9), 0xAB).unwrap();
    }

    // A telemetria diz exatamente o que o guest fez: um registrador de índice e uma
    // porta de dados escrita 768 vezes — a rampa de gamma de 256 entradas RGB.
    let registros = bus.telemetria().registros();
    assert_eq!(registros.len(), 2, "{}", bus.telemetria().relatorio());
    let dados = registros
        .iter()
        .find(|r| r.endereco == GuestPhys(0xFD68_13C9))
        .expect("porta de dados registrada");
    assert_eq!(dados.ocorrencias, 768);
    assert_eq!(dados.operacao, Operacao::Escrita);
    assert_eq!(dados.tamanho, 1);
    assert_eq!(dados.primeiro_valor, Some(0xAB));

    // Nada disso pode ter caído na RAM do guest.
    assert_eq!(bus.arena().bytes(), &antes[..], "MMIO vazou para a arena");
}

#[test]
fn leitura_de_registrador_devolve_zero_declarado_e_e_registrada() {
    let bus = bus();
    // Zero é placeholder declarado, não comportamento de hardware; o que importa é que
    // a leitura não falha e que fica registrada, para a Fase 5 saber que foi lida.
    assert_eq!(bus.ler_u32(GuestVirt(0xFD60_0100)).unwrap(), 0);
    let registros = bus.telemetria().registros();
    assert_eq!(registros.len(), 1);
    assert_eq!(registros[0].operacao, Operacao::Leitura);
    assert_eq!(registros[0].tamanho, 4);
}

#[test]
fn telemetria_atribui_o_registrador_ao_dispositivo_certo() {
    let mut bus = bus();
    bus.escrever_u32(GuestVirt(0xFD00_0140), 1).unwrap(); // PMC_INTR_EN_0
    let r = &bus.telemetria().registros()[0];
    assert_eq!(
        r.motivo,
        xbox_mem::Motivo::NaoImplementado(Dispositivo::Nv2a),
        "o relatório precisa dizer qual dispositivo foi tocado"
    );
}

#[test]
fn acesso_que_cruza_o_fim_da_janela_falha_em_vez_de_vazar_para_a_flash() {
    let mut bus = bus();
    // A flash começa em 0xFF000000; a janela termina em 0xFE000000. Um u32 que cruza o
    // fim da janela toca a página seguinte, que não está mapeada.
    let erro = bus
        .escrever_u32(GuestVirt(NV2A_BASE + NV2A_SIZE - 2), 0)
        .unwrap_err();
    assert!(erro.e_falha_de_pagina(), "{erro}");
    // E a falha não pode ter deixado metade do store registrada como escrita.
    assert!(
        bus.telemetria().registros().is_empty(),
        "store parcial deixou rastro: {}",
        bus.telemetria().relatorio()
    );
}

#[test]
fn imagem_nao_pode_ocupar_a_janela_de_mmio() {
    // A reserva vale para qualquer imagem: seção em 0xFD000000 sobreporia hardware.
    let mut bytes = xbe_com_codigo(&[0xC3]).unwrap();
    bytes[0x300 + 0x04..0x300 + 0x08].copy_from_slice(&0xFD68_0000u32.to_le_bytes());
    let erro = Image::prepare_aot(&bytes).unwrap_err().to_string();
    assert!(
        erro.contains("reservada") || erro.contains("fora da imagem") || erro.contains("sobrepos"),
        "mensagem inesperada: {erro}"
    );
}
