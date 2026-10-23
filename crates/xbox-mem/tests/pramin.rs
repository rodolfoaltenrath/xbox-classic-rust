use xbox_core::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_mem::nv2a_pramin::PRAMIN_SIZE;
use xbox_mem::{Arena, Bus, GuestPhys, GuestVirt, Perm, PraminProfile};
const START: GuestVirt = GuestVirt(0x50000000);
fn bus() -> Bus {
    let mut bus = Bus::com_arena(Arena::com_tamanho(0x4000).unwrap()).unwrap();
    for va in [0x50000000, 0x60000000] {
        bus.mapear(GuestVirt(va), GuestPhys(0xFD700000), 256, Perm::RW)
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
fn instancias_ramht_e_limites_usam_mesmo_backing_em_aliases_e_fisico() {
    let mut bus = bus();
    assert_eq!(bus.nv2a_pramin().profile(), PraminProfile::IndependentV1);
    for offset in [0, 0x10000, 0x110A0, 0x1386C, 0xFFFFC] {
        assert_eq!(bus.ler_u32(GuestVirt(START.0 + offset)).unwrap(), 0);
        bus.escrever_u32(GuestVirt(START.0 + offset), offset ^ 0xCAFEBABE)
            .unwrap();
        assert_eq!(
            bus.ler_u32(GuestVirt(0x60000000 + offset)).unwrap(),
            offset ^ 0xCAFEBABE
        );
        let mut bytes = [0; 4];
        bus.ler_fisico(GuestPhys(0xFD700000 + offset), &mut bytes)
            .unwrap();
        assert_eq!(u32::from_le_bytes(bytes), offset ^ 0xCAFEBABE);
        assert_eq!(
            bus.nv2a_pramin().read(0x700000 + offset),
            Some(offset ^ 0xCAFEBABE)
        );
    }
    bus.escrever_fisico(GuestPhys(0xFD7110A0), &0xDEADBEEFu32.to_le_bytes())
        .unwrap();
    assert_eq!(
        bus.ler_u32(GuestVirt(START.0 + 0x110A0)).unwrap(),
        0xDEADBEEF
    );
    assert_eq!(bus.telemetria().total_de_acessos(), 0);
    assert_eq!(bus.frames().usados(), 0);
    assert!(!bus.nv2a_irq().line_asserted());
    assert!(bus.arena().bytes().iter().all(|&v| v == 0));
    assert!(bus.dma(GuestPhys(0xFD700000), 4).is_err());
    // Fora da janela continua sem modelo, sem espelhar o primeiro word.
    bus.escrever_fisico(GuestPhys(0xFD800000), &42u32.to_le_bytes())
        .unwrap();
    let mut bytes = [1; 4];
    bus.ler_fisico(GuestPhys(0xFD800000), &mut bytes).unwrap();
    assert_eq!(bytes, [0; 4]);
    assert_eq!(bus.telemetria().total_de_acessos(), 2);
    assert_eq!(bus.nv2a_pramin().read(0x800000), None);
}
#[test]
fn larguras_e_sobreposicoes_invalidas_recusam_sem_alterar_destino_ou_instancias() {
    let mut bus = bus();
    bus.escrever_u32(START, 0xCAFEBABE).unwrap();
    let before = bus.nv2a_pramin().clone();
    for (offset, len) in [
        (0, 1),
        (0, 2),
        (0, 8),
        (1, 4),
        (0xFFF, 4),
        (0xFFC, 8),
        (0xFFFFC, 8),
        (0xFFFFF, 4),
    ] {
        let mut bytes = vec![0xAA; len];
        assert!(
            bus.ler_bytes(GuestVirt(START.0 + offset), &mut bytes)
                .is_err()
        );
        assert_eq!(bytes, vec![0xAA; len]);
        assert!(
            bus.escrever_bytes(GuestVirt(START.0 + offset), &bytes)
                .is_err()
        );
        assert_eq!(bus.nv2a_pramin(), &before);
    }
    for (pa, len) in [(0xFD6FFFFF, 4), (0xFD7FFFFC, 8), (0xFD700001, 4)] {
        assert!(
            bus.escrever_fisico(GuestPhys(pa), &vec![0xAA; len])
                .is_err()
        );
        let mut bytes = vec![0xAA; len];
        assert!(bus.ler_fisico(GuestPhys(pa), &mut bytes).is_err());
        assert_eq!(bytes, vec![0xAA; len]);
    }
    assert_eq!(bus.nv2a_pramin(), &before);
    assert_eq!(bus.telemetria().total_de_acessos(), 0);
}
#[test]
fn preflight_ram_pramin_nao_deixa_store_parcial() {
    let mut bus = bus();
    let frame = bus.frames_mut().alocar().unwrap();
    bus.mapear(GuestVirt(0x1F000), frame, 1, Perm::RW).unwrap();
    bus.mapear(GuestVirt(0x20000), GuestPhys(0xFD700000), 1, Perm::RW)
        .unwrap();
    bus.escrever_u16(GuestVirt(0x1FFFE), 0xBEEF).unwrap();
    assert!(bus.escrever_bytes(GuestVirt(0x1FFFE), &[0xAA; 6]).is_ok()); // RAM + um word válido.
    bus.escrever_u16(GuestVirt(0x1FFFE), 0xBEEF).unwrap();
    let before = bus.nv2a_pramin().clone();
    assert!(bus.escrever_bytes(GuestVirt(0x1FFFE), &[0xBB; 7]).is_err());
    assert_eq!(bus.ler_u16(GuestVirt(0x1FFFE)).unwrap(), 0xBEEF);
    assert_eq!(bus.nv2a_pramin(), &before);
    assert_eq!(bus.telemetria().total_de_acessos(), 0);
}
#[test]
fn snapshot_preserva_instancias_altas_e_replay_sem_interferir_ram_pll_irq() {
    let mut source = bus();
    source
        .escrever_u32(GuestVirt(START.0 + 0xFFFFC), 0xCAFEBABE)
        .unwrap();
    source
        .escrever_u32(GuestVirt(START.0 + 0x110A0), 0xDEADBEEF)
        .unwrap();
    source.nv2a_irq_mut().raise_vblank();
    source
        .escrever_fisico(GuestPhys(0xFD680500), &0x00010902u32.to_le_bytes())
        .unwrap();
    let bytes = saved(&source);
    let mut dest = bus();
    load(&mut dest, &bytes);
    assert_eq!(saved(&dest), bytes);
    assert_eq!(dest.nv2a_pramin(), source.nv2a_pramin());
    assert_eq!(dest.nv2a_irq(), source.nv2a_irq());
    assert_eq!(dest.nv2a_pramdac(), source.nv2a_pramdac());
    dest.escrever_u32(GuestVirt(START.0 + 0x110A0), 42).unwrap();
    load(&mut dest, &bytes);
    assert_eq!(
        dest.ler_u32(GuestVirt(START.0 + 0x110A0)).unwrap(),
        0xDEADBEEF
    );
}
#[test]
fn snapshot_recusa_versao_perfil_tamanho_e_payload_truncado() {
    let bytes = saved(&bus());
    let at = bytes.len() - 9 - PRAMIN_SIZE - 20;
    assert_eq!(&bytes[at..at + 4], b"PRAM");
    let mut dest = bus();
    for (offset, value) in [(4, 0u32), (4, 2), (8, 2), (8, 0)] {
        let mut bad = bytes.clone();
        bad[at + offset..at + offset + 4].copy_from_slice(&value.to_le_bytes());
        assert!(dest.load(&mut SnapshotReader::new(&bad).unwrap()).is_err());
    }
    for len in [
        0u64,
        1,
        PRAMIN_SIZE as u64 - 1,
        PRAMIN_SIZE as u64 + 1,
        u64::MAX,
    ] {
        let mut bad = bytes.clone();
        bad[at + 12..at + 20].copy_from_slice(&len.to_le_bytes());
        assert!(dest.load(&mut SnapshotReader::new(&bad).unwrap()).is_err());
    }
    for end in [
        at,
        at + 4,
        at + 8,
        at + 12,
        at + 19,
        at + 20,
        at + 21,
        bytes.len() - 1,
    ] {
        assert!(
            dest.load(&mut SnapshotReader::new(&bytes[..end]).unwrap())
                .is_err()
        );
    }
}
#[test]
fn bus_v1_a_v4_migram_sem_pramin_e_sem_herdar_conteudo_do_destino() {
    let mut source = bus();
    source.nv2a_irq_mut().raise_vblank();
    source.escrever_u32(START, 0xCAFEBABE).unwrap();
    let current = saved(&source);
    for version in 1u32..=4 {
        let remove = 9
            + PRAMIN_SIZE
            + 20
            + if version < 4 { 20 } else { 0 }
            + if version < 3 { 16 } else { 0 }
            + if version < 2 { 12 } else { 0 };
        let mut old = current[..current.len() - remove].to_vec();
        old[16..20].copy_from_slice(&version.to_le_bytes());
        let mut dest = bus();
        dest.escrever_u32(START, 42).unwrap();
        load(&mut dest, &old);
        assert_eq!(dest.nv2a_pramin().profile(), PraminProfile::LegacyUnmodeled);
        assert_eq!(dest.ler_u32(START).unwrap(), 0);
        dest.escrever_u32(START, 0xCAFEBABE).unwrap();
        assert_eq!(dest.ler_u32(START).unwrap(), 0);
        assert_eq!(dest.telemetria().total_de_acessos(), 3);
        assert_eq!(
            dest.nv2a_pramdac().coefficient(),
            if version >= 3 { Some(0x00011C01) } else { None }
        );
        assert_eq!(
            dest.nv2a_irq().read(0x600100),
            Some(u32::from(version >= 2))
        );
        let checkpoint = saved(&dest);
        assert_eq!(
            &checkpoint[checkpoint.len() - 29..checkpoint.len() - 25],
            b"PRAM"
        );
        let mut replay = bus();
        load(&mut replay, &checkpoint);
        assert_eq!(saved(&replay), checkpoint);
    }
}
