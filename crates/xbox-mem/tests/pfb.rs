mod support;
use support::without_pfifo;
use xbox_core::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_mem::{Arena, Bus, GuestPhys, GuestVirt, MotivoDeFalta, Perm, PfbProfile};
const CFG0: GuestVirt = GuestVirt(0x50000200);
const CFG1: GuestVirt = GuestVirt(0x50000204);
fn bus() -> Bus {
    let mut bus = Bus::com_arena(Arena::com_tamanho(64 << 20).unwrap()).unwrap();
    for va in [0x50000000, 0x60000000] {
        bus.mapear(GuestVirt(va), GuestPhys(0xFD100000), 1, Perm::RW)
            .unwrap();
    }
    bus
}
fn saved(bus: &Bus) -> Vec<u8> {
    let mut out = SnapshotWriter::new();
    bus.save(&mut out);
    out.finish()
}
fn load(bus: &mut Bus, bytes: &[u8]) {
    let mut reader = SnapshotReader::new(bytes).unwrap();
    bus.load(&mut reader).unwrap();
    assert_eq!(reader.remaining(), 0);
}
#[test]
fn perfil_64_mib_e_alias_fisico_observam_configuracao_unica_sem_tocar_ram_irq() {
    let mut bus = bus();
    assert_eq!(bus.nv2a_pfb().profile(), PfbProfile::FirmwareMemoryV1);
    assert_eq!(bus.ler_u32(CFG0).unwrap(), 0x03070003);
    assert_eq!(bus.ler_u32(CFG1).unwrap(), 0x11448000);
    bus.escrever_u32(CFG0, 0xDEADBEEF).unwrap();
    assert_eq!(bus.ler_u32(GuestVirt(0x60000200)).unwrap(), 0xDEADBEEF);
    bus.escrever_fisico(GuestPhys(0xFD100204), &0x12345678u32.to_le_bytes())
        .unwrap();
    assert_eq!(bus.ler_u32(CFG1).unwrap(), 0x12345678);
    let mut bytes = [0; 4];
    bus.ler_fisico(GuestPhys(0xFD100200), &mut bytes).unwrap();
    assert_eq!(bytes, 0xDEADBEEFu32.to_le_bytes());
    assert_eq!(bus.telemetria().total_de_acessos(), 0);
    assert!(!bus.nv2a_irq().line_asserted());
    assert!(bus.arena().bytes().iter().all(|&v| v == 0));
    // Offset vizinho não ganha comportamento por acidente.
    assert_eq!(bus.ler_u32(GuestVirt(CFG1.0 + 4)).unwrap(), 0);
    assert_eq!(bus.telemetria().total_de_acessos(), 1);
}
#[test]
fn larguras_sobreposicoes_e_acesso_fisico_invalido_recusam_sem_programacao() {
    let mut bus = bus();
    let before = bus.nv2a_pfb().clone();
    for (offset, len) in [(0, 1), (4, 2), (1, 4), (-1, 4), (0, 8), (7, 2)] {
        let addr = GuestVirt(CFG0.0.wrapping_add_signed(offset));
        let mut bytes = vec![0xAA; len];
        assert!(matches!(
            bus.ler_bytes(addr, &mut bytes).unwrap_err().motivo,
            MotivoDeFalta::NaoImplementado { .. }
        ));
        assert_eq!(bytes, vec![0xAA; len]);
        assert!(bus.escrever_bytes(addr, &bytes).is_err());
        assert_eq!(bus.nv2a_pfb(), &before);
    }
    assert!(
        bus.escrever_fisico(GuestPhys(0xFD100203), &[1, 2, 3, 4])
            .is_err()
    );
    assert_eq!(bus.nv2a_pfb(), &before);
    assert_eq!(bus.telemetria().total_de_acessos(), 0);
}
#[test]
fn preflight_ram_e_mmio_recusa_antes_de_qualquer_store() {
    let mut bus = bus();
    let frame = bus.frames_mut().alocar().unwrap();
    bus.mapear(GuestVirt(0x1F000), frame, 1, Perm::RW).unwrap();
    bus.mapear(GuestVirt(0x20000), GuestPhys(0xFD100000), 1, Perm::RW)
        .unwrap();
    bus.escrever_u16(GuestVirt(0x1FFFE), 0xBEEF).unwrap();
    assert!(
        bus.escrever_bytes(GuestVirt(0x1FFFE), &vec![0xAA; 0x206])
            .is_err()
    );
    assert_eq!(bus.ler_u16(GuestVirt(0x1FFFE)).unwrap(), 0xBEEF);
    assert_eq!(bus.ler_u32(CFG0).unwrap(), 0x03070003);
    assert_eq!(bus.telemetria().total_de_acessos(), 0);
}
#[test]
fn snapshot_preserva_configuracao_programada_e_demais_dispositivos() {
    let mut source = bus();
    source.escrever_u32(CFG0, 0).unwrap();
    source.escrever_u32(CFG1, u32::MAX).unwrap();
    source.nv2a_irq_mut().raise_vblank();
    source
        .escrever_fisico(GuestPhys(0xFD680500), &0x00010902u32.to_le_bytes())
        .unwrap();
    let bytes = saved(&source);
    let mut dest = bus();
    load(&mut dest, &bytes);
    assert_eq!(saved(&dest), bytes);
    assert_eq!(dest.ler_u32(CFG0).unwrap(), 0);
    assert_eq!(dest.ler_u32(CFG1).unwrap(), u32::MAX);
    assert_eq!(dest.nv2a_irq(), source.nv2a_irq());
    assert_eq!(dest.nv2a_pramdac(), source.nv2a_pramdac());
}
#[test]
fn snapshot_recusa_metadados_invalidos_e_truncamentos() {
    let bytes = saved(&bus());
    let tag = without_pfifo(&bytes).len() - 9 - xbox_mem::nv2a_pramin::PRAMIN_SIZE - 20 - 20;
    assert_eq!(&bytes[tag..tag + 4], b"PFBM");
    let mut dest = bus();
    for (offset, value) in [(tag + 4, 0u32), (tag + 4, 2), (tag + 8, 9), (tag + 8, 0)] {
        let mut bad = bytes.clone();
        bad[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        assert!(dest.load(&mut SnapshotReader::new(&bad).unwrap()).is_err());
    }
    for end in tag..tag + 20 {
        assert!(
            dest.load(&mut SnapshotReader::new(&bytes[..end]).unwrap())
                .is_err()
        );
    }
}
#[test]
fn migracao_bus_v1_a_v3_preserva_ausencia_de_pfb_e_replay_legado() {
    let mut source = bus();
    source.nv2a_irq_mut().raise_vblank();
    let current_full = saved(&source);
    let current = without_pfifo(&current_full);
    for version in 1u32..=3 {
        let remove = 9
            + xbox_mem::nv2a_pramin::PRAMIN_SIZE
            + 20
            + 20
            + if version < 3 { 16 } else { 0 }
            + if version < 2 { 12 } else { 0 };
        let mut old = current[..current.len() - remove].to_vec();
        old[16..20].copy_from_slice(&version.to_le_bytes());
        let mut dest = bus();
        load(&mut dest, &old);
        assert_eq!(dest.nv2a_pfb().profile(), PfbProfile::LegacyUnmodeled);
        assert_eq!(dest.ler_u32(CFG0).unwrap(), 0);
        dest.escrever_u32(CFG0, 0x03070003).unwrap();
        assert_eq!(dest.ler_u32(CFG0).unwrap(), 0);
        assert_eq!(dest.telemetria().total_de_acessos(), 3);
        assert_eq!(
            dest.nv2a_pramdac().coefficient(),
            if version == 3 { Some(0x00011C01) } else { None }
        );
        assert_eq!(
            dest.nv2a_irq().read(0x600100),
            Some(u32::from(version >= 2))
        );
        let checkpoint = saved(&dest);
        let mut replay = bus();
        load(&mut replay, &checkpoint);
        assert_eq!(saved(&replay), checkpoint);
    }
}
