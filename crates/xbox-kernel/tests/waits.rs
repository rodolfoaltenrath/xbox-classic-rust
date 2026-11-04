//! Motor de esperas: `NtWaitForSingleObjectEx`, `NtWaitForMultipleObjectsEx`, timeouts
//! relativos e o registro da espera pendente (ADR-0099).
use xbox_core::time::Clock;
use xbox_core::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_formats::fixtures::xbe_com_imports;
use xbox_kernel::dispatch::{
    Dispatcher, Erro, MAX_WAIT_OBJECTS, STATUS_ACCESS_VIOLATION, STATUS_INVALID_HANDLE,
    STATUS_SUCCESS, STATUS_TIMEOUT, STATUS_WAIT_0,
};
use xbox_kernel::imports;
use xbox_kernel::loader::{Image, STACK_BASE, STACK_SIZE};
use xbox_mem::{Bus, GuestVirt};

const STATUS_INVALID_PARAMETER: u32 = 0xC000_000D;
const CREATE_EVENT: u32 = 189;
const SET_EVENT: u32 = 225;
const PULSE_EVENT: u32 = 205;
const CLOSE: u32 = 187;
const CREATE_SEMAPHORE: u32 = 193;
const WAIT: u32 = 233;
const WAIT_EX: u32 = 234;
const WAIT_MULTIPLE: u32 = 235;
const NOTIFICATION: u32 = 0;
const SYNCHRONIZATION: u32 = 1;
const WAIT_ALL: u32 = 0;
const WAIT_ANY: u32 = 1;
const SAIDA: u32 = 0x12100;
const HANDLES: u32 = 0x12200;
const TIMEOUT: u32 = 0x12300;
/// Aponta para um `LARGE_INTEGER` zero: o timeout "não espere".
const POLL: u32 = 0x12310;

fn setup() -> (Bus, Dispatcher, [u32; 8]) {
    let image = xbe_com_imports(
        &[0xC3],
        &[
            CREATE_EVENT,
            SET_EVENT,
            PULSE_EVENT,
            CLOSE,
            CREATE_SEMAPHORE,
            222,
            WAIT,
            WAIT_EX,
            WAIT_MULTIPLE,
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

/// Chama um serviço; `relogio` ≠ `None` usa o caminho escalonado com esse Clock.
fn chamar(
    bus: &mut Bus,
    d: &mut Dispatcher,
    regs: &[u32; 8],
    ordinal: u32,
    args: &[u32],
    relogio: Option<u64>,
) -> Result<u32, Erro> {
    bus.escrever_u32(GuestVirt(regs[4]), 0x11000).unwrap();
    for (i, arg) in args.iter().enumerate() {
        bus.escrever_u32(GuestVirt(regs[4] + 4 + i as u32 * 4), *arg)
            .unwrap();
    }
    let pc = imports::trampoline(ordinal);
    match relogio {
        Some(ciclos) => {
            let mut clock = Clock::new();
            clock.set(ciclos);
            d.atender_escalonado(bus, pc, regs, &clock)
        }
        None => d.atender(bus, pc, regs),
    }
    .map(|r| r.eax)
}

fn evento(bus: &mut Bus, d: &mut Dispatcher, regs: &[u32; 8], kind: u32, signaled: u32) -> u32 {
    assert_eq!(
        chamar(
            bus,
            d,
            regs,
            CREATE_EVENT,
            &[SAIDA, 0, kind, signaled],
            None
        )
        .unwrap(),
        STATUS_SUCCESS
    );
    bus.ler_u32(GuestVirt(SAIDA)).unwrap()
}

fn semaforo(bus: &mut Bus, d: &mut Dispatcher, regs: &[u32; 8], initial: u32) -> u32 {
    assert_eq!(
        chamar(
            bus,
            d,
            regs,
            CREATE_SEMAPHORE,
            &[SAIDA, 0, initial, 5],
            None
        )
        .unwrap(),
        STATUS_SUCCESS
    );
    bus.ler_u32(GuestVirt(SAIDA)).unwrap()
}

fn lista(bus: &mut Bus, handles: &[u32]) {
    for (i, handle) in handles.iter().enumerate() {
        bus.escrever_u32(GuestVirt(HANDLES + i as u32 * 4), *handle)
            .unwrap();
    }
}

fn intervalo(bus: &mut Bus, valor: i64) {
    bus.escrever_u64(GuestVirt(TIMEOUT), valor.cast_unsigned())
        .unwrap();
}

/// `NtWaitForMultipleObjectsEx(Count, Handles, WaitType, WaitMode, Alertable, Timeout)`.
fn espera_multipla(
    bus: &mut Bus,
    d: &mut Dispatcher,
    regs: &[u32; 8],
    handles: &[u32],
    tipo: u32,
    timeout: u32,
    relogio: Option<u64>,
) -> Result<u32, Erro> {
    lista(bus, handles);
    // Nos testes, `0` quer dizer "não espere" (aponta para um LARGE_INTEGER zero); o timeout
    // NULL (infinito) é exercitado pelas chamadas diretas a `chamar`.
    let timeout = if timeout == 0 { POLL } else { timeout };
    chamar(
        bus,
        d,
        regs,
        WAIT_MULTIPLE,
        &[handles.len() as u32, HANDLES, tipo, 1, 0, timeout],
        relogio,
    )
}

fn secao(d: &Dispatcher, tag: [u8; 4]) -> Vec<u8> {
    let mut out = SnapshotWriter::new();
    d.save(&mut out);
    let bytes = out.finish();
    let at = bytes.windows(4).rposition(|w| w == tag).unwrap();
    bytes[at..].to_vec()
}

#[test]
fn parametros_invalidos_sao_recusados_sem_consumir_nada() {
    let (mut bus, mut d, regs) = setup();
    let e = evento(&mut bus, &mut d, &regs, SYNCHRONIZATION, 1);
    lista(&mut bus, &[e]);
    for (contagem, tipo, modo) in [
        (0, 1, 1),
        (MAX_WAIT_OBJECTS + 1, 1, 1),
        (1, 2, 1),
        (1, 1, 2),
    ] {
        assert_eq!(
            chamar(
                &mut bus,
                &mut d,
                &regs,
                WAIT_MULTIPLE,
                &[contagem, HANDLES, tipo, modo, 0, 0],
                None
            )
            .unwrap(),
            STATUS_INVALID_PARAMETER,
            "contagem {contagem}, tipo {tipo}, modo {modo}"
        );
    }
    // Lista de handles fora da RAM.
    assert_eq!(
        chamar(
            &mut bus,
            &mut d,
            &regs,
            WAIT_MULTIPLE,
            &[1, 0x10, 1, 1, 0, 0],
            None
        )
        .unwrap(),
        STATUS_ACCESS_VIOLATION
    );
    // Ponteiro de timeout fora da RAM.
    assert_eq!(
        espera_multipla(&mut bus, &mut d, &regs, &[e], WAIT_ANY, 0x10, None).unwrap(),
        STATUS_ACCESS_VIOLATION
    );
    // Handle inexistente na lista.
    assert_eq!(
        espera_multipla(&mut bus, &mut d, &regs, &[e, 0x7777], WAIT_ANY, POLL, None).unwrap(),
        STATUS_INVALID_HANDLE
    );
    // Nada disso consumiu o evento de sincronização sinalizado.
    assert_eq!(
        espera_multipla(&mut bus, &mut d, &regs, &[e], WAIT_ANY, POLL, None).unwrap(),
        STATUS_WAIT_0
    );
}

#[test]
fn alertavel_absoluto_e_repetidos_sao_variantes_recusadas() {
    let (mut bus, mut d, regs) = setup();
    let a = evento(&mut bus, &mut d, &regs, NOTIFICATION, 1);
    let b = evento(&mut bus, &mut d, &regs, NOTIFICATION, 1);
    lista(&mut bus, &[a, b]);
    let recusa = |erro: Result<u32, Erro>, trecho: &str| {
        let erro = erro.unwrap_err();
        assert!(
            matches!(erro, Erro::VarianteNaoImplementada { .. }),
            "{erro}"
        );
        assert!(erro.to_string().contains(trecho), "{erro}");
    };
    recusa(
        chamar(
            &mut bus,
            &mut d,
            &regs,
            WAIT_MULTIPLE,
            &[2, HANDLES, 1, 1, 1, 0],
            None,
        ),
        "alertável",
    );
    intervalo(&mut bus, 5); // positivo = absoluto
    recusa(
        chamar(
            &mut bus,
            &mut d,
            &regs,
            WAIT_MULTIPLE,
            &[2, HANDLES, 1, 1, 0, TIMEOUT],
            None,
        ),
        "timeout absoluto",
    );
    recusa(
        espera_multipla(&mut bus, &mut d, &regs, &[a, a], WAIT_ANY, POLL, None),
        "repetidos",
    );
}

#[test]
fn espera_por_qualquer_devolve_o_indice_e_consome_so_o_satisfeito() {
    let (mut bus, mut d, regs) = setup();
    let livre = evento(&mut bus, &mut d, &regs, SYNCHRONIZATION, 0);
    let primeiro = evento(&mut bus, &mut d, &regs, SYNCHRONIZATION, 1);
    let segundo = evento(&mut bus, &mut d, &regs, SYNCHRONIZATION, 1);
    // O primeiro sinalizado da lista vence; o outro continua sinalizado.
    assert_eq!(
        espera_multipla(
            &mut bus,
            &mut d,
            &regs,
            &[livre, primeiro, segundo],
            WAIT_ANY,
            0,
            None
        )
        .unwrap(),
        STATUS_WAIT_0 + 1
    );
    assert_eq!(
        espera_multipla(
            &mut bus,
            &mut d,
            &regs,
            &[livre, primeiro, segundo],
            WAIT_ANY,
            0,
            None
        )
        .unwrap(),
        STATUS_WAIT_0 + 2,
        "o primeiro foi consumido; agora é o segundo"
    );
    assert_eq!(
        espera_multipla(
            &mut bus,
            &mut d,
            &regs,
            &[livre, primeiro, segundo],
            WAIT_ANY,
            0,
            None
        )
        .unwrap(),
        STATUS_TIMEOUT
    );
}

#[test]
fn espera_por_todos_so_consome_quando_todos_estao_prontos() {
    let (mut bus, mut d, regs) = setup();
    let pronto = evento(&mut bus, &mut d, &regs, SYNCHRONIZATION, 1);
    let sem = semaforo(&mut bus, &mut d, &regs, 0);
    assert_eq!(
        espera_multipla(
            &mut bus,
            &mut d,
            &regs,
            &[pronto, sem],
            WAIT_ALL,
            POLL,
            None
        )
        .unwrap(),
        STATUS_TIMEOUT
    );
    assert_eq!(d.semaphore(sem), Some((0, 5)), "o semáforo não foi tocado");
    // A tentativa que falhou também não consumiu o evento de sincronização.
    assert_eq!(
        espera_multipla(&mut bus, &mut d, &regs, &[pronto], WAIT_ALL, POLL, None).unwrap(),
        STATUS_WAIT_0
    );
    // Com os dois prontos, a espera consome os dois de uma vez.
    let pronto = evento(&mut bus, &mut d, &regs, SYNCHRONIZATION, 1);
    assert_eq!(
        chamar(&mut bus, &mut d, &regs, 222, &[sem, 2, 0], None).unwrap(),
        STATUS_SUCCESS
    );
    assert_eq!(
        espera_multipla(
            &mut bus,
            &mut d,
            &regs,
            &[pronto, sem],
            WAIT_ALL,
            POLL,
            None
        )
        .unwrap(),
        STATUS_WAIT_0
    );
    assert_eq!(d.semaphore(sem), Some((1, 5)), "um tique consumido");
    assert_eq!(
        espera_multipla(&mut bus, &mut d, &regs, &[pronto], WAIT_ANY, POLL, None).unwrap(),
        STATUS_TIMEOUT,
        "o evento de sincronização foi consumido junto"
    );
}

#[test]
fn zero_nao_espera_e_o_timeout_relativo_exige_o_scheduler() {
    let (mut bus, mut d, regs) = setup();
    let e = evento(&mut bus, &mut d, &regs, NOTIFICATION, 0);
    intervalo(&mut bus, 0);
    assert_eq!(
        chamar(&mut bus, &mut d, &regs, WAIT_EX, &[e, 1, 0, TIMEOUT], None).unwrap(),
        STATUS_TIMEOUT,
        "poll devolve TIMEOUT sem bloquear, até sem scheduler"
    );
    assert!(!d.take_block_request());
    intervalo(&mut bus, -10_000);
    let erro = chamar(&mut bus, &mut d, &regs, WAIT_EX, &[e, 1, 0, TIMEOUT], None).unwrap_err();
    assert!(
        erro.to_string().contains("timeout exige o scheduler"),
        "{erro}"
    );
}

#[test]
fn prazo_nasce_na_primeira_tentativa_e_a_repeticao_nao_o_reinicia() {
    let (mut bus, mut d, regs) = setup();
    let e = evento(&mut bus, &mut d, &regs, NOTIFICATION, 0);
    intervalo(&mut bus, -10_000); // 1 ms
    let um_ms = Clock::hundred_ns_to_cycles(10_000).unwrap();
    let t0 = 1_000;
    // Primeira tentativa: bloqueia com o prazo t0 + 1 ms.
    chamar(
        &mut bus,
        &mut d,
        &regs,
        WAIT_EX,
        &[e, 1, 0, TIMEOUT],
        Some(t0),
    )
    .unwrap();
    assert!(d.take_block_request());
    assert_eq!(d.take_block_deadline(), Some(t0 + um_ms));
    // Repetição no meio do intervalo: continua bloqueada, com o MESMO prazo.
    chamar(
        &mut bus,
        &mut d,
        &regs,
        WAIT_EX,
        &[e, 1, 0, TIMEOUT],
        Some(t0 + um_ms / 2),
    )
    .unwrap();
    assert!(d.take_block_request());
    assert_eq!(
        d.take_block_deadline(),
        Some(t0 + um_ms),
        "o prazo não recomeçou"
    );
    // No prazo: TIMEOUT, e a espera pendente some.
    assert_eq!(
        chamar(
            &mut bus,
            &mut d,
            &regs,
            WAIT_EX,
            &[e, 1, 0, TIMEOUT],
            Some(t0 + um_ms)
        )
        .unwrap(),
        STATUS_TIMEOUT
    );
    assert!(!d.take_block_request());
    // Uma espera nova depois do timeout calcula um prazo novo.
    chamar(
        &mut bus,
        &mut d,
        &regs,
        WAIT_EX,
        &[e, 1, 0, TIMEOUT],
        Some(t0 + 10 * um_ms),
    )
    .unwrap();
    assert_eq!(d.take_block_deadline(), Some(t0 + 11 * um_ms));
}

#[test]
fn sinalizar_antes_do_prazo_satisfaz_a_repeticao() {
    let (mut bus, mut d, regs) = setup();
    let e = evento(&mut bus, &mut d, &regs, SYNCHRONIZATION, 0);
    intervalo(&mut bus, -10_000);
    chamar(
        &mut bus,
        &mut d,
        &regs,
        WAIT_EX,
        &[e, 1, 0, TIMEOUT],
        Some(0),
    )
    .unwrap();
    assert!(d.take_block_request());
    d.take_block_deadline();
    chamar(&mut bus, &mut d, &regs, SET_EVENT, &[e, 0], None).unwrap();
    assert_eq!(
        chamar(
            &mut bus,
            &mut d,
            &regs,
            WAIT_EX,
            &[e, 1, 0, TIMEOUT],
            Some(5)
        )
        .unwrap(),
        STATUS_WAIT_0
    );
    assert!(!d.take_block_request(), "satisfeita: nada a bloquear");
    // O evento de sincronização foi consumido: uma espera nova, sem sinal, bloqueia de
    // novo com um prazo novo (a pendência anterior terminou).
    chamar(
        &mut bus,
        &mut d,
        &regs,
        WAIT_EX,
        &[e, 1, 0, TIMEOUT],
        Some(6),
    )
    .unwrap();
    assert!(d.take_block_request());
    let um_ms = Clock::hundred_ns_to_cycles(10_000).unwrap();
    assert_eq!(d.take_block_deadline(), Some(6 + um_ms));
}

#[test]
fn pulso_libera_a_espera_por_qualquer_entre_varios_eventos() {
    let (mut bus, mut d, regs) = setup();
    let a = evento(&mut bus, &mut d, &regs, NOTIFICATION, 0);
    let b = evento(&mut bus, &mut d, &regs, NOTIFICATION, 0);
    // Ambos os eventos entram como esperados quando a espera bloqueia.
    intervalo(&mut bus, -1_000_000);
    espera_multipla(&mut bus, &mut d, &regs, &[a, b], WAIT_ANY, TIMEOUT, Some(0)).unwrap();
    assert!(d.take_block_request());
    d.take_block_deadline();
    // O pulso em `b` libera a espera sem deixar o evento sinalizado.
    chamar(&mut bus, &mut d, &regs, PULSE_EVENT, &[b, 0], None).unwrap();
    assert_eq!(
        espera_multipla(&mut bus, &mut d, &regs, &[a, b], WAIT_ANY, TIMEOUT, Some(7)).unwrap(),
        STATUS_WAIT_0 + 1
    );
    intervalo(&mut bus, 0);
    assert_eq!(
        espera_multipla(&mut bus, &mut d, &regs, &[a, b], WAIT_ANY, TIMEOUT, Some(8)).unwrap(),
        STATUS_TIMEOUT,
        "o pulso não deixou nada sinalizado"
    );
}

#[test]
fn fechar_um_evento_esperado_invalida_a_repeticao() {
    let (mut bus, mut d, regs) = setup();
    let a = evento(&mut bus, &mut d, &regs, NOTIFICATION, 0);
    intervalo(&mut bus, -1_000_000);
    chamar(
        &mut bus,
        &mut d,
        &regs,
        WAIT_EX,
        &[a, 1, 0, TIMEOUT],
        Some(0),
    )
    .unwrap();
    assert!(d.take_block_request());
    d.take_block_deadline();
    chamar(&mut bus, &mut d, &regs, CLOSE, &[a], None).unwrap();
    assert_eq!(
        chamar(
            &mut bus,
            &mut d,
            &regs,
            WAIT_EX,
            &[a, 1, 0, TIMEOUT],
            Some(1)
        )
        .unwrap(),
        STATUS_INVALID_HANDLE
    );
    assert!(!d.take_block_request());
}

#[test]
fn snapshot_preserva_a_espera_pendente_e_migra_kevt_v2_e_krnl_v16() {
    let (mut bus, mut d, regs) = setup();
    let a = evento(&mut bus, &mut d, &regs, NOTIFICATION, 0);
    intervalo(&mut bus, -1_000_000);
    chamar(
        &mut bus,
        &mut d,
        &regs,
        WAIT_EX,
        &[a, 1, 0, TIMEOUT],
        Some(0),
    )
    .unwrap();
    assert!(d.take_block_request());
    let prazo = d.take_block_deadline().unwrap();
    let mut out = SnapshotWriter::new();
    d.save(&mut out);
    let bytes = out.finish();

    let mut restaurado = Dispatcher::new();
    Snapshot::load(&mut restaurado, &mut SnapshotReader::new(&bytes).unwrap()).unwrap();
    let wait = secao(&restaurado, *b"WAIT");
    assert_eq!(wait, secao(&d, *b"WAIT"));
    assert_eq!(&wait[9..13], &1u32.to_le_bytes(), "thread 1");
    assert_eq!(wait[13], 1, "tem prazo");
    assert_eq!(&wait[14..22], &prazo.to_le_bytes());
    // A repetição no restaurado ainda respeita o mesmo prazo.
    chamar(
        &mut bus,
        &mut restaurado,
        &regs,
        WAIT_EX,
        &[a, 1, 0, TIMEOUT],
        Some(prazo - 1),
    )
    .unwrap();
    assert_eq!(restaurado.take_block_deadline(), Some(prazo), "mesmo prazo");
    assert_eq!(
        chamar(
            &mut bus,
            &mut restaurado,
            &regs,
            WAIT_EX,
            &[a, 1, 0, TIMEOUT],
            Some(prazo)
        )
        .unwrap(),
        STATUS_TIMEOUT
    );

    // KEVT v2 (um evento esperado, sem liberação) migra para a lista de um.
    let kevt = bytes.windows(4).rposition(|w| w == b"KEVT").unwrap();
    let mut v2 = bytes.clone();
    // v3: tag, versão, n, n × (handle, tipo, sinalizado), contagem, esperados, liberado.
    let n = u32::from_le_bytes(v2[kevt + 8..kevt + 12].try_into().unwrap()) as usize;
    let objetos = kevt + 12 + n * 9;
    assert_eq!(
        &v2[objetos..objetos + 4],
        &1u32.to_le_bytes(),
        "um esperado"
    );
    let esperado = u32::from_le_bytes(v2[objetos + 4..objetos + 8].try_into().unwrap());
    v2.splice(
        objetos..objetos + 12,
        esperado.to_le_bytes().into_iter().chain([0]),
    );
    v2[kevt + 4..kevt + 8].copy_from_slice(&2u32.to_le_bytes());
    let mut migrado = Dispatcher::new();
    Snapshot::load(&mut migrado, &mut SnapshotReader::new(&v2).unwrap()).unwrap();
    assert_eq!(secao(&migrado, *b"KEVT"), secao(&d, *b"KEVT"));

    // KRNL v16: sem WAIT; migra para nenhuma espera pendente.
    let wait_at = bytes.windows(4).rposition(|w| w == b"WAIT").unwrap();
    let mut v16 = bytes.clone();
    v16.truncate(wait_at);
    let krnl = v16.windows(4).position(|w| w == b"KRNL").unwrap();
    v16[krnl + 4..krnl + 8].copy_from_slice(&16u32.to_le_bytes());
    let mut sem_espera = Dispatcher::new();
    Snapshot::load(&mut sem_espera, &mut SnapshotReader::new(&v16).unwrap()).unwrap();
    assert_eq!(
        secao(&sem_espera, *b"WAIT").len(),
        9,
        "WAIT regravada sem pendência"
    );
}
