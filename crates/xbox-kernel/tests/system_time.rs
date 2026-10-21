use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_core::time::{CPU_HZ, Clock};
use xbox_formats::fixtures::xbe_com_imports;
use xbox_kernel::dispatch::{Dispatcher, EepromState, Erro, PciConfig, SystemTimeReference};
use xbox_kernel::imports;
use xbox_kernel::loader::{Image, STACK_BASE, STACK_SIZE};
use xbox_mem::{Bus, GuestPhys, GuestVirt, Perm};

const OUTPUT: u32 = 0x12101;
const UNIX_EPOCH: i64 = 116_444_736_000_000_000;

fn setup() -> (Bus, Dispatcher, [u32; 8]) {
    let image = xbe_com_imports(&[0xC3], &[128]).unwrap();
    let mut bus = Image::prepare_aot(&image).unwrap().image.bus;
    let mut regs = [0; 8];
    regs[4] = STACK_BASE + STACK_SIZE - 0x100;
    bus.escrever_u32(GuestVirt(regs[4]), 0x11000).unwrap();
    bus.escrever_u32(GuestVirt(regs[4] + 4), OUTPUT).unwrap();
    (bus, Dispatcher::new(), regs)
}

fn call(bus: &mut Bus, d: &mut Dispatcher, regs: &[u32; 8], clock: Clock) -> Result<u32, Erro> {
    d.atender_com_clock(bus, imports::trampoline(128), regs, &clock)
        .map(|r| r.eax)
}

fn read(bus: &Bus, at: u32) -> u64 {
    let mut bytes = [0; 8];
    bus.ler_bytes(GuestVirt(at), &mut bytes).unwrap();
    u64::from_le_bytes(bytes)
}

fn saved(value: &impl Snapshot) -> Vec<u8> {
    let mut out = SnapshotWriter::new();
    value.save(&mut out);
    out.finish()
}

#[test]
fn epoch_2000_e_segundo_decorrido_escrevem_large_integer_com_guardas() {
    let (mut bus, mut d, regs) = setup();
    bus.escrever_bytes(GuestVirt(OUTPUT - 1), &[0xAA; 10])
        .unwrap();
    let mut clock = Clock::new();
    assert_eq!(call(&mut bus, &mut d, &regs, clock).unwrap(), 0);
    // 1601..2000: 399 anos, 96 dias bissextos (1700, 1800, 1900 excluídos).
    let epoch = (399u64 * 365 + 96) * 86400 * 10_000_000;
    assert_eq!(read(&bus, OUTPUT), epoch);
    clock.advance(CPU_HZ);
    call(&mut bus, &mut d, &regs, clock).unwrap();
    assert_eq!(read(&bus, OUTPUT), epoch + 10_000_000);
    assert_eq!(bus.ler_u8(GuestVirt(OUTPUT - 1)).unwrap(), 0xAA);
    assert_eq!(bus.ler_u8(GuestVirt(OUTPUT + 8)).unwrap(), 0xAA);
    assert_eq!(clock.cycles(), CPU_HZ);
    assert_eq!(d.atendidas(), 2);
}

#[test]
fn fracao_100ns_e_clock_maximo_sem_overflow_intermediario() {
    let reference = SystemTimeReference::from_boot_ticks(0).unwrap();
    let mut clock = Clock::new();
    clock.set(73);
    assert_eq!(reference.ticks_at(&clock), Some(0));
    clock.set(74);
    assert_eq!(reference.ticks_at(&clock), Some(1));
    clock.set(CPU_HZ - 1);
    assert_eq!(reference.ticks_at(&clock), Some(9_999_999));
    clock.set(CPU_HZ);
    assert_eq!(reference.ticks_at(&clock), Some(10_000_000));
    clock.set(u64::MAX);
    // Referência por quociente/resto, sem multiplicar o total de ciclos.
    let expected = (u64::MAX / CPU_HZ) * 10_000_000 + (u64::MAX % CPU_HZ) * 10_000_000 / CPU_HZ;
    assert_eq!(reference.ticks_at(&clock), Some(expected));
    assert!(SystemTimeReference::from_boot_ticks(-1).is_none());
}

#[test]
fn contexto_ausente_e_soma_fora_do_large_integer_recusam_sem_saida() {
    let (mut bus, mut d, regs) = setup();
    bus.escrever_bytes(GuestVirt(OUTPUT), &[0xAA; 8]).unwrap();
    assert!(matches!(
        d.atender(&mut bus, imports::trampoline(128), &regs),
        Err(Erro::ContextoTemporal { .. })
    ));
    d = Dispatcher::with_system_time(SystemTimeReference::from_boot_ticks(i64::MAX).unwrap());
    let mut clock = Clock::new();
    clock.advance(74);
    assert!(matches!(
        call(&mut bus, &mut d, &regs, clock),
        Err(Erro::ContextoTemporal { .. })
    ));
    assert_eq!(read(&bus, OUTPUT), 0xAAAA_AAAA_AAAA_AAAA);
    assert_eq!(d.atendidas(), 0);
    call(&mut bus, &mut d, &regs, Clock::new()).unwrap();
    assert_eq!(read(&bus, OUTPUT), i64::MAX as u64);
}

#[test]
fn saida_inteira_validada_antes_de_escrever_ram_ou_mmio() {
    let (mut bus, mut d, regs) = setup();
    bus.escrever_u32(GuestVirt(0x12FFC), 0xDEADBEEF).unwrap();
    bus.mapear(GuestVirt(0x5000_0000), GuestPhys(0xFD00_0000), 1, Perm::RW)
        .unwrap();
    for pointer in [0, u32::MAX - 3, 0x6000_0000, 0x11000, 0x12FFC, 0x5000_0000] {
        bus.escrever_u32(GuestVirt(regs[4] + 4), pointer).unwrap();
        assert!(matches!(
            call(&mut bus, &mut d, &regs, Clock::new()),
            Err(Erro::ArgumentoInvalido { .. })
        ));
        assert_eq!(bus.ler_u32(GuestVirt(0x12FFC)).unwrap(), 0xDEADBEEF);
    }
    // Um LARGE_INTEGER pode atravessar páginas válidas; uma segunda página RO recusa tudo.
    bus.mapear(GuestVirt(0x13000), GuestPhys(0x13000), 1, Perm::RO)
        .unwrap();
    bus.escrever_u32(GuestVirt(regs[4] + 4), 0x12FFC).unwrap();
    assert!(call(&mut bus, &mut d, &regs, Clock::new()).is_err());
    assert_eq!(bus.ler_u32(GuestVirt(0x12FFC)).unwrap(), 0xDEADBEEF);
    bus.mapear(GuestVirt(0x13000), GuestPhys(0x13000), 1, Perm::RW)
        .unwrap();
    call(&mut bus, &mut d, &regs, Clock::new()).unwrap();
    assert_eq!(read(&bus, 0x12FFC), d.system_time().boot_ticks());
}

#[test]
fn snapshot_preserva_referencia_configurada_e_rejeita_corrupcao_transacionalmente() {
    let d = Dispatcher::with_system_time(SystemTimeReference::from_boot_ticks(UNIX_EPOCH).unwrap());
    let snapshot = saved(&d);
    let mut restored = Dispatcher::new();
    let mut inp = SnapshotReader::new(&snapshot).unwrap();
    restored.load(&mut inp).unwrap();
    assert_eq!(inp.remaining(), 0);
    assert_eq!(saved(&restored), snapshot);
    assert_eq!(restored.system_time().boot_ticks(), UNIX_EPOCH as u64);
    for end in 12..snapshot.len() {
        assert!(
            restored
                .load(&mut SnapshotReader::new(&snapshot[..end]).unwrap())
                .is_err()
        );
        assert_eq!(saved(&restored), snapshot);
    }
    for (version, ticks) in [(0u32, 0u64), (2, 0), (1, u64::MAX)] {
        let mut bad = snapshot.clone();
        let at = bad.windows(4).position(|tag| tag == b"STIM").unwrap();
        bad[at + 4..at + 8].copy_from_slice(&version.to_le_bytes());
        bad[at + 8..at + 16].copy_from_slice(&ticks.to_le_bytes());
        assert!(
            restored
                .load(&mut SnapshotReader::new(&bad).unwrap())
                .is_err()
        );
        assert_eq!(saved(&restored), snapshot);
    }
}

#[test]
fn krnl_v1_a_v3_migram_referencia_sem_herdar_configuracao_destino() {
    for version in 1..=3 {
        let mut out = SnapshotWriter::new();
        out.section("KRNL", version);
        out.u64(23);
        out.u64(4);
        out.section("KMUT", 2);
        out.u32(4);
        out.u32(0);
        if version >= 2 {
            EepromState::default().save(&mut out);
        }
        if version >= 3 {
            PciConfig::default().save(&mut out);
        }
        let bytes = out.finish();
        let mut d =
            Dispatcher::with_system_time(SystemTimeReference::from_boot_ticks(UNIX_EPOCH).unwrap());
        let mut inp = SnapshotReader::new(&bytes).unwrap();
        d.load(&mut inp).unwrap();
        assert_eq!(inp.remaining(), 0);
        assert_eq!(*d.system_time(), SystemTimeReference::default());
        assert_eq!((d.atendidas(), d.recusadas()), (23, 4));
    }
}
