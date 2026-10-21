use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_formats::fixtures::xbe_com_imports;
use xbox_kernel::dispatch::{Dispatcher, EepromState, Erro, Irql, PciConfig, SystemTimeReference};
use xbox_kernel::imports::{self, Abi, Kind};
use xbox_kernel::loader::{Image, STACK_BASE, STACK_SIZE};
use xbox_mem::{Bus, GuestVirt};

fn setup() -> (Bus, [u32; 8]) {
    let bytes = xbe_com_imports(&[0xC3], &[103, 160, 161]).unwrap();
    let mut bus = Image::prepare_aot(&bytes).unwrap().image.bus;
    let mut regs = [0xDEADBEEF; 8];
    // Só a palavra de retorno está mapeada; stdcall/0 não lê argumentos.
    regs[4] = STACK_BASE + STACK_SIZE - 4;
    bus.escrever_u32(GuestVirt(regs[4]), 0x11000).unwrap();
    bus.escrever_u32(GuestVirt(0x12100), 0xCAFEBABE).unwrap();
    (bus, regs)
}

fn saved(value: &impl Snapshot) -> Vec<u8> {
    let mut out = SnapshotWriter::new();
    value.save(&mut out);
    out.finish()
}

#[test]
fn consulta_kirql_usa_contexto_configurado_sem_argumentos_ou_efeitos_na_ram() {
    let (mut bus, regs) = setup();
    assert_eq!(
        imports::lookup(103).unwrap().kind,
        Kind::Function(Abi::Stdcall(0))
    );
    assert_eq!(Dispatcher::new().irql(), Irql::Passive);
    for (irql, expected) in [(Irql::Passive, 0), (Irql::Apc, 1), (Irql::Dispatch, 2)] {
        let mut d = Dispatcher::with_irql(irql);
        assert_eq!(
            d.atender(&mut bus, imports::trampoline(103), &regs)
                .unwrap()
                .eax,
            expected
        );
        assert_eq!(d.irql(), irql);
        assert_eq!((d.atendidas(), d.recusadas()), (1, 0));
        assert_eq!(bus.ler_u32(GuestVirt(0x12100)).unwrap(), 0xCAFEBABE);
        assert_eq!(bus.ler_u32(GuestVirt(regs[4])).unwrap(), 0x11000);
    }
}

#[test]
fn raise_e_lower_continuam_recusados_sem_normalizar_nivel() {
    let (mut bus, regs) = setup();
    let mut d = Dispatcher::with_irql(Irql::Dispatch);
    for ordinal in [160, 161] {
        assert!(!imports::servico_implementado(ordinal));
        assert!(matches!(
            d.atender(&mut bus, imports::trampoline(ordinal), &regs),
            Err(Erro::NaoImplementado { .. })
        ));
        assert_eq!(d.irql(), Irql::Dispatch);
    }
    for level in 3..=255 {
        assert!(Irql::from_level(level).is_none());
    }
    assert_eq!((d.atendidas(), d.recusadas()), (0, 2));
}

#[test]
fn snapshot_preserva_niveis_e_carga_de_kernel_recusa_corrupcao_sem_commit_parcial() {
    let (mut bus, regs) = setup();
    for irql in [Irql::Passive, Irql::Apc, Irql::Dispatch] {
        let mut d = Dispatcher::with_irql(irql);
        d.atender(&mut bus, imports::trampoline(103), &regs)
            .unwrap();
        let bytes = saved(&d);
        let mut restored = Dispatcher::new();
        let mut inp = SnapshotReader::new(&bytes).unwrap();
        restored.load(&mut inp).unwrap();
        assert_eq!(inp.remaining(), 0);
        assert_eq!(saved(&restored), bytes);
        assert_eq!(restored.irql(), irql);
        assert_eq!(
            restored
                .atender(&mut bus, imports::trampoline(103), &regs)
                .unwrap()
                .eax,
            u32::from(irql.level())
        );
        let original = saved(&restored);
        for (version, level) in [(0u32, 0u8), (2, 0), (1, 3), (1, 31), (1, 255)] {
            let mut bad = bytes.clone();
            let at = bad.len() - 9;
            bad[at + 4..at + 8].copy_from_slice(&version.to_le_bytes());
            bad[at + 8] = level;
            assert!(
                restored
                    .load(&mut SnapshotReader::new(&bad).unwrap())
                    .is_err()
            );
            assert_eq!(saved(&restored), original);
        }
        for end in 12..bytes.len() {
            assert!(
                restored
                    .load(&mut SnapshotReader::new(&bytes[..end]).unwrap())
                    .is_err()
            );
            assert_eq!(saved(&restored), original);
        }
    }
}

#[test]
fn krnl_v1_a_v4_migram_para_passive_sem_herdar_contexto_destino() {
    for version in 1..=4 {
        let mut out = SnapshotWriter::new();
        out.section("KRNL", version);
        out.u64(7);
        out.u64(3);
        out.section("KMUT", 2);
        out.u32(4);
        out.u32(0);
        let eeprom = EepromState::supplied([0xAA; 256]);
        let time = SystemTimeReference::from_boot_ticks(116_444_736_000_000_000).unwrap();
        if version >= 2 {
            eeprom.save(&mut out);
        }
        if version >= 3 {
            PciConfig::default().save(&mut out);
        }
        if version >= 4 {
            time.save(&mut out);
        }
        let bytes = out.finish();
        let mut d = Dispatcher::with_irql(Irql::Dispatch);
        let mut inp = SnapshotReader::new(&bytes).unwrap();
        d.load(&mut inp).unwrap();
        assert_eq!(inp.remaining(), 0);
        assert_eq!(d.irql(), Irql::Passive);
        assert_eq!((d.atendidas(), d.recusadas()), (7, 3));
        assert_eq!(
            *d.eeprom(),
            if version >= 2 {
                eeprom
            } else {
                EepromState::default()
            }
        );
        assert_eq!(
            *d.system_time(),
            if version >= 4 {
                time
            } else {
                SystemTimeReference::default()
            }
        );
    }
}
