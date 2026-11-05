//! Contador de performance e conversões de `TIME_FIELDS` (ADR-0101).
use xbox_core::time::{CPU_HZ, Clock, PERF_COUNTER_HZ};
use xbox_formats::fixtures::xbe_com_imports;
use xbox_kernel::dispatch::{
    Dispatcher, Erro, ORDINAL_KE_QUERY_INTERRUPT_TIME, ORDINAL_KE_QUERY_PERFORMANCE_COUNTER,
    ORDINAL_KE_QUERY_PERFORMANCE_FREQUENCY, ORDINAL_RTL_TIME_FIELDS_TO_TIME,
    ORDINAL_RTL_TIME_TO_TIME_FIELDS,
};
use xbox_kernel::imports;
use xbox_kernel::loader::{Image, STACK_BASE, STACK_SIZE};
use xbox_mem::{Bus, GuestVirt};

const TEMPO: u32 = 0x12100;
const CAMPOS: u32 = 0x12200;

fn setup() -> (Bus, Dispatcher, [u32; 8]) {
    let image = xbe_com_imports(
        &[0xC3],
        &[
            ORDINAL_KE_QUERY_INTERRUPT_TIME,
            ORDINAL_KE_QUERY_PERFORMANCE_COUNTER,
            ORDINAL_KE_QUERY_PERFORMANCE_FREQUENCY,
            ORDINAL_RTL_TIME_FIELDS_TO_TIME,
            ORDINAL_RTL_TIME_TO_TIME_FIELDS,
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
    args: &[u32],
    clock: Clock,
) -> Result<u32, Erro> {
    bus.escrever_u32(GuestVirt(regs[4]), 0x11000).unwrap();
    for (i, arg) in args.iter().enumerate() {
        bus.escrever_u32(GuestVirt(regs[4] + 4 + i as u32 * 4), *arg)
            .unwrap();
    }
    d.atender_com_clock(bus, imports::trampoline(ordinal), regs, &clock)
        .map(|r| r.eax)
}

fn campos(bus: &mut Bus, valores: [i16; 8]) {
    for (i, v) in valores.into_iter().enumerate() {
        bus.escrever_u16(GuestVirt(CAMPOS + i as u32 * 2), v.cast_unsigned())
            .unwrap();
    }
}

fn le_campos(bus: &Bus) -> [i16; 8] {
    std::array::from_fn(|i| {
        bus.ler_u16(GuestVirt(CAMPOS + i as u32 * 2))
            .unwrap()
            .cast_signed()
    })
}

#[test]
fn frequencia_e_o_relogio_acpi_de_3_375_mhz_em_edx_eax() {
    let (mut bus, mut d, regs) = setup();
    let clock = Clock::new();
    let baixa = chamar(
        &mut bus,
        &mut d,
        &regs,
        ORDINAL_KE_QUERY_PERFORMANCE_FREQUENCY,
        &[],
        clock,
    )
    .unwrap();
    let alta = d
        .take_return_high()
        .expect("ULONGLONG devolve a palavra alta");
    assert_eq!((u64::from(alta) << 32) | u64::from(baixa), PERF_COUNTER_HZ);
    assert_eq!(PERF_COUNTER_HZ, 3_375_000);
    assert_eq!(d.take_return_high(), None, "a palavra alta é consumida");
}

#[test]
fn contador_acompanha_o_clock_da_sessao() {
    let (mut bus, mut d, regs) = setup();
    let mut clock = Clock::new();
    let mut leituras = Vec::new();
    for ciclos in [0, CPU_HZ / 2, CPU_HZ, 10 * CPU_HZ + 12345] {
        clock.set(ciclos);
        let baixa = chamar(
            &mut bus,
            &mut d,
            &regs,
            ORDINAL_KE_QUERY_PERFORMANCE_COUNTER,
            &[],
            clock,
        )
        .unwrap();
        let valor = (u64::from(d.take_return_high().unwrap()) << 32) | u64::from(baixa);
        assert_eq!(valor, clock.perf_counter());
        leituras.push(valor);
    }
    assert_eq!(leituras[0], 0);
    assert_eq!(
        leituras[2], PERF_COUNTER_HZ,
        "um segundo guest = a frequência"
    );
    assert!(leituras.windows(2).all(|w| w[0] < w[1]), "monótono");
}

#[test]
fn contador_sem_o_clock_da_sessao_e_recusado() {
    let (mut bus, mut d, regs) = setup();
    bus.escrever_u32(GuestVirt(regs[4]), 0x11000).unwrap();
    let erro = d
        .atender(
            &mut bus,
            imports::trampoline(ORDINAL_KE_QUERY_PERFORMANCE_COUNTER),
            &regs,
        )
        .unwrap_err();
    assert!(matches!(erro, Erro::ContextoTemporal { .. }), "{erro}");
    assert_eq!(d.take_return_high(), None);
}

#[test]
fn campos_para_tempo_e_de_volta_pela_abi() {
    let (mut bus, mut d, regs) = setup();
    let clock = Clock::new();
    campos(&mut bus, [2000, 3, 1, 12, 34, 56, 789, 0]);
    assert_eq!(
        chamar(
            &mut bus,
            &mut d,
            &regs,
            ORDINAL_RTL_TIME_FIELDS_TO_TIME,
            &[CAMPOS, TEMPO],
            clock
        )
        .unwrap(),
        1,
        "BOOLEAN TRUE"
    );
    let ticks = bus.ler_u64(GuestVirt(TEMPO)).unwrap().cast_signed();
    assert_eq!(
        ticks,
        xbox_kernel::dispatch::fields_to_ticks([2000, 3, 1, 12, 34, 56, 789, 0]).unwrap()
    );
    // E de volta: o dia da semana (quarta-feira) é preenchido.
    campos(&mut bus, [0; 8]);
    chamar(
        &mut bus,
        &mut d,
        &regs,
        ORDINAL_RTL_TIME_TO_TIME_FIELDS,
        &[TEMPO, CAMPOS],
        clock,
    )
    .unwrap();
    assert_eq!(le_campos(&bus), [2000, 3, 1, 12, 34, 56, 789, 3]);
}

#[test]
fn campos_invalidos_devolvem_false_sem_escrever() {
    let (mut bus, mut d, regs) = setup();
    let clock = Clock::new();
    bus.escrever_u64(GuestVirt(TEMPO), 0xAAAA_AAAA_AAAA_AAAA)
        .unwrap();
    for ruim in [
        [1900, 2, 29, 0, 0, 0, 0, 0],
        [2000, 13, 1, 0, 0, 0, 0, 0],
        [1600, 1, 1, 0, 0, 0, 0, 0],
    ] {
        campos(&mut bus, ruim);
        assert_eq!(
            chamar(
                &mut bus,
                &mut d,
                &regs,
                ORDINAL_RTL_TIME_FIELDS_TO_TIME,
                &[CAMPOS, TEMPO],
                clock
            )
            .unwrap(),
            0,
            "{ruim:?}"
        );
    }
    assert_eq!(
        bus.ler_u64(GuestVirt(TEMPO)).unwrap(),
        0xAAAA_AAAA_AAAA_AAAA
    );
}

#[test]
fn ponteiros_fora_da_ram_e_tempo_negativo_sao_recusados_sem_escrever() {
    let (mut bus, mut d, regs) = setup();
    let clock = Clock::new();
    for args in [[CAMPOS, 0x10], [0x10, TEMPO]] {
        assert!(matches!(
            chamar(
                &mut bus,
                &mut d,
                &regs,
                ORDINAL_RTL_TIME_FIELDS_TO_TIME,
                &args,
                clock
            ),
            Err(Erro::ArgumentoInvalido { .. })
        ));
        assert!(matches!(
            chamar(
                &mut bus,
                &mut d,
                &regs,
                ORDINAL_RTL_TIME_TO_TIME_FIELDS,
                &args,
                clock
            ),
            Err(Erro::ArgumentoInvalido { .. })
        ));
    }
    campos(&mut bus, [7; 8]);
    bus.escrever_u64(GuestVirt(TEMPO), (-5i64).cast_unsigned())
        .unwrap();
    assert!(
        chamar(
            &mut bus,
            &mut d,
            &regs,
            ORDINAL_RTL_TIME_TO_TIME_FIELDS,
            &[TEMPO, CAMPOS],
            clock
        )
        .is_err()
    );
    assert_eq!(le_campos(&bus), [7; 8], "a saída não foi tocada");
}

#[test]
fn tempo_de_interrupcao_e_o_tempo_desde_o_boot_em_100_ns() {
    let (mut bus, mut d, regs) = setup();
    let mut clock = Clock::new();
    for (ciclos, esperado) in [(0, 0), (CPU_HZ, 10_000_000), (3 * CPU_HZ, 30_000_000)] {
        clock.set(ciclos);
        let baixa = chamar(
            &mut bus,
            &mut d,
            &regs,
            ORDINAL_KE_QUERY_INTERRUPT_TIME,
            &[],
            clock,
        )
        .unwrap();
        let valor = (u64::from(d.take_return_high().unwrap()) << 32) | u64::from(baixa);
        assert_eq!(valor, esperado);
    }
}
