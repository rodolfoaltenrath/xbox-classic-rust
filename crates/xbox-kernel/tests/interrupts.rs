//! Serviços de interrupção, DPC e notificação de desligamento (ADR-0055).
use xbox_core::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_formats::fixtures::xbe_com_imports;
use xbox_kernel::{
    dispatch::{
        Dispatcher, Erro, ORDINAL_HAL_GET_INTERRUPT_VECTOR,
        ORDINAL_HAL_REGISTER_SHUTDOWN_NOTIFICATION, ORDINAL_KE_CONNECT_INTERRUPT,
        ORDINAL_KE_DISCONNECT_INTERRUPT, ORDINAL_KE_INITIALIZE_DPC,
        ORDINAL_KE_INITIALIZE_INTERRUPT, ORDINAL_KE_INSERT_QUEUE_DPC, ORDINAL_KE_REMOVE_QUEUE_DPC,
    },
    imports,
    loader::{Image, STACK_BASE, STACK_SIZE},
};
use xbox_mem::{Bus, GuestVirt};

/// Rotina "de serviço": qualquer endereço em `.text` da fixture (0x11000).
const ROTINA: u32 = 0x11000;
const KINT: u32 = STACK_BASE + STACK_SIZE - 0x1000;
const DPC: u32 = KINT + 0x100;
const SAIDA: u32 = KINT + 0x200;

fn setup() -> (Bus, Dispatcher, [u32; 8]) {
    let image = xbe_com_imports(
        &[0xC3],
        &[
            ORDINAL_HAL_GET_INTERRUPT_VECTOR,
            ORDINAL_HAL_REGISTER_SHUTDOWN_NOTIFICATION,
            ORDINAL_KE_CONNECT_INTERRUPT,
            ORDINAL_KE_DISCONNECT_INTERRUPT,
            ORDINAL_KE_INITIALIZE_DPC,
            ORDINAL_KE_INITIALIZE_INTERRUPT,
            ORDINAL_KE_INSERT_QUEUE_DPC,
            ORDINAL_KE_REMOVE_QUEUE_DPC,
        ],
    )
    .unwrap();
    let mut regs = [0; 8];
    regs[4] = STACK_BASE + STACK_SIZE - 0x100;
    (
        Image::prepare_aot(&image).unwrap().image.bus,
        Dispatcher::new(),
        regs,
    )
}

fn chamar(
    bus: &mut Bus,
    d: &mut Dispatcher,
    regs: &[u32; 8],
    ordinal: u32,
    argumentos: &[u32],
) -> Result<u32, Erro> {
    bus.escrever_u32(GuestVirt(regs[4]), 0x11000).unwrap();
    for (i, valor) in argumentos.iter().enumerate() {
        bus.escrever_u32(GuestVirt(regs[4] + 4 + i as u32 * 4), *valor)
            .unwrap();
    }
    d.atender(bus, imports::trampoline(ordinal), regs)
        .map(|r| r.eax)
}

fn inicializar_interrupcao(bus: &mut Bus, d: &mut Dispatcher, regs: &[u32; 8], vector: u32) {
    chamar(
        bus,
        d,
        regs,
        ORDINAL_KE_INITIALIZE_INTERRUPT,
        &[KINT, ROTINA, 0x15, vector, 24, 0, 1],
    )
    .unwrap();
}

/// Só a seção de interrupções (a última): o resto do snapshot tem contadores de chamadas.
fn secao_kint(bytes: &[u8]) -> Vec<u8> {
    let inicio = bytes.windows(4).rposition(|w| w == b"KINT").unwrap();
    bytes[inicio..].to_vec()
}

fn salvar(d: &Dispatcher) -> Vec<u8> {
    let mut out = SnapshotWriter::new();
    d.save(&mut out);
    out.finish()
}

#[test]
fn vetor_e_irql_seguem_a_convencao_declarada_e_o_nivel_e_validado() {
    let (mut bus, mut d, regs) = setup();
    assert_eq!(
        chamar(
            &mut bus,
            &mut d,
            &regs,
            ORDINAL_HAL_GET_INTERRUPT_VECTOR,
            &[3, SAIDA]
        )
        .unwrap(),
        0x33
    );
    assert_eq!(bus.ler_u8(GuestVirt(SAIDA)).unwrap(), 24);
    // Nível acima de 15 ou ponteiro fora da RAM: recusa, sem escrever.
    bus.escrever_u8(GuestVirt(SAIDA), 0xEE).unwrap();
    assert!(
        chamar(
            &mut bus,
            &mut d,
            &regs,
            ORDINAL_HAL_GET_INTERRUPT_VECTOR,
            &[16, SAIDA]
        )
        .is_err()
    );
    assert!(
        chamar(
            &mut bus,
            &mut d,
            &regs,
            ORDINAL_HAL_GET_INTERRUPT_VECTOR,
            &[3, 0x10]
        )
        .is_err()
    );
    assert_eq!(bus.ler_u8(GuestVirt(SAIDA)).unwrap(), 0xEE);
}

#[test]
fn inicializar_preenche_o_cabecalho_da_kinterrupt_sem_conectar() {
    let (mut bus, mut d, regs) = setup();
    inicializar_interrupcao(&mut bus, &mut d, &regs, 0x33);
    let u32_at = |bus: &Bus, off| bus.ler_u32(GuestVirt(KINT + off)).unwrap();
    assert_eq!(u32_at(&bus, 0), ROTINA);
    assert_eq!(u32_at(&bus, 4), 0x15);
    assert_eq!(u32_at(&bus, 8), 3, "BusInterruptLevel = vetor - 0x30");
    assert_eq!(u32_at(&bus, 12), 24);
    assert_eq!(
        bus.ler_u8(GuestVirt(KINT + 16)).unwrap(),
        0,
        "ainda desconectada"
    );
    assert_eq!(bus.ler_u8(GuestVirt(KINT + 17)).unwrap(), 1);
    assert_eq!(u32_at(&bus, 20), 0, "ServiceCount");
}

#[test]
fn vetor_fora_da_faixa_e_modo_invalido_sao_recusados_sem_escrever() {
    let (mut bus, mut d, regs) = setup();
    for argumentos in [
        [KINT, ROTINA, 0, 0x2F, 24, 0, 1],
        [KINT, ROTINA, 0, 0x40, 24, 0, 1],
        [KINT, ROTINA, 0, 0x33, 24, 2, 1],
        [0x10, ROTINA, 0, 0x33, 24, 0, 1],
    ] {
        assert!(matches!(
            chamar(
                &mut bus,
                &mut d,
                &regs,
                ORDINAL_KE_INITIALIZE_INTERRUPT,
                &argumentos
            ),
            Err(Erro::ArgumentoInvalido { .. })
        ));
    }
    assert_eq!(bus.ler_u32(GuestVirt(KINT)).unwrap(), 0, "nada foi escrito");
}

#[test]
fn conectar_e_desconectar_alteram_o_byte_connected_e_sao_idempotentes() {
    let (mut bus, mut d, regs) = setup();
    inicializar_interrupcao(&mut bus, &mut d, &regs, 0x33);
    let connected = |bus: &Bus| bus.ler_u8(GuestVirt(KINT + 16)).unwrap();
    assert_eq!(
        chamar(
            &mut bus,
            &mut d,
            &regs,
            ORDINAL_KE_CONNECT_INTERRUPT,
            &[KINT]
        )
        .unwrap(),
        1
    );
    assert_eq!(connected(&bus), 1);
    assert_eq!(
        chamar(
            &mut bus,
            &mut d,
            &regs,
            ORDINAL_KE_CONNECT_INTERRUPT,
            &[KINT]
        )
        .unwrap(),
        0,
        "já conectada"
    );
    assert_eq!(
        chamar(
            &mut bus,
            &mut d,
            &regs,
            ORDINAL_KE_DISCONNECT_INTERRUPT,
            &[KINT]
        )
        .unwrap(),
        1
    );
    assert_eq!(connected(&bus), 0);
    assert_eq!(
        chamar(
            &mut bus,
            &mut d,
            &regs,
            ORDINAL_KE_DISCONNECT_INTERRUPT,
            &[KINT]
        )
        .unwrap(),
        0,
        "já desconectada"
    );
}

#[test]
fn conectar_exige_objeto_inicializado_e_rotina_executavel() {
    let (mut bus, mut d, regs) = setup();
    // Memória zerada: BusInterruptLevel = 0 é válido, mas a rotina nula não é executável.
    assert!(matches!(
        chamar(
            &mut bus,
            &mut d,
            &regs,
            ORDINAL_KE_CONNECT_INTERRUPT,
            &[KINT]
        ),
        Err(Erro::ArgumentoInvalido { .. })
    ));
    // Rotina em dados (pilha, não executável).
    chamar(
        &mut bus,
        &mut d,
        &regs,
        ORDINAL_KE_INITIALIZE_INTERRUPT,
        &[KINT, KINT, 0, 0x33, 24, 0, 1],
    )
    .unwrap();
    assert!(matches!(
        chamar(
            &mut bus,
            &mut d,
            &regs,
            ORDINAL_KE_CONNECT_INTERRUPT,
            &[KINT]
        ),
        Err(Erro::ArgumentoInvalido { .. })
    ));
    assert_eq!(bus.ler_u8(GuestVirt(KINT + 16)).unwrap(), 0, "não conectou");
}

#[test]
fn dpc_insere_uma_vez_remove_e_reinsere() {
    let (mut bus, mut d, regs) = setup();
    chamar(
        &mut bus,
        &mut d,
        &regs,
        ORDINAL_KE_INITIALIZE_DPC,
        &[DPC, ROTINA, 0xD1],
    )
    .unwrap();
    assert_eq!(bus.ler_u16(GuestVirt(DPC)).unwrap(), 19);
    let inserted = |bus: &Bus| bus.ler_u8(GuestVirt(DPC + 2)).unwrap();
    assert_eq!(
        chamar(
            &mut bus,
            &mut d,
            &regs,
            ORDINAL_KE_INSERT_QUEUE_DPC,
            &[DPC, 0xA1, 0xA2]
        )
        .unwrap(),
        1
    );
    assert_eq!(inserted(&bus), 1);
    assert_eq!(bus.ler_u32(GuestVirt(DPC + 20)).unwrap(), 0xA1);
    assert_eq!(bus.ler_u32(GuestVirt(DPC + 24)).unwrap(), 0xA2);
    assert_eq!(
        chamar(
            &mut bus,
            &mut d,
            &regs,
            ORDINAL_KE_INSERT_QUEUE_DPC,
            &[DPC, 0xB1, 0xB2]
        )
        .unwrap(),
        0,
        "já na fila: FALSE, e os argumentos do primeiro ficam"
    );
    assert_eq!(bus.ler_u32(GuestVirt(DPC + 20)).unwrap(), 0xA1);
    assert!(d.has_queued_dpcs());

    assert_eq!(
        chamar(&mut bus, &mut d, &regs, ORDINAL_KE_REMOVE_QUEUE_DPC, &[DPC]).unwrap(),
        1
    );
    assert_eq!(inserted(&bus), 0);
    assert!(!d.has_queued_dpcs());
    assert_eq!(
        chamar(&mut bus, &mut d, &regs, ORDINAL_KE_REMOVE_QUEUE_DPC, &[DPC]).unwrap(),
        0
    );
    assert_eq!(
        chamar(
            &mut bus,
            &mut d,
            &regs,
            ORDINAL_KE_INSERT_QUEUE_DPC,
            &[DPC, 0, 0]
        )
        .unwrap(),
        1
    );
}

#[test]
fn dpc_nao_inicializado_e_recusado() {
    let (mut bus, mut d, regs) = setup();
    assert!(matches!(
        chamar(
            &mut bus,
            &mut d,
            &regs,
            ORDINAL_KE_INSERT_QUEUE_DPC,
            &[DPC, 0, 0]
        ),
        Err(Erro::ArgumentoInvalido { .. })
    ));
}

#[test]
fn next_dpc_marca_como_nao_enfileirado_antes_de_entregar_e_respeita_a_ordem() {
    let (mut bus, mut d, regs) = setup();
    let outro = DPC + 0x40;
    chamar(
        &mut bus,
        &mut d,
        &regs,
        ORDINAL_KE_INITIALIZE_DPC,
        &[DPC, ROTINA, 0xD1],
    )
    .unwrap();
    chamar(
        &mut bus,
        &mut d,
        &regs,
        ORDINAL_KE_INITIALIZE_DPC,
        &[outro, ROTINA, 0xD2],
    )
    .unwrap();
    chamar(
        &mut bus,
        &mut d,
        &regs,
        ORDINAL_KE_INSERT_QUEUE_DPC,
        &[DPC, 1, 2],
    )
    .unwrap();
    chamar(
        &mut bus,
        &mut d,
        &regs,
        ORDINAL_KE_INSERT_QUEUE_DPC,
        &[outro, 3, 4],
    )
    .unwrap();

    let primeiro = d.next_dpc(&mut bus).unwrap().unwrap();
    assert_eq!(primeiro.entry, ROTINA);
    assert_eq!(primeiro.arguments, [DPC, 0xD1, 1, 2]);
    assert_eq!(primeiro.count, 4);
    assert_eq!(
        bus.ler_u8(GuestVirt(DPC + 2)).unwrap(),
        0,
        "a rotina pode se reenfileirar"
    );
    let segundo = d.next_dpc(&mut bus).unwrap().unwrap();
    assert_eq!(segundo.arguments, [outro, 0xD2, 3, 4]);
    assert!(d.next_dpc(&mut bus).unwrap().is_none());
}

#[test]
fn notificacao_de_desligamento_registra_remove_e_desregistrar_sem_registro_e_inofensivo() {
    let (mut bus, mut d, regs) = setup();
    let registro = KINT + 0x300;
    bus.escrever_u32(GuestVirt(registro), ROTINA).unwrap();
    // O `pbkit` desregistra antes de ter registrado.
    assert_eq!(
        chamar(
            &mut bus,
            &mut d,
            &regs,
            ORDINAL_HAL_REGISTER_SHUTDOWN_NOTIFICATION,
            &[registro, 0]
        )
        .unwrap(),
        0
    );
    let vazio = secao_kint(&salvar(&d));
    chamar(
        &mut bus,
        &mut d,
        &regs,
        ORDINAL_HAL_REGISTER_SHUTDOWN_NOTIFICATION,
        &[registro, 1],
    )
    .unwrap();
    assert_ne!(secao_kint(&salvar(&d)), vazio, "registrou");
    chamar(
        &mut bus,
        &mut d,
        &regs,
        ORDINAL_HAL_REGISTER_SHUTDOWN_NOTIFICATION,
        &[registro, 0],
    )
    .unwrap();
    assert_eq!(secao_kint(&salvar(&d)), vazio, "removeu");
    // Rotina fora de memória executável: recusa.
    bus.escrever_u32(GuestVirt(registro), KINT).unwrap();
    assert!(
        chamar(
            &mut bus,
            &mut d,
            &regs,
            ORDINAL_HAL_REGISTER_SHUTDOWN_NOTIFICATION,
            &[registro, 1]
        )
        .is_err()
    );
}

#[test]
fn pending_interrupt_exige_linha_ligada_e_interrupcao_da_gpu_conectada() {
    let (mut bus, mut d, regs) = setup();
    assert!(d.pending_interrupt(&bus).is_none());
    inicializar_interrupcao(&mut bus, &mut d, &regs, 0x33);
    chamar(
        &mut bus,
        &mut d,
        &regs,
        ORDINAL_KE_CONNECT_INTERRUPT,
        &[KINT],
    )
    .unwrap();
    assert!(d.pending_interrupt(&bus).is_none(), "linha desligada");
    assert!(!d.interrupt_can_signal(&bus), "vblank não habilitado");

    bus.nv2a_irq_mut().write(0x0060_0140, 1);
    bus.nv2a_irq_mut().write(0x0000_0140, 1);
    assert!(d.interrupt_can_signal(&bus));
    assert!(
        d.pending_interrupt(&bus).is_none(),
        "habilitado, mas sem vblank"
    );
    bus.nv2a_irq_mut().raise_vblank();
    let chamada = d.pending_interrupt(&bus).unwrap();
    assert_eq!(chamada.entry, ROTINA);
    assert_eq!(chamada.arguments[..2], [KINT, 0x15]);
    assert_eq!(chamada.count, 2);

    // Outro nível de barramento não é a GPU.
    chamar(
        &mut bus,
        &mut d,
        &regs,
        ORDINAL_KE_DISCONNECT_INTERRUPT,
        &[KINT],
    )
    .unwrap();
    inicializar_interrupcao(&mut bus, &mut d, &regs, 0x35);
    chamar(
        &mut bus,
        &mut d,
        &regs,
        ORDINAL_KE_CONNECT_INTERRUPT,
        &[KINT],
    )
    .unwrap();
    assert!(d.pending_interrupt(&bus).is_none());
    assert!(!d.interrupt_can_signal(&bus));
}

#[test]
fn snapshot_preserva_interrupcoes_dpcs_e_confere_com_a_memoria_guest() {
    let (mut bus, mut d, regs) = setup();
    inicializar_interrupcao(&mut bus, &mut d, &regs, 0x33);
    chamar(
        &mut bus,
        &mut d,
        &regs,
        ORDINAL_KE_CONNECT_INTERRUPT,
        &[KINT],
    )
    .unwrap();
    chamar(
        &mut bus,
        &mut d,
        &regs,
        ORDINAL_KE_INITIALIZE_DPC,
        &[DPC, ROTINA, 0xD1],
    )
    .unwrap();
    chamar(
        &mut bus,
        &mut d,
        &regs,
        ORDINAL_KE_INSERT_QUEUE_DPC,
        &[DPC, 5, 6],
    )
    .unwrap();
    let bytes = salvar(&d);

    let mut restaurado = Dispatcher::new();
    Snapshot::load(&mut restaurado, &mut SnapshotReader::new(&bytes).unwrap()).unwrap();
    restaurado.restore_memory_context(None, &bus).unwrap();
    assert_eq!(salvar(&restaurado), bytes);
    assert!(restaurado.has_queued_dpcs());

    // Memória que diverge do snapshot (DPC marcado como não enfileirado) é recusada.
    bus.escrever_u8(GuestVirt(DPC + 2), 0).unwrap();
    let mut outro = Dispatcher::new();
    Snapshot::load(&mut outro, &mut SnapshotReader::new(&bytes).unwrap()).unwrap();
    assert!(outro.restore_memory_context(None, &bus).is_err());
}
