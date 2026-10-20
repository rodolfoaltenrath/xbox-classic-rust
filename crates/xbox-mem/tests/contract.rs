//! Contrato T1–T12 com arena pequena; T1–T10 podem rodar sob Miri.

use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_mem::dispositivos::{Dispositivo, Motivo};
use xbox_mem::{Acesso, Arena, ArenaOff, Bus, GuestPhys, GuestVirt, MotivoDeFalta, Perm};

fn bus() -> Bus {
    Bus::com_arena(Arena::com_tamanho(64 * 1024).unwrap()).unwrap()
}

#[test]
fn t1_t2_limites_e_overflow_nao_escapam_da_arena() {
    let mut arena = Arena::com_tamanho(8192).unwrap();
    assert!(Arena::com_tamanho(isize::MAX as usize + 1).is_err());
    arena.escrever(ArenaOff(8191), &[7]).unwrap();
    assert_eq!(arena.faixa(ArenaOff(8191), 1).unwrap(), &[7]);
    for (offset, length) in [
        (8192, 1),
        (8193, 1),
        (8191, 4),
        (u32::MAX, 4),
        (1, usize::MAX),
    ] {
        assert!(arena.faixa(ArenaOff(offset), length).is_err());
    }
    let mut bus = bus();
    assert!(bus.dma(GuestPhys(u32::MAX), 4).is_err());
    bus.mapear(GuestVirt(0xFFFFF000), GuestPhys(0), 1, Perm::RW)
        .unwrap();
    bus.escrever_u16(GuestVirt(0xFFFFFFFE), 0x1234).unwrap();
    let fault = bus.escrever_u32(GuestVirt(0xFFFFFFFE), 0).unwrap_err();
    assert_eq!(fault.motivo, MotivoDeFalta::Transbordo);
    assert_eq!(bus.ler_u16(GuestVirt(0xFFFFFFFE)).unwrap(), 0x1234);
}

#[test]
fn t3_t6_desalinhamento_e_falta_na_segunda_pagina() {
    let mut bus = bus();
    bus.mapear(GuestVirt(0x1000), GuestPhys(0), 1, Perm::RW)
        .unwrap();
    bus.escrever_u8(GuestVirt(0x1FFF), 0x12).unwrap();
    let fault = bus.ler_u32(GuestVirt(0x1FFF)).unwrap_err();
    assert_eq!(fault.endereco, GuestVirt(0x2000));
    assert_eq!(fault.codigo_de_erro(), 0);
    assert!(bus.escrever_u64(GuestVirt(0x1FFF), u64::MAX).is_err());
    assert_eq!(bus.ler_u8(GuestVirt(0x1FFF)).unwrap(), 0x12);
    // Frames não contíguos: o acesso deve consultar ambas as traduções.
    bus.mapear(GuestVirt(0x2000), GuestPhys(0x5000), 1, Perm::RW)
        .unwrap();
    for address in [0x1001, 0x1FFD, 0x1FFE, 0x1FFF] {
        bus.escrever_u16(GuestVirt(address), 0x1234).unwrap();
        assert_eq!(bus.ler_u16(GuestVirt(address)).unwrap(), 0x1234);
        bus.escrever_u32(GuestVirt(address), 0x12345678).unwrap();
        assert_eq!(bus.ler_u32(GuestVirt(address)).unwrap(), 0x12345678);
        bus.escrever_u64(GuestVirt(address), 0x0123456789ABCDEF)
            .unwrap();
        assert_eq!(bus.ler_u64(GuestVirt(address)).unwrap(), 0x0123456789ABCDEF);
    }
}

#[test]
fn t4_permissoes_codigos_e_bits_de_acesso() {
    let mut bus = bus();
    bus.mapear(GuestVirt(0x1000), GuestPhys(0), 1, Perm::RO)
        .unwrap();
    assert!(!bus.mmu().acessado(GuestVirt(0x1000)));
    assert_eq!(
        bus.escrever_u8(GuestVirt(0x1000), 1)
            .unwrap_err()
            .codigo_de_erro(),
        3
    );
    assert_eq!(
        bus.traduzir(GuestVirt(0x1000), Acesso::Execucao)
            .unwrap_err()
            .codigo_de_erro(),
        17
    );
    bus.ler_u8(GuestVirt(0x1000)).unwrap();
    assert!(bus.mmu().acessado(GuestVirt(0x1000)));
    assert!(!bus.mmu().sujo(GuestVirt(0x1000)));
    bus.mapear(GuestVirt(0x1000), GuestPhys(0), 1, Perm::RW)
        .unwrap();
    bus.escrever_u8(GuestVirt(0x1000), 1).unwrap();
    assert!(bus.mmu().sujo(GuestVirt(0x1000)));
}

#[test]
fn t5_alias_explicito_de_pagina_grande_respeita_fisico_e_permissoes() {
    let mut bus = bus();
    bus.mapear(GuestVirt(0x1000), GuestPhys(0x3000), 1, Perm::RW)
        .unwrap();
    bus.mmu_mut()
        .mapear_grande(GuestVirt(0x80000000), GuestPhys(0), Perm::RW)
        .unwrap();
    bus.escrever_u32(GuestVirt(0x1000), 0x12345678).unwrap();
    assert_eq!(bus.ler_u32(GuestVirt(0x80003000)).unwrap(), 0x12345678);
    bus.escrever_u8(GuestVirt(0x80003001), 0xAB).unwrap();
    assert_eq!(bus.ler_u32(GuestVirt(0x1000)).unwrap(), 0x1234AB78);
    assert!(
        bus.traduzir(GuestVirt(0x80003000), Acesso::Execucao)
            .is_err()
    );
}

#[test]
fn t7_t8_t9_mmio_buraco_e_tiled_tem_telemetria_e_contexto() {
    let mut bus = bus();
    let context = xbox_mem::Contexto {
        pc: Some(GuestVirt(0x11000)),
        thread: Some(7),
    };
    bus.definir_contexto(context);
    for (physical, device) in [
        (0xFD000000, Dispositivo::Nv2a),
        (0xFE800000, Dispositivo::Apu),
        (0xFEC00000, Dispositivo::Ac97),
        (0xFED00000, Dispositivo::UsbOhci0),
        (0xFED08000, Dispositivo::UsbOhci1),
        (0xFEF00000, Dispositivo::Nic),
    ] {
        bus.mapear(GuestVirt(0x6000), GuestPhys(physical), 1, Perm::RW)
            .unwrap();
        assert_eq!(bus.ler_u32(GuestVirt(0x6040)).unwrap(), 0);
        bus.escrever_u32(GuestVirt(0x6040), 42).unwrap();
        assert!(
            bus.telemetria()
                .registros()
                .iter()
                .filter(|r| r.endereco == GuestPhys(physical + 0x40))
                .all(|r| r.motivo == Motivo::NaoImplementado(device))
        );
    }
    assert_eq!(bus.telemetria().total_de_acessos(), 12);
    assert!(
        bus.telemetria()
            .registros()
            .iter()
            .all(|r| r.primeiro_contexto == context)
    );
    for (physical, tiled) in [(0x40000000, false), (0xF0000000, true)] {
        bus.mapear(GuestVirt(0x6000), GuestPhys(physical), 1, Perm::RW)
            .unwrap();
        let fault = bus.ler_u32(GuestVirt(0x6000)).unwrap_err();
        assert!(!fault.e_falha_de_pagina());
        assert_eq!(
            matches!(fault.motivo, MotivoDeFalta::NaoImplementado { .. }),
            tiled
        );
    }
    assert_eq!(bus.telemetria().total_de_acessos(), 14);
}

#[test]
fn t10_snapshot_restaura_ram_mmu_frames_e_bytes_deterministicos() {
    fn save(bus: &Bus) -> Vec<u8> {
        let mut w = SnapshotWriter::new();
        bus.save(&mut w);
        w.finish()
    }
    let mut original = bus();
    let frame = original.frames_mut().alocar().unwrap();
    original
        .mapear(GuestVirt(0x1000), frame, 1, Perm::RW)
        .unwrap();
    original
        .escrever_u32(GuestVirt(0x1003), 0x12345678)
        .unwrap();
    let bytes = save(&original);
    assert_eq!(save(&original), bytes);
    let mut restored = bus();
    restored
        .load(&mut SnapshotReader::new(&bytes).unwrap())
        .unwrap();
    assert_eq!(save(&restored), bytes);
    assert_eq!(restored.ler_u32(GuestVirt(0x1003)).unwrap(), 0x12345678);
    assert_ne!(restored.frames_mut().alocar().unwrap(), frame);
}

#[test]
#[cfg_attr(
    miri,
    ignore = "T13 cobre T1–T10; traço de 20000 acessos roda nativamente"
)]
fn t11_t12_traco_independente_de_20000_acessos_e_diferencial_com_vetor() {
    let mut bus = bus();
    bus.mapear(GuestVirt(0x1000), GuestPhys(0x2000), 4, Perm::RW)
        .unwrap();
    let mut model = vec![0; 0x4000];
    let mut seed = 0x12345678u32;
    let mut next = || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        seed
    };
    for _ in 0..20000 {
        let address = next() & 0xFFFF;
        let length = (next() % 9) as usize;
        let data = next().to_le_bytes();
        let input: Vec<_> = (0..length).map(|i| data[i % 4]).collect();
        let valid =
            length == 0 || (address >= 0x1000 && u64::from(address) + length as u64 <= 0x5000);
        let write = bus.escrever_bytes(GuestVirt(address), &input);
        assert_eq!(write.is_ok(), valid);
        if valid && length != 0 {
            model[address as usize - 0x1000..address as usize - 0x1000 + length]
                .copy_from_slice(&input);
        }
        let mut actual = vec![0; length];
        assert_eq!(
            bus.ler_bytes(GuestVirt(address), &mut actual).is_ok(),
            valid
        );
        if valid {
            assert_eq!(actual, input);
        }
    }
    assert_eq!(
        bus.dma(GuestPhys(0x2000), model.len() as u32).unwrap(),
        model
    );
}
