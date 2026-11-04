//! `NtSuspendThread`, `NtResumeThread`, `NtYieldExecution` e a criação suspensa (ADR-0098).
use xbox_core::time::Clock;
use xbox_core::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_formats::{Xbe, fixtures::xbe_com_imports};
use xbox_kernel::dispatch::{
    Dispatcher, Erro, MAX_SUSPEND_COUNT, ORDINAL_NT_RESUME_THREAD, ORDINAL_NT_SUSPEND_THREAD,
    ORDINAL_NT_YIELD_EXECUTION, STATUS_ACCESS_VIOLATION, STATUS_INVALID_HANDLE,
    STATUS_NO_YIELD_PERFORMED, STATUS_SUCCESS, STATUS_SUSPEND_COUNT_EXCEEDED,
    STATUS_THREAD_IS_TERMINATING,
};
use xbox_kernel::imports;
use xbox_kernel::loader::{Image, STACK_BASE, STACK_SIZE};
use xbox_mem::{Bus, GuestVirt};

const CREATE: u32 = 255;
const CLOSE: u32 = 187;
const SAIDA: u32 = 0x12100;
const ANTERIOR: u32 = 0x12200;

fn setup() -> (Bus, Dispatcher, [u32; 8]) {
    let bytes = xbe_com_imports(
        &[0xC3],
        &[
            CREATE,
            CLOSE,
            ORDINAL_NT_SUSPEND_THREAD,
            ORDINAL_NT_RESUME_THREAD,
            ORDINAL_NT_YIELD_EXECUTION,
        ],
    )
    .unwrap();
    let xbe = Xbe::parse(&bytes).unwrap();
    let d = Dispatcher::for_image(xbe.header.base_address, xbe.header.size_of_image).unwrap();
    let bus = Image::prepare_aot(&bytes).unwrap().image.bus;
    let mut regs = [0; 8];
    regs[4] = STACK_BASE + STACK_SIZE - 0x100;
    (bus, d, regs)
}

/// Cria a filha pelo caminho escalonado; `suspended` é o argumento `CreateSuspended`.
fn criar(bus: &mut Bus, d: &mut Dispatcher, regs: &[u32; 8], suspended: u32) -> Result<u32, Erro> {
    chamar(
        bus,
        d,
        regs,
        CREATE,
        &[
            SAIDA,
            0,
            65536,
            20,
            SAIDA + 4,
            0x11000,
            0x1234,
            suspended,
            0,
            0x11000,
        ],
        true,
    )
}

fn chamar(
    bus: &mut Bus,
    d: &mut Dispatcher,
    regs: &[u32; 8],
    ordinal: u32,
    args: &[u32],
    scheduled: bool,
) -> Result<u32, Erro> {
    bus.escrever_u32(GuestVirt(regs[4]), 0x11000).unwrap();
    for (i, arg) in args.iter().enumerate() {
        bus.escrever_u32(GuestVirt(regs[4] + 4 + i as u32 * 4), *arg)
            .unwrap();
    }
    let pc = imports::trampoline(ordinal);
    if scheduled {
        d.atender_escalonado(bus, pc, regs, &Clock::new())
    } else {
        d.atender(bus, pc, regs)
    }
    .map(|r| r.eax)
}

fn handle(bus: &Bus) -> u32 {
    bus.ler_u32(GuestVirt(SAIDA)).unwrap()
}

fn anterior(bus: &Bus) -> u32 {
    bus.ler_u32(GuestVirt(ANTERIOR)).unwrap()
}

fn salvar(d: &Dispatcher) -> Vec<u8> {
    let mut out = SnapshotWriter::new();
    d.save(&mut out);
    out.finish()
}

#[test]
fn sem_scheduler_os_tres_servicos_e_a_criacao_suspensa_sao_recusados() {
    let (mut bus, mut d, regs) = setup();
    assert_eq!(criar(&mut bus, &mut d, &regs, 0).unwrap(), STATUS_SUCCESS);
    let h = handle(&bus);
    for (ordinal, args) in [
        (ORDINAL_NT_SUSPEND_THREAD, vec![h, 0]),
        (ORDINAL_NT_RESUME_THREAD, vec![h, 0]),
        (ORDINAL_NT_YIELD_EXECUTION, vec![]),
    ] {
        assert!(
            matches!(
                chamar(&mut bus, &mut d, &regs, ordinal, &args, false),
                Err(Erro::VarianteNaoImplementada { .. })
            ),
            "ordinal {ordinal}"
        );
    }
    assert_eq!(d.child_suspend_count(), 0);
    assert!(!d.take_reschedule_request());
    assert!(!d.take_yield_request());

    // Criar suspensa também exige o scheduler.
    let (mut bus, mut d, regs) = setup();
    let args = [
        SAIDA,
        0,
        65536,
        20,
        SAIDA + 4,
        0x11000,
        0x1234,
        1,
        0,
        0x11000,
    ];
    assert!(matches!(
        chamar(&mut bus, &mut d, &regs, CREATE, &args, false),
        Err(Erro::VarianteNaoImplementada { .. })
    ));
    assert!(d.child_thread().is_none(), "nada foi criado");
}

#[test]
fn suspender_e_retomar_contam_e_devolvem_a_contagem_anterior() {
    let (mut bus, mut d, regs) = setup();
    criar(&mut bus, &mut d, &regs, 0).unwrap();
    let h = handle(&bus);
    for esperado in 0..3 {
        bus.escrever_u32(GuestVirt(ANTERIOR), 0xAAAA_AAAA).unwrap();
        assert_eq!(
            chamar(
                &mut bus,
                &mut d,
                &regs,
                ORDINAL_NT_SUSPEND_THREAD,
                &[h, ANTERIOR],
                true
            )
            .unwrap(),
            STATUS_SUCCESS
        );
        assert_eq!(anterior(&bus), esperado);
        assert_eq!(d.child_suspend_count(), esperado + 1);
        assert!(d.take_reschedule_request(), "suspender pede reavaliação");
    }
    for esperado in [3, 2, 1] {
        assert_eq!(
            chamar(
                &mut bus,
                &mut d,
                &regs,
                ORDINAL_NT_RESUME_THREAD,
                &[h, ANTERIOR],
                true
            )
            .unwrap(),
            STATUS_SUCCESS
        );
        assert_eq!(anterior(&bus), esperado);
        assert_eq!(d.child_suspend_count(), esperado - 1);
        assert!(d.take_reschedule_request());
    }
    // Retomar uma thread não suspensa: sucesso, anterior 0, nada a reavaliar.
    assert_eq!(
        chamar(
            &mut bus,
            &mut d,
            &regs,
            ORDINAL_NT_RESUME_THREAD,
            &[h, ANTERIOR],
            true
        )
        .unwrap(),
        STATUS_SUCCESS
    );
    assert_eq!(anterior(&bus), 0);
    assert_eq!(d.child_suspend_count(), 0);
    assert!(!d.take_reschedule_request());
    // A saída é opcional.
    assert_eq!(
        chamar(
            &mut bus,
            &mut d,
            &regs,
            ORDINAL_NT_SUSPEND_THREAD,
            &[h, 0],
            true
        )
        .unwrap(),
        STATUS_SUCCESS
    );
    assert_eq!(d.child_suspend_count(), 1);
}

#[test]
fn limite_de_suspensao_recusa_sem_alterar_a_contagem() {
    let (mut bus, mut d, regs) = setup();
    criar(&mut bus, &mut d, &regs, 0).unwrap();
    let h = handle(&bus);
    for _ in 0..MAX_SUSPEND_COUNT {
        chamar(
            &mut bus,
            &mut d,
            &regs,
            ORDINAL_NT_SUSPEND_THREAD,
            &[h, 0],
            true,
        )
        .unwrap();
    }
    assert_eq!(d.child_suspend_count(), u32::from(MAX_SUSPEND_COUNT));
    d.take_reschedule_request();
    bus.escrever_u32(GuestVirt(ANTERIOR), 0xAAAA_AAAA).unwrap();
    assert_eq!(
        chamar(
            &mut bus,
            &mut d,
            &regs,
            ORDINAL_NT_SUSPEND_THREAD,
            &[h, ANTERIOR],
            true
        )
        .unwrap(),
        STATUS_SUSPEND_COUNT_EXCEEDED
    );
    assert_eq!(d.child_suspend_count(), u32::from(MAX_SUSPEND_COUNT));
    assert_eq!(anterior(&bus), 0xAAAA_AAAA, "a saída não foi escrita");
    assert!(!d.take_reschedule_request());
}

#[test]
fn handle_alheio_fechado_ou_saida_invalida_nao_alteram_nada() {
    let (mut bus, mut d, regs) = setup();
    criar(&mut bus, &mut d, &regs, 0).unwrap();
    let h = handle(&bus);
    for ordinal in [ORDINAL_NT_SUSPEND_THREAD, ORDINAL_NT_RESUME_THREAD] {
        assert_eq!(
            chamar(&mut bus, &mut d, &regs, ordinal, &[0x1234, 0], true).unwrap(),
            STATUS_INVALID_HANDLE
        );
        // Saída fora da RAM: falha antes de qualquer efeito.
        assert_eq!(
            chamar(&mut bus, &mut d, &regs, ordinal, &[h, 0x10], true).unwrap(),
            STATUS_ACCESS_VIOLATION
        );
    }
    assert_eq!(d.child_suspend_count(), 0, "nenhuma suspensão vazou");
    assert!(!d.take_reschedule_request());

    // Handle fechado deixa de resolver a thread.
    chamar(&mut bus, &mut d, &regs, CLOSE, &[h], true).unwrap();
    for ordinal in [ORDINAL_NT_SUSPEND_THREAD, ORDINAL_NT_RESUME_THREAD] {
        assert_eq!(
            chamar(&mut bus, &mut d, &regs, ordinal, &[h, 0], true).unwrap(),
            STATUS_INVALID_HANDLE
        );
    }
}

#[test]
fn suspender_thread_terminada_devolve_status_proprio() {
    let (mut bus, mut d, regs) = setup();
    criar(&mut bus, &mut d, &regs, 0).unwrap();
    let h = handle(&bus);
    d.start_ready_thread().unwrap();
    d.select_thread(2).unwrap();
    d.terminate_running_thread(7).unwrap();
    d.select_thread(1).unwrap();
    assert_eq!(
        chamar(
            &mut bus,
            &mut d,
            &regs,
            ORDINAL_NT_SUSPEND_THREAD,
            &[h, ANTERIOR],
            true
        )
        .unwrap(),
        STATUS_THREAD_IS_TERMINATING
    );
    assert_eq!(d.child_suspend_count(), 0);
}

#[test]
fn criada_suspensa_so_inicia_depois_de_zerar_a_contagem() {
    let (mut bus, mut d, regs) = setup();
    assert_eq!(criar(&mut bus, &mut d, &regs, 1).unwrap(), STATUS_SUCCESS);
    let h = handle(&bus);
    assert_eq!(d.child_suspend_count(), 1);
    assert!(d.start_ready_thread().is_none(), "suspensa não inicia");
    assert_eq!(
        chamar(
            &mut bus,
            &mut d,
            &regs,
            ORDINAL_NT_RESUME_THREAD,
            &[h, ANTERIOR],
            true
        )
        .unwrap(),
        STATUS_SUCCESS
    );
    assert_eq!(anterior(&bus), 1);
    assert!(d.start_ready_thread().is_some(), "retomada inicia");
}

#[test]
fn yield_pede_cessao_e_o_status_final_e_da_sessao() {
    let (mut bus, mut d, regs) = setup();
    assert_eq!(
        chamar(
            &mut bus,
            &mut d,
            &regs,
            ORDINAL_NT_YIELD_EXECUTION,
            &[],
            true
        )
        .unwrap(),
        STATUS_SUCCESS,
        "EAX provisório; a sessão o troca por STATUS_NO_YIELD_PERFORMED se não houver ninguém"
    );
    assert!(d.take_yield_request());
    assert!(!d.take_yield_request(), "o pedido é consumido");
    assert_eq!(STATUS_NO_YIELD_PERFORMED, 0x4000_0024);
}

#[test]
fn snapshot_preserva_a_contagem_e_recusa_estados_invalidos() {
    let (mut bus, mut d, regs) = setup();
    criar(&mut bus, &mut d, &regs, 1).unwrap();
    let h = handle(&bus);
    chamar(
        &mut bus,
        &mut d,
        &regs,
        ORDINAL_NT_SUSPEND_THREAD,
        &[h, 0],
        true,
    )
    .unwrap();
    assert_eq!(d.child_suspend_count(), 2);
    let bytes = salvar(&d);
    let mut r = Dispatcher::new();
    Snapshot::load(&mut r, &mut SnapshotReader::new(&bytes).unwrap()).unwrap();
    assert_eq!(r.child_suspend_count(), 2);
    r.restore_memory_context(None, &bus).unwrap();
    assert_eq!(salvar(&r), bytes, "ida e volta idêntica");
}
