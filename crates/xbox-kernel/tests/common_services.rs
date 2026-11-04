//! `IoDeleteSymbolicLink`, `AvSetSavedDataAddress`, `DbgPrint` e `HalReturnToFirmware`
//! (ADR-0100).
use xbox_core::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_formats::fixtures::xbe_com_imports;
use xbox_kernel::dispatch::{
    Dispatcher, Erro, MAX_DEBUG_OUTPUT, ORDINAL_AV_GET_SAVED_DATA_ADDRESS,
    ORDINAL_AV_SET_SAVED_DATA_ADDRESS, ORDINAL_DBG_PRINT,
    ORDINAL_HAL_REGISTER_SHUTDOWN_NOTIFICATION, ORDINAL_HAL_RETURN_TO_FIRMWARE,
    ORDINAL_IO_CREATE_SYMBOLIC_LINK, ORDINAL_IO_DELETE_SYMBOLIC_LINK, ORDINAL_NT_CLOSE,
    ORDINAL_NT_OPEN_SYMBOLIC_LINK_OBJECT, STATUS_OBJECT_NAME_NOT_FOUND, STATUS_SUCCESS,
};
use xbox_kernel::imports;
use xbox_kernel::loader::{Image, STACK_BASE, STACK_SIZE};
use xbox_mem::{Bus, GuestVirt};

const ROTINA: u32 = 0x11000;
const TEXTO: u32 = 0x12100;
const NOME: u32 = 0x12200;
const ALVO: u32 = 0x12300;
const SAIDA: u32 = 0x12400;
const ATRIBUTOS: u32 = 0x12410;

fn setup() -> (Bus, Dispatcher, [u32; 8]) {
    let image = xbe_com_imports(
        &[0xC3],
        &[
            ORDINAL_IO_CREATE_SYMBOLIC_LINK,
            ORDINAL_IO_DELETE_SYMBOLIC_LINK,
            ORDINAL_NT_OPEN_SYMBOLIC_LINK_OBJECT,
            ORDINAL_NT_CLOSE,
            ORDINAL_AV_SET_SAVED_DATA_ADDRESS,
            ORDINAL_AV_GET_SAVED_DATA_ADDRESS,
            ORDINAL_DBG_PRINT,
            ORDINAL_HAL_REGISTER_SHUTDOWN_NOTIFICATION,
            ORDINAL_HAL_RETURN_TO_FIRMWARE,
        ],
    )
    .unwrap();
    let mut regs = [0; 8];
    regs[4] = STACK_BASE + STACK_SIZE - 0x200;
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
    args: &[u32],
) -> Result<u32, Erro> {
    bus.escrever_u32(GuestVirt(regs[4]), 0x11000).unwrap();
    for (i, arg) in args.iter().enumerate() {
        bus.escrever_u32(GuestVirt(regs[4] + 4 + i as u32 * 4), *arg)
            .unwrap();
    }
    d.atender(bus, imports::trampoline(ordinal), regs)
        .map(|r| r.eax)
}

/// Grava uma `OBJECT_STRING` (`Length`, `MaximumLength`, `Buffer`) em `at`, com o texto logo depois.
fn texto(bus: &mut Bus, at: u32, valor: &[u8]) {
    let corpo = at + 8;
    bus.escrever_bytes(GuestVirt(corpo), valor).unwrap();
    bus.escrever_u16(GuestVirt(at), valor.len() as u16).unwrap();
    bus.escrever_u16(GuestVirt(at + 2), valor.len() as u16 + 1)
        .unwrap();
    bus.escrever_u32(GuestVirt(at + 4), corpo).unwrap();
}

fn saved(d: &Dispatcher) -> Vec<u8> {
    let mut out = SnapshotWriter::new();
    d.save(&mut out);
    out.finish()
}

fn cria_d(bus: &mut Bus, d: &mut Dispatcher, regs: &[u32; 8]) -> u32 {
    texto(bus, NOME, b"\\??\\D:");
    texto(bus, ALVO, b"\\Device\\CdRom0");
    chamar(bus, d, regs, ORDINAL_IO_CREATE_SYMBOLIC_LINK, &[NOME, ALVO]).unwrap()
}

#[test]
fn apagar_o_link_libera_o_nome_e_mantem_handles_ate_o_close() {
    let (mut bus, mut d, regs) = setup();
    assert_eq!(cria_d(&mut bus, &mut d, &regs), STATUS_SUCCESS);
    // Abre um handle antes de apagar.
    bus.escrever_u32(GuestVirt(ATRIBUTOS), 0).unwrap();
    bus.escrever_u32(GuestVirt(ATRIBUTOS + 4), NOME).unwrap();
    bus.escrever_u32(GuestVirt(ATRIBUTOS + 8), 0x40).unwrap();
    assert_eq!(
        chamar(
            &mut bus,
            &mut d,
            &regs,
            ORDINAL_NT_OPEN_SYMBOLIC_LINK_OBJECT,
            &[SAIDA, ATRIBUTOS]
        )
        .unwrap(),
        STATUS_SUCCESS
    );
    let handle = bus.ler_u32(GuestVirt(SAIDA)).unwrap();

    assert_eq!(
        chamar(
            &mut bus,
            &mut d,
            &regs,
            ORDINAL_IO_DELETE_SYMBOLIC_LINK,
            &[NOME]
        )
        .unwrap(),
        STATUS_SUCCESS
    );
    assert_eq!(d.drive_d_target(), None, "o nome saiu do namespace");
    assert_eq!(
        chamar(
            &mut bus,
            &mut d,
            &regs,
            ORDINAL_NT_OPEN_SYMBOLIC_LINK_OBJECT,
            &[SAIDA, ATRIBUTOS]
        )
        .unwrap(),
        STATUS_OBJECT_NAME_NOT_FOUND
    );
    assert_eq!(
        chamar(
            &mut bus,
            &mut d,
            &regs,
            ORDINAL_IO_DELETE_SYMBOLIC_LINK,
            &[NOME]
        )
        .unwrap(),
        STATUS_OBJECT_NAME_NOT_FOUND,
        "apagar de novo"
    );
    // O handle aberto antes continua fechável, uma vez.
    assert_eq!(
        chamar(&mut bus, &mut d, &regs, ORDINAL_NT_CLOSE, &[handle]).unwrap(),
        STATUS_SUCCESS
    );
    assert_ne!(
        chamar(&mut bus, &mut d, &regs, ORDINAL_NT_CLOSE, &[handle]).unwrap(),
        STATUS_SUCCESS
    );
    // E o nome pode ser criado de novo.
    assert_eq!(cria_d(&mut bus, &mut d, &regs), STATUS_SUCCESS);
    assert_eq!(d.drive_d_target(), Some("\\Device\\CdRom0"));
}

#[test]
fn apagar_nome_fora_do_perfil_e_recusado_sem_tocar_em_d() {
    let (mut bus, mut d, regs) = setup();
    assert_eq!(cria_d(&mut bus, &mut d, &regs), STATUS_SUCCESS);
    texto(&mut bus, NOME, b"\\??\\E:");
    assert!(matches!(
        chamar(
            &mut bus,
            &mut d,
            &regs,
            ORDINAL_IO_DELETE_SYMBOLIC_LINK,
            &[NOME]
        ),
        Err(Erro::VarianteNaoImplementada { .. })
    ));
    assert_eq!(d.drive_d_target(), Some("\\Device\\CdRom0"));
}

#[test]
fn endereco_de_dados_salvos_comeca_em_zero_e_o_set_o_devolve() {
    let (mut bus, mut d, regs) = setup();
    assert_eq!(
        chamar(
            &mut bus,
            &mut d,
            &regs,
            ORDINAL_AV_GET_SAVED_DATA_ADDRESS,
            &[]
        )
        .unwrap(),
        0
    );
    for endereco in [0x0123_4567, 0, 0xFFFF_FFFF] {
        chamar(
            &mut bus,
            &mut d,
            &regs,
            ORDINAL_AV_SET_SAVED_DATA_ADDRESS,
            &[endereco],
        )
        .unwrap();
        assert_eq!(
            chamar(
                &mut bus,
                &mut d,
                &regs,
                ORDINAL_AV_GET_SAVED_DATA_ADDRESS,
                &[]
            )
            .unwrap(),
            endereco
        );
    }
    chamar(
        &mut bus,
        &mut d,
        &regs,
        ORDINAL_AV_SET_SAVED_DATA_ADDRESS,
        &[0xCAFE_0000],
    )
    .unwrap();
    let bytes = saved(&d);
    let mut restaurado = Dispatcher::new();
    Snapshot::load(&mut restaurado, &mut SnapshotReader::new(&bytes).unwrap()).unwrap();
    assert_eq!(restaurado.video().saved_data_address(), 0xCAFE_0000);
    assert_eq!(saved(&restaurado), bytes, "ida e volta idêntica");
}

fn imprime(
    bus: &mut Bus,
    d: &mut Dispatcher,
    regs: &[u32; 8],
    formato: &[u8],
    args: &[u32],
) -> Result<u32, Erro> {
    let mut bytes = formato.to_vec();
    bytes.push(0);
    bus.escrever_bytes(GuestVirt(TEXTO), &bytes).unwrap();
    let mut todos = vec![TEXTO];
    todos.extend_from_slice(args);
    chamar(bus, d, regs, ORDINAL_DBG_PRINT, &todos)
}

#[test]
fn dbgprint_formata_as_conversoes_de_32_bits() {
    let (mut bus, mut d, regs) = setup();
    bus.escrever_bytes(GuestVirt(NOME), b"xbox\0").unwrap();
    let status = imprime(
        &mut bus,
        &mut d,
        &regs,
        b"[%s] %d %i %u %x %X %c %p %5d|%-5d|%05d|%05d|%%\n",
        &[
            NOME,
            (-7i32).cast_unsigned(),
            12,
            4_000_000_000,
            0xBEEF,
            0xBEEF,
            u32::from(b'Z'),
            0x00DE_ADB0,
            42,
            42,
            42,
            (-42i32).cast_unsigned(),
        ],
    )
    .unwrap();
    assert_eq!(status, STATUS_SUCCESS);
    assert_eq!(
        std::str::from_utf8(d.debug_output()).unwrap(),
        "[xbox] -7 12 4000000000 beef BEEF Z 00DEADB0    42|42   |00042|-0042|%\n"
    );
}

#[test]
fn dbgprint_acumula_e_recusa_o_que_nao_sabe_imprimir_sem_escrever_nada() {
    let (mut bus, mut d, regs) = setup();
    imprime(&mut bus, &mut d, &regs, b"a=%d;", &[1]).unwrap();
    imprime(&mut bus, &mut d, &regs, b"b=%d;", &[2]).unwrap();
    assert_eq!(d.debug_output(), b"a=1;b=2;");
    for formato in [&b"%f"[..], b"%lld", b"%n", b"%S", b"%I64d", b"abc %", b"%2"] {
        assert!(
            matches!(
                imprime(&mut bus, &mut d, &regs, formato, &[0, 0]),
                Err(Erro::VarianteNaoImplementada { .. })
            ),
            "{}",
            String::from_utf8_lossy(formato)
        );
    }
    // %s com ponteiro nulo ou fora da RAM.
    for ponteiro in [0, 0x10] {
        assert!(imprime(&mut bus, &mut d, &regs, b"x%s", &[ponteiro]).is_err());
    }
    assert_eq!(
        d.debug_output(),
        b"a=1;b=2;",
        "recusa não imprime meia linha"
    );
}

#[test]
fn dbgprint_descarta_e_conta_o_que_passa_do_limite() {
    let (mut bus, mut d, regs) = setup();
    let linha = [b'x'; 400];
    let mut formato = linha.to_vec();
    formato.extend_from_slice(b"%d");
    let mut impressos = 0usize;
    while impressos < MAX_DEBUG_OUTPUT + 1000 {
        imprime(&mut bus, &mut d, &regs, &formato, &[1]).unwrap();
        impressos += 401;
    }
    assert_eq!(d.debug_output().len(), MAX_DEBUG_OUTPUT);
    assert_eq!(
        d.debug_output_dropped() as usize,
        impressos - MAX_DEBUG_OUTPUT
    );
}

#[test]
fn retorno_ao_firmware_registra_o_pedido_e_recusa_rotina_invalida() {
    let (mut bus, mut d, regs) = setup();
    assert_eq!(d.take_firmware_request(), None);
    for rotina in 0..5 {
        chamar(
            &mut bus,
            &mut d,
            &regs,
            ORDINAL_HAL_RETURN_TO_FIRMWARE,
            &[rotina],
        )
        .unwrap();
        assert_eq!(d.take_firmware_request(), Some(rotina));
        assert_eq!(d.take_firmware_request(), None, "o pedido é consumido");
    }
    for rotina in [5, 6, u32::MAX] {
        assert!(matches!(
            chamar(
                &mut bus,
                &mut d,
                &regs,
                ORDINAL_HAL_RETURN_TO_FIRMWARE,
                &[rotina]
            ),
            Err(Erro::ArgumentoInvalido { .. })
        ));
        assert_eq!(d.take_firmware_request(), None);
    }
}

#[test]
fn notificacoes_de_desligamento_saem_da_maior_prioridade_para_a_menor() {
    let (mut bus, mut d, regs) = setup();
    let registro = |n: u32| 0x12500 + n * 0x20;
    for (n, prioridade) in [(0u32, 1i32), (1, 5), (2, 5), (3, -2)] {
        bus.escrever_u32(GuestVirt(registro(n)), ROTINA).unwrap();
        bus.escrever_u32(GuestVirt(registro(n) + 4), prioridade.cast_unsigned())
            .unwrap();
        chamar(
            &mut bus,
            &mut d,
            &regs,
            ORDINAL_HAL_REGISTER_SHUTDOWN_NOTIFICATION,
            &[registro(n), 1],
        )
        .unwrap();
    }
    let chamadas = d.shutdown_notifications(&bus).unwrap();
    let ordem: Vec<u32> = chamadas.iter().map(|c| c.arguments[0]).collect();
    assert_eq!(
        ordem,
        [registro(1), registro(2), registro(0), registro(3)],
        "5, 5 (ordem de registro), 1, -2"
    );
    assert!(chamadas.iter().all(|c| c.entry == ROTINA && c.count == 1));
}
