use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_formats::fixtures::xbe_com_imports;
use xbox_kernel::dispatch::{Dispatcher, EepromState, Erro, PciConfig, PciProfile};
use xbox_kernel::imports;
use xbox_kernel::loader::{Image, STACK_BASE, STACK_SIZE};
use xbox_mem::{Acesso, Bus, GuestPhys, GuestVirt, Perm};

const OUTPUT: u32 = 0x12100;

fn setup() -> (Bus, Dispatcher) {
    let image = xbe_com_imports(&[0xC3], &[46]).unwrap();
    (
        Image::prepare_aot(&image).unwrap().image.bus,
        Dispatcher::new(),
    )
}

fn call(bus: &mut Bus, d: &mut Dispatcher, args: [u32; 6]) -> Result<u32, Erro> {
    let esp = STACK_BASE + STACK_SIZE - 0x100;
    bus.escrever_u32(GuestVirt(esp), 0x11000).unwrap();
    for (i, value) in args.into_iter().enumerate() {
        bus.escrever_u32(GuestVirt(esp + 4 + i as u32 * 4), value)
            .unwrap();
    }
    let mut regs = [0; 8];
    regs[4] = esp;
    d.atender(bus, imports::trampoline(46), &regs)
        .map(|r| r.eax)
}

fn bytes(bus: &Bus, at: u32, size: usize) -> Vec<u8> {
    let mut data = vec![0; size];
    bus.ler_bytes(GuestVirt(at), &mut data).unwrap();
    data
}

fn saved(value: &impl Snapshot) -> Vec<u8> {
    let mut out = SnapshotWriter::new();
    value.save(&mut out);
    out.finish()
}

#[test]
fn leitura_pll_tem_bytes_publicos_guardas_e_frequencia_coerente_com_clock() {
    let (mut bus, mut d) = setup();
    bus.escrever_bytes(GuestVirt(OUTPUT - 1), &[0xAA; 6])
        .unwrap();
    assert_eq!(
        call(&mut bus, &mut d, [0, 0x60, 0x6C, OUTPUT, 4, 0]).unwrap(),
        0
    );
    assert_eq!(bytes(&bus, OUTPUT - 1, 6), [0xAA, 1, 8, 0x23, 1, 0xAA]);
    let word = bus.ler_u32(GuestVirt(OUTPUT)).unwrap();
    let divider = u64::from(word & 255);
    let multiplier = u64::from((word >> 8) & 255);
    let cpu_hz = (16_666_666_666u64 / divider * multiplier) * 55 / 10 / 1000;
    assert_eq!(cpu_hz, 733_333_333);
    assert_eq!(cpu_hz, xbox_core::time::CPU_HZ);
    assert_eq!(d.pci().profile(), PciProfile::RetailClockV1);
    assert_eq!(d.atendidas(), 1);
    // A saída é cópia; modificá-la não reprograma PLL.
    bus.escrever_u32(GuestVirt(OUTPUT), 0).unwrap();
    call(&mut bus, &mut d, [0, 0x60, 0x6C, OUTPUT, 4, 0]).unwrap();
    assert_eq!(bus.ler_u32(GuestVirt(OUTPUT)).unwrap(), 0x0123_0801);
}

#[test]
fn subleituras_desalinhadas_cobrem_todos_os_bytes_sem_tocar_guardas() {
    let (mut bus, mut d) = setup();
    let expected = [1, 8, 0x23, 1];
    for start in 0..4usize {
        for length in 1..=4 - start {
            bus.escrever_bytes(GuestVirt(OUTPUT), &[0xAA; 8]).unwrap();
            call(
                &mut bus,
                &mut d,
                [0, 0x60, 0x6C + start as u32, OUTPUT + 1, length as u32, 0],
            )
            .unwrap();
            assert_eq!(
                bytes(&bus, OUTPUT + 1, length),
                expected[start..start + length]
            );
            assert_eq!(bus.ler_u8(GuestVirt(OUTPUT)).unwrap(), 0xAA);
            assert_eq!(
                bus.ler_u8(GuestVirt(OUTPUT + 1 + length as u32)).unwrap(),
                0xAA
            );
        }
    }
    for register in [0x6C, 0x70] {
        assert_eq!(
            call(&mut bus, &mut d, [0, 0x60, register, 0, 0, 0]).unwrap(),
            0
        );
    }
}

#[test]
fn escritas_recusam_e_boolean_usa_somente_byte_baixo() {
    let (mut bus, mut d) = setup();
    bus.escrever_u32(GuestVirt(OUTPUT), 0xDEADBEEF).unwrap();
    for write in [1, 0x101, 255] {
        assert!(matches!(
            call(&mut bus, &mut d, [0, 0x60, 0x6C, OUTPUT, 4, write]),
            Err(Erro::VarianteNaoImplementada { .. })
        ));
        assert_eq!(bus.ler_u32(GuestVirt(OUTPUT)).unwrap(), 0xDEADBEEF);
    }
    call(&mut bus, &mut d, [0, 0x60, 0x6C, OUTPUT, 4, 0x100]).unwrap();
    assert_eq!(bus.ler_u32(GuestVirt(OUTPUT)).unwrap(), 0x0123_0801);
    assert_eq!((d.atendidas(), d.recusadas()), (1, 3));
}

#[test]
fn funcoes_e_regioes_nao_modeladas_recusam_com_diagnostico_e_sem_zeros_ficticios() {
    let (mut bus, mut d) = setup();
    bus.escrever_u32(GuestVirt(OUTPUT), 0xDEADBEEF).unwrap();
    for (number, slot, register, length) in [
        (1, 0x60, 0x6C, 4),
        (0, 0, 0x6C, 4),
        (0, 0x61, 0x6C, 4),
        (0, 0x60, 0, 4),
        (0, 0x60, 0x6B, 2),
        (0, 0x60, 0x6F, 2),
    ] {
        let error = call(
            &mut bus,
            &mut d,
            [number, slot, register, OUTPUT, length, 0],
        )
        .unwrap_err();
        assert!(matches!(error, Erro::VarianteNaoImplementada { .. }));
        let message = error.to_string();
        for fragment in ["HalReadWritePCISpace", "46", "0x00011000", "0x00012100"] {
            assert!(message.contains(fragment), "{message}");
        }
        assert_eq!(bus.ler_u32(GuestVirt(OUTPUT)).unwrap(), 0xDEADBEEF);
    }
}

#[test]
fn coordenadas_pci_e_intervalos_com_wrap_sao_argumentos_invalidos() {
    let (mut bus, mut d) = setup();
    bus.escrever_u32(GuestVirt(OUTPUT), 0xDEADBEEF).unwrap();
    for (number, slot, register, length) in [
        (256, 0x60, 0x6C, 4),
        (0, 0x160, 0x6C, 4),
        (u32::MAX, 0x60, 0x6C, 4),
        (0, 0x60, u32::MAX, 4),
        (0, 0x60, 0x6C, u32::MAX),
        (0, 0x60, 255, 2),
        (0, 0x60, 256, 1),
    ] {
        assert!(matches!(
            call(
                &mut bus,
                &mut d,
                [number, slot, register, OUTPUT, length, 0]
            ),
            Err(Erro::ArgumentoInvalido { .. })
        ));
        assert_eq!(bus.ler_u32(GuestVirt(OUTPUT)).unwrap(), 0xDEADBEEF);
    }
    assert_eq!(d.atendidas(), 0);
}

#[test]
fn ponteiros_invalidos_mmio_e_pagina_parcial_nao_deixam_store_parcial() {
    let (mut bus, mut d) = setup();
    bus.escrever_u16(GuestVirt(0x12FFE), 0xBEEF).unwrap();
    bus.mapear(GuestVirt(0x5000_0000), GuestPhys(0xFD00_0000), 1, Perm::RW)
        .unwrap();
    for pointer in [0, u32::MAX - 1, 0x6000_0000, 0x11000, 0x12FFE, 0x5000_0000] {
        assert!(matches!(
            call(&mut bus, &mut d, [0, 0x60, 0x6C, pointer, 4, 0]),
            Err(Erro::ArgumentoInvalido { .. })
        ));
        assert_eq!(bus.ler_u16(GuestVirt(0x12FFE)).unwrap(), 0xBEEF);
    }
    assert_eq!(d.atendidas(), 0);
}

#[test]
fn duas_paginas_validas_recebem_leitura_e_segunda_ro_recusa_antes_do_primeiro_byte() {
    let (mut bus, mut d) = setup();
    bus.mapear(GuestVirt(0x13000), GuestPhys(0x13000), 1, Perm::RW)
        .unwrap();
    call(&mut bus, &mut d, [0, 0x60, 0x6C, 0x12FFE, 4, 0]).unwrap();
    assert_eq!(bytes(&bus, 0x12FFE, 4), [1, 8, 0x23, 1]);
    bus.escrever_bytes(GuestVirt(0x12FFE), &[0xAA; 4]).unwrap();
    let frame = bus.traduzir(GuestVirt(0x13000), Acesso::Leitura).unwrap();
    bus.mapear(GuestVirt(0x13000), frame, 1, Perm::RO).unwrap();
    assert!(call(&mut bus, &mut d, [0, 0x60, 0x6C, 0x12FFE, 4, 0]).is_err());
    assert_eq!(bytes(&bus, 0x12FFE, 4), [0xAA; 4]);
}

#[test]
fn snapshot_pci_e_kernel_preservam_perfil_e_recusam_payload_invalido_atomicamente() {
    let (mut bus, mut d) = setup();
    call(&mut bus, &mut d, [0, 0x60, 0x6C, OUTPUT, 4, 0]).unwrap();
    let snapshot = saved(&d);
    let mut restored = Dispatcher::new();
    let mut inp = SnapshotReader::new(&snapshot).unwrap();
    restored.load(&mut inp).unwrap();
    assert_eq!(inp.remaining(), 0);
    assert_eq!(saved(&restored), snapshot);
    for (version, id) in [(0, 1), (2, 1), (1, 0), (1, 2), (1, u32::MAX)] {
        let mut out = SnapshotWriter::new();
        out.section("PCIC", version);
        out.u32(id);
        assert!(
            PciConfig::default()
                .load(&mut SnapshotReader::new(&out.finish()).unwrap())
                .is_err()
        );
    }
    for end in 0..snapshot.len() {
        if let Ok(mut input) = SnapshotReader::new(&snapshot[..end]) {
            assert!(restored.load(&mut input).is_err());
            assert_eq!(saved(&restored), snapshot);
        }
    }
    let mut bad = snapshot.clone();
    let profile = bad.windows(4).position(|tag| tag == b"PCIC").unwrap() + 8;
    bad[profile..profile + 4].copy_from_slice(&2u32.to_le_bytes());
    assert!(
        restored
            .load(&mut SnapshotReader::new(&bad).unwrap())
            .is_err()
    );
    assert_eq!(saved(&restored), snapshot);
}

#[test]
fn krnl_v1_e_v2_migram_perfil_pci_sem_perder_estado_anterior() {
    for version in [1, 2] {
        let mut out = SnapshotWriter::new();
        out.section("KRNL", version);
        out.u64(123);
        out.u64(4);
        out.section("KMUT", 2);
        out.u32(4);
        out.u32(0);
        let eeprom = EepromState::supplied([0xAA; 256]);
        if version == 2 {
            eeprom.save(&mut out);
        }
        let bytes = out.finish();
        let mut restored = Dispatcher::new();
        let mut input = SnapshotReader::new(&bytes).unwrap();
        restored.load(&mut input).unwrap();
        assert_eq!(input.remaining(), 0);
        assert_eq!(restored.pci().profile(), PciProfile::RetailClockV1);
        assert_eq!((restored.atendidas(), restored.recusadas()), (123, 4));
        let expected_eeprom = if version == 2 {
            eeprom
        } else {
            EepromState::default()
        };
        assert_eq!(restored.eeprom(), &expected_eeprom);
        let (mut bus, _) = setup();
        call(&mut bus, &mut restored, [0, 0x60, 0x6C, OUTPUT, 4, 0]).unwrap();
        assert_eq!(bus.ler_u32(GuestVirt(OUTPUT)).unwrap(), 0x0123_0801);
    }
}
