//! Eventos do kernel: criação, sinalização, espera sem bloqueio e ciclo de vida.
use xbox_core::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_formats::fixtures::xbe_com_imports;
use xbox_kernel::{
    dispatch::{
        Dispatcher, Erro, ORDINAL_NT_CLEAR_EVENT, ORDINAL_NT_CLOSE, ORDINAL_NT_CREATE_EVENT,
        ORDINAL_NT_CREATE_MUTANT, ORDINAL_NT_PULSE_EVENT, ORDINAL_NT_SET_EVENT,
        ORDINAL_NT_WAIT_FOR_SINGLE_OBJECT, STATUS_ACCESS_VIOLATION, STATUS_INSUFFICIENT_RESOURCES,
        STATUS_INVALID_HANDLE, STATUS_SUCCESS, STATUS_WAIT_0,
    },
    imports,
    loader::{Image, STACK_BASE, STACK_SIZE},
};
use xbox_mem::{Bus, GuestVirt};

const STATUS_INVALID_PARAMETER: u32 = 0xC000_000D;
const NOTIFICATION: u32 = 0;
const SYNCHRONIZATION: u32 = 1;
/// Onde o guest "guarda" o handle e o estado anterior: pilha, que é RAM gravável.
const SAIDA: u32 = STACK_BASE + STACK_SIZE - 0x800;
const ANTERIOR: u32 = SAIDA + 0x10;

fn setup() -> (Bus, Dispatcher, [u32; 8]) {
    let image = xbe_com_imports(
        &[0xC3],
        &[
            ORDINAL_NT_CREATE_EVENT,
            ORDINAL_NT_SET_EVENT,
            ORDINAL_NT_CLEAR_EVENT,
            ORDINAL_NT_PULSE_EVENT,
            ORDINAL_NT_CLOSE,
            ORDINAL_NT_WAIT_FOR_SINGLE_OBJECT,
            ORDINAL_NT_CREATE_MUTANT,
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

/// Cria um evento e devolve o handle que o guest recebeu.
fn criar(bus: &mut Bus, d: &mut Dispatcher, regs: &[u32; 8], kind: u32, signaled: u32) -> u32 {
    let status = chamar(
        bus,
        d,
        regs,
        ORDINAL_NT_CREATE_EVENT,
        &[SAIDA, 0, kind, signaled],
    )
    .unwrap();
    assert_eq!(status, STATUS_SUCCESS);
    bus.ler_u32(GuestVirt(SAIDA)).unwrap()
}

fn esperar(bus: &mut Bus, d: &mut Dispatcher, regs: &[u32; 8], handle: u32) -> Result<u32, Erro> {
    chamar(
        bus,
        d,
        regs,
        ORDINAL_NT_WAIT_FOR_SINGLE_OBJECT,
        &[handle, 0, 0],
    )
}

fn salvar(d: &Dispatcher) -> Vec<u8> {
    let mut out = SnapshotWriter::new();
    d.save(&mut out);
    out.finish()
}

#[test]
fn evento_de_notificacao_sinalizado_satisfaz_esperas_repetidas() {
    let (mut bus, mut d, regs) = setup();
    let h = criar(&mut bus, &mut d, &regs, NOTIFICATION, 1);
    assert_ne!(h, 0);
    assert_eq!(esperar(&mut bus, &mut d, &regs, h).unwrap(), STATUS_WAIT_0);
    assert_eq!(
        esperar(&mut bus, &mut d, &regs, h).unwrap(),
        STATUS_WAIT_0,
        "notificação não é limpa pela espera"
    );
}

#[test]
fn evento_de_sincronizacao_e_limpo_pela_espera_satisfeita() {
    let (mut bus, mut d, regs) = setup();
    let h = criar(&mut bus, &mut d, &regs, SYNCHRONIZATION, 1);
    assert_eq!(esperar(&mut bus, &mut d, &regs, h).unwrap(), STATUS_WAIT_0);
    assert!(
        matches!(
            esperar(&mut bus, &mut d, &regs, h),
            Err(Erro::VarianteNaoImplementada { .. })
        ),
        "a segunda espera encontra o evento limpo"
    );
}

#[test]
fn esperar_evento_nao_sinalizado_e_erro_identificavel_sem_alterar_o_estado() {
    let (mut bus, mut d, regs) = setup();
    let h = criar(&mut bus, &mut d, &regs, SYNCHRONIZATION, 0);
    let erro = esperar(&mut bus, &mut d, &regs, h).unwrap_err();
    let texto = erro.to_string();
    assert!(
        matches!(erro, Erro::VarianteNaoImplementada { .. }),
        "{texto}"
    );
    assert!(texto.contains("scheduler ou interrupção"), "{texto}");
    // Sinalizar e esperar de novo tem de funcionar como se nada tivesse acontecido.
    chamar(&mut bus, &mut d, &regs, ORDINAL_NT_SET_EVENT, &[h, 0]).unwrap();
    assert_eq!(esperar(&mut bus, &mut d, &regs, h).unwrap(), STATUS_WAIT_0);
}

#[test]
fn set_clear_e_pulse_devolvem_o_estado_anterior_na_memoria_guest() {
    let (mut bus, mut d, regs) = setup();
    let h = criar(&mut bus, &mut d, &regs, NOTIFICATION, 0);
    let anterior = |bus: &Bus| bus.ler_u32(GuestVirt(ANTERIOR)).unwrap();

    bus.escrever_u32(GuestVirt(ANTERIOR), 0xAAAA_AAAA).unwrap();
    chamar(
        &mut bus,
        &mut d,
        &regs,
        ORDINAL_NT_SET_EVENT,
        &[h, ANTERIOR],
    )
    .unwrap();
    assert_eq!(anterior(&bus), 0, "estava limpo");
    chamar(
        &mut bus,
        &mut d,
        &regs,
        ORDINAL_NT_SET_EVENT,
        &[h, ANTERIOR],
    )
    .unwrap();
    assert_eq!(anterior(&bus), 1, "já estava sinalizado");

    // O pulso, sem threads esperando, deixa o evento limpo e relata o estado anterior.
    chamar(
        &mut bus,
        &mut d,
        &regs,
        ORDINAL_NT_PULSE_EVENT,
        &[h, ANTERIOR],
    )
    .unwrap();
    assert_eq!(anterior(&bus), 1);
    assert!(
        esperar(&mut bus, &mut d, &regs, h).is_err(),
        "pulso deixa limpo"
    );

    chamar(&mut bus, &mut d, &regs, ORDINAL_NT_SET_EVENT, &[h, 0]).unwrap();
    assert_eq!(
        chamar(&mut bus, &mut d, &regs, ORDINAL_NT_CLEAR_EVENT, &[h]).unwrap(),
        STATUS_SUCCESS
    );
    assert!(esperar(&mut bus, &mut d, &regs, h).is_err(), "clear limpa");
}

#[test]
fn handle_invalido_e_ponteiro_ruim_sao_recusados_sem_efeito() {
    let (mut bus, mut d, regs) = setup();
    for ordinal in [ORDINAL_NT_SET_EVENT, ORDINAL_NT_PULSE_EVENT] {
        assert_eq!(
            chamar(&mut bus, &mut d, &regs, ordinal, &[0x1234, 0]).unwrap(),
            STATUS_INVALID_HANDLE
        );
    }
    assert_eq!(
        chamar(&mut bus, &mut d, &regs, ORDINAL_NT_CLEAR_EVENT, &[0x1234]).unwrap(),
        STATUS_INVALID_HANDLE
    );

    let h = criar(&mut bus, &mut d, &regs, NOTIFICATION, 0);
    // PreviousState em endereço não mapeado: falha antes de mudar o evento.
    assert_eq!(
        chamar(&mut bus, &mut d, &regs, ORDINAL_NT_SET_EVENT, &[h, 0x10]).unwrap(),
        STATUS_ACCESS_VIOLATION
    );
    assert!(
        esperar(&mut bus, &mut d, &regs, h).is_err(),
        "continua limpo"
    );
}

#[test]
fn criacao_invalida_nao_consome_handle_nem_publica_objeto() {
    let (mut bus, mut d, regs) = setup();
    let primeiro = criar(&mut bus, &mut d, &regs, NOTIFICATION, 0);

    // Tipo desconhecido.
    assert_eq!(
        chamar(
            &mut bus,
            &mut d,
            &regs,
            ORDINAL_NT_CREATE_EVENT,
            &[SAIDA, 0, 2, 0]
        )
        .unwrap(),
        STATUS_INVALID_PARAMETER
    );
    // Saída fora da RAM.
    assert_eq!(
        chamar(
            &mut bus,
            &mut d,
            &regs,
            ORDINAL_NT_CREATE_EVENT,
            &[0x10, 0, 0, 0]
        )
        .unwrap(),
        STATUS_ACCESS_VIOLATION
    );
    // OBJECT_ATTRIBUTES com nome: variante recusada.
    bus.escrever_bytes(GuestVirt(ANTERIOR + 0x20), &[1; 12])
        .unwrap();
    assert!(matches!(
        chamar(
            &mut bus,
            &mut d,
            &regs,
            ORDINAL_NT_CREATE_EVENT,
            &[SAIDA, ANTERIOR + 0x20, 0, 0]
        ),
        Err(Erro::VarianteNaoImplementada { .. })
    ));
    let segundo = criar(&mut bus, &mut d, &regs, NOTIFICATION, 0);
    assert_eq!(segundo, primeiro + 4, "as recusas não consumiram handles");
}

#[test]
fn close_remove_o_evento_e_o_handle_deixa_de_valer() {
    let (mut bus, mut d, regs) = setup();
    let h = criar(&mut bus, &mut d, &regs, NOTIFICATION, 1);
    assert_eq!(
        chamar(&mut bus, &mut d, &regs, ORDINAL_NT_CLOSE, &[h]).unwrap(),
        STATUS_SUCCESS
    );
    assert_eq!(
        esperar(&mut bus, &mut d, &regs, h).unwrap(),
        STATUS_INVALID_HANDLE
    );
    assert_eq!(
        chamar(&mut bus, &mut d, &regs, ORDINAL_NT_SET_EVENT, &[h, 0]).unwrap(),
        STATUS_INVALID_HANDLE
    );
}

#[test]
fn eventos_e_mutants_compartilham_o_contador_sem_colidir() {
    let (mut bus, mut d, regs) = setup();
    let evento = criar(&mut bus, &mut d, &regs, NOTIFICATION, 1);
    chamar(
        &mut bus,
        &mut d,
        &regs,
        ORDINAL_NT_CREATE_MUTANT,
        &[SAIDA, 0, 0],
    )
    .unwrap();
    let mutante = bus.ler_u32(GuestVirt(SAIDA)).unwrap();
    let outro_evento = criar(&mut bus, &mut d, &regs, NOTIFICATION, 0);
    let mut handles = [evento, mutante, outro_evento];
    handles.sort_unstable();
    assert!(handles.windows(2).all(|w| w[0] < w[1]), "{handles:?}");
    // Cada um responde pelo próprio tipo.
    assert_eq!(
        esperar(&mut bus, &mut d, &regs, evento).unwrap(),
        STATUS_WAIT_0
    );
    assert_eq!(
        esperar(&mut bus, &mut d, &regs, mutante).unwrap(),
        STATUS_WAIT_0
    );
}

#[test]
fn limite_de_eventos_devolve_recursos_insuficientes() {
    let (mut bus, mut d, regs) = setup();
    for _ in 0..4096 {
        criar(&mut bus, &mut d, &regs, NOTIFICATION, 0);
    }
    assert_eq!(
        chamar(
            &mut bus,
            &mut d,
            &regs,
            ORDINAL_NT_CREATE_EVENT,
            &[SAIDA, 0, 0, 0]
        )
        .unwrap(),
        STATUS_INSUFFICIENT_RESOURCES
    );
}

#[test]
fn snapshot_preserva_eventos_e_migra_versoes_antigas_para_tabela_vazia() {
    let (mut bus, mut d, regs) = setup();
    let sinalizado = criar(&mut bus, &mut d, &regs, NOTIFICATION, 1);
    let limpo = criar(&mut bus, &mut d, &regs, SYNCHRONIZATION, 0);
    let bytes = salvar(&d);

    let mut restaurado = Dispatcher::new();
    Snapshot::load(&mut restaurado, &mut SnapshotReader::new(&bytes).unwrap()).unwrap();
    assert_eq!(salvar(&restaurado), bytes, "ida e volta idêntica");
    assert_eq!(
        esperar(&mut bus, &mut restaurado, &regs, sinalizado).unwrap(),
        STATUS_WAIT_0
    );
    assert!(esperar(&mut bus, &mut restaurado, &regs, limpo).is_err());
}

#[test]
fn snapshot_v10_sem_eventos_migra_para_tabela_vazia() {
    let d = Dispatcher::new();
    let mut bytes = salvar(&d);
    // Um v10 é o v11 sem a seção `KEVT` final (tag, versão e contagem: 12 bytes), com a
    // versão do contêiner KRNL em 10. A versão fica logo depois da tag da primeira seção.
    assert_eq!(&bytes[12..16], b"KRNL");
    let corpo = bytes.len() - 12;
    assert_eq!(&bytes[corpo..corpo + 4], b"KEVT");
    bytes.truncate(corpo);
    bytes[16..20].copy_from_slice(&10u32.to_le_bytes());

    let mut migrado = Dispatcher::new();
    Snapshot::load(&mut migrado, &mut SnapshotReader::new(&bytes).unwrap()).unwrap();
    assert_eq!(salvar(&migrado), salvar(&Dispatcher::new()));
}
