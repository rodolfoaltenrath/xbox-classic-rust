//! `KeDelayExecutionThread`: conversão do intervalo, recusas e o pedido de tempo.
//!
//! O serviço **não avança relógio nenhum**: ele valida e deixa um pedido que a sessão
//! consome (ADR-0051). Estes testes fixam o que o pedido contém e que cada recusa não
//! deixa pedido pendente — um pedido órfão faria o atendimento seguinte avançar o tempo
//! por um intervalo que ninguém pediu.

use xbox_core::time::CPU_HZ;
use xbox_formats::fixtures::xbe_com_imports;
use xbox_kernel::dispatch::{
    Dispatcher, Erro, ORDINAL_KE_DELAY_EXECUTION_THREAD, PedidoDeTempo, STATUS_SUCCESS,
};
use xbox_kernel::imports;
use xbox_kernel::loader::{Image, STACK_BASE, STACK_SIZE};
use xbox_mem::{Bus, GuestVirt};

/// Onde o teste guarda o `LARGE_INTEGER` do intervalo (RAM da imagem, gravável).
const INTERVALO: u32 = 0x0001_2200;

fn setup() -> (Bus, Dispatcher, [u32; 8]) {
    let image = xbe_com_imports(&[0xC3], &[ORDINAL_KE_DELAY_EXECUTION_THREAD]).unwrap();
    let mut bus = Image::prepare_aot(&image).unwrap().image.bus;
    let mut regs = [0; 8];
    regs[4] = STACK_BASE + STACK_SIZE - 0x100;
    bus.escrever_u32(GuestVirt(regs[4]), 0x0001_1000).unwrap();
    (bus, Dispatcher::new(), regs)
}

/// Escreve `(WaitMode, Alertable, &Interval)` e o `Interval` em memória guest.
fn preparar(bus: &mut Bus, regs: &[u32; 8], wait_mode: u32, alertable: u32, interval: i64) {
    bus.escrever_u32(GuestVirt(regs[4] + 4), wait_mode).unwrap();
    bus.escrever_u32(GuestVirt(regs[4] + 8), alertable).unwrap();
    bus.escrever_u32(GuestVirt(regs[4] + 12), INTERVALO)
        .unwrap();
    bus.escrever_bytes(GuestVirt(INTERVALO), &interval.to_le_bytes())
        .unwrap();
}

fn atender(bus: &mut Bus, d: &mut Dispatcher, regs: &[u32; 8]) -> Result<u32, Erro> {
    d.atender(
        bus,
        imports::trampoline(ORDINAL_KE_DELAY_EXECUTION_THREAD),
        regs,
    )
    .map(|r| r.eax)
}

#[test]
fn sleep_de_dois_segundos_pede_dois_segundos_de_ciclos() {
    // É o que o `SleepEx` do nxdk produz: UserMode, não alertável, ms * -10000.
    let (mut bus, mut d, regs) = setup();
    preparar(&mut bus, &regs, 1, 0, -20_000_000);

    assert_eq!(atender(&mut bus, &mut d, &regs).unwrap(), STATUS_SUCCESS);
    assert_eq!(
        d.take_time_request(),
        Some(PedidoDeTempo { ciclos: 2 * CPU_HZ }),
        "2000 ms são exatamente 2 s de ciclos do Clock"
    );
}

#[test]
fn pedido_e_consumido_uma_vez_so() {
    let (mut bus, mut d, regs) = setup();
    preparar(&mut bus, &regs, 1, 0, -10_000);
    atender(&mut bus, &mut d, &regs).unwrap();

    assert!(d.take_time_request().is_some());
    assert_eq!(
        d.take_time_request(),
        None,
        "um pedido já consumido não pode reaparecer e avançar o relógio duas vezes"
    );
}

#[test]
fn kernel_mode_e_user_mode_sao_aceitos_e_outro_valor_e_invalido() {
    for modo in [0u32, 1] {
        let (mut bus, mut d, regs) = setup();
        preparar(&mut bus, &regs, modo, 0, -10_000);
        assert_eq!(atender(&mut bus, &mut d, &regs).unwrap(), STATUS_SUCCESS);
    }
    for modo in [2u32, 7, 0xFFFF_FFFF] {
        let (mut bus, mut d, regs) = setup();
        preparar(&mut bus, &regs, modo, 0, -10_000);
        let erro = atender(&mut bus, &mut d, &regs).unwrap_err();
        assert!(matches!(erro, Erro::ArgumentoInvalido { .. }), "{erro:?}");
        assert_eq!(d.take_time_request(), None, "recusa deixou pedido pendente");
    }
}

#[test]
fn espera_alertavel_recusa_como_variante_sem_pedido() {
    // Alertável pode terminar em STATUS_ALERTED/STATUS_USER_APC. Devolver SUCCESS
    // esconderia essa diferença, e APCs não existem.
    let (mut bus, mut d, regs) = setup();
    preparar(&mut bus, &regs, 1, 1, -10_000);
    let erro = atender(&mut bus, &mut d, &regs).unwrap_err();
    assert!(
        matches!(erro, Erro::VarianteNaoImplementada { .. }),
        "{erro:?}"
    );
    assert!(erro.to_string().contains("alertável"), "{erro}");
    assert_eq!(d.take_time_request(), None);

    // BOOLEAN usa só o byte baixo: 0x100 é FALSE e deve ser aceito.
    let (mut bus, mut d, regs) = setup();
    preparar(&mut bus, &regs, 1, 0x100, -10_000);
    assert_eq!(atender(&mut bus, &mut d, &regs).unwrap(), STATUS_SUCCESS);
}

#[test]
fn intervalo_zero_ou_positivo_recusa_porque_nao_e_relativo() {
    // Positivo é tempo absoluto; zero seria um yield. Nenhum dos dois é o que o Sleep do
    // nxdk produz, e escolher um significado seria inventar semântica.
    for valor in [0i64, 1, 10_000_000, i64::MAX] {
        let (mut bus, mut d, regs) = setup();
        preparar(&mut bus, &regs, 1, 0, valor);
        let erro = atender(&mut bus, &mut d, &regs).unwrap_err();
        assert!(
            matches!(erro, Erro::VarianteNaoImplementada { .. }),
            "intervalo {valor}: {erro:?}"
        );
        assert_eq!(
            d.take_time_request(),
            None,
            "intervalo {valor} deixou pedido"
        );
    }
}

#[test]
fn intervalo_negativo_enorme_recusa_em_vez_de_truncar_para_um_sono_curto() {
    // i64::MIN tem módulo 2^63, que não cabe em ciclos de u64: precisa recusar. O erro a
    // evitar é a conversão truncar e devolver um sono de poucos ciclos.
    for valor in [i64::MIN, i64::MIN + 1, -(i64::MAX / 2)] {
        let (mut bus, mut d, regs) = setup();
        preparar(&mut bus, &regs, 1, 0, valor);
        let erro = atender(&mut bus, &mut d, &regs).unwrap_err();
        assert!(
            matches!(erro, Erro::ArgumentoInvalido { .. }),
            "{valor}: {erro:?}"
        );
        assert_eq!(d.take_time_request(), None);
    }
}

#[test]
fn ponteiro_de_intervalo_invalido_recusa_sem_pedido() {
    // Nulo, página não mapeada e MMIO não são "zero de espera".
    for ponteiro in [0u32, 0x5000_0000, 0xFD00_0000] {
        let (mut bus, mut d, regs) = setup();
        bus.escrever_u32(GuestVirt(regs[4] + 4), 1).unwrap();
        bus.escrever_u32(GuestVirt(regs[4] + 8), 0).unwrap();
        bus.escrever_u32(GuestVirt(regs[4] + 12), ponteiro).unwrap();
        let erro = atender(&mut bus, &mut d, &regs).unwrap_err();
        assert!(
            matches!(erro, Erro::ArgumentoInvalido { .. }),
            "ponteiro 0x{ponteiro:08X}: {erro:?}"
        );
        assert_eq!(d.take_time_request(), None);
    }
}

#[test]
fn intervalo_que_cruza_pagina_e_aceito_se_toda_a_faixa_e_valida() {
    // `LARGE_INTEGER` desalinhado atravessando duas páginas mapeadas é legítimo no x86.
    let (mut bus, mut d, regs) = setup();
    // A pilha é RAM mapeada em faixa contígua; a fixture da imagem só mapeia uma página
    // de `.rdata`. 0x7000_9FFC cruza 0x7000_9000 / 0x7000_A000, ambas mapeadas.
    let cruzando = STACK_BASE + 0x9FFC;
    bus.escrever_u32(GuestVirt(regs[4] + 4), 1).unwrap();
    bus.escrever_u32(GuestVirt(regs[4] + 8), 0).unwrap();
    bus.escrever_u32(GuestVirt(regs[4] + 12), cruzando).unwrap();
    bus.escrever_bytes(GuestVirt(cruzando), &(-10_000i64).to_le_bytes())
        .unwrap();
    assert_eq!(atender(&mut bus, &mut d, &regs).unwrap(), STATUS_SUCCESS);
    assert!(d.take_time_request().is_some());
}

/// Salva o dispatcher e descarta o prefixo `KRNL`, que contém os contadores
/// `atendidas`/`recusadas` e **deve** mudar numa recusa. O que sobra são as seções dos
/// serviços (mutants, EEPROM, tempo, IRQL, memória virtual, threads, vídeo...).
fn estado_dos_servicos(d: &Dispatcher) -> Vec<u8> {
    let mut out = xbox_core::snapshot::SnapshotWriter::new();
    xbox_core::snapshot::Snapshot::save(d, &mut out);
    let bytes = out.finish();
    // Cabeçalho do container (12 bytes) + "KRNL" + versão (u32) + dois contadores (u64).
    bytes[12 + 4 + 4 + 8 + 8..].to_vec()
}

#[test]
fn recusa_muda_so_os_contadores_e_nao_o_estado_dos_servicos() {
    let (mut bus, mut d, regs) = setup();
    preparar(&mut bus, &regs, 1, 1, -10_000);
    let antes = estado_dos_servicos(&d);
    let recusadas_antes = d.recusadas();

    assert!(atender(&mut bus, &mut d, &regs).is_err());

    assert_eq!(
        d.recusadas(),
        recusadas_antes + 1,
        "a recusa tem de ser contada"
    );
    assert_eq!(
        estado_dos_servicos(&d),
        antes,
        "uma recusa não pode alterar o estado de nenhum serviço"
    );
}

#[test]
fn sucesso_tambem_nao_altera_o_estado_dos_servicos() {
    // A espera só deixa um pedido transiente: nada de serviço é modificado, e o pedido
    // não entra no snapshot (a sessão o aplica antes de qualquer ponto de salvamento).
    let (mut bus, mut d, regs) = setup();
    preparar(&mut bus, &regs, 1, 0, -20_000_000);
    let antes = estado_dos_servicos(&d);

    atender(&mut bus, &mut d, &regs).unwrap();

    assert!(d.take_time_request().is_some());
    assert_eq!(estado_dos_servicos(&d), antes);
}

#[test]
fn carregar_snapshot_nao_ressuscita_pedido_de_tempo() {
    // Um pedido pendente descartado ao carregar: reaplicá-lo avançaria o relógio por um
    // intervalo que já foi aplicado, ou descartado, na sessão de origem.
    let (mut bus, mut d, regs) = setup();
    preparar(&mut bus, &regs, 1, 0, -20_000_000);
    atender(&mut bus, &mut d, &regs).unwrap();
    // O pedido está pendente; salvar e carregar num dispatcher novo.
    let mut out = xbox_core::snapshot::SnapshotWriter::new();
    xbox_core::snapshot::Snapshot::save(&d, &mut out);
    let bytes = out.finish();

    let mut novo = Dispatcher::new();
    xbox_core::snapshot::Snapshot::load(
        &mut novo,
        &mut xbox_core::snapshot::SnapshotReader::new(&bytes).unwrap(),
    )
    .unwrap();
    assert_eq!(novo.take_time_request(), None);
}
