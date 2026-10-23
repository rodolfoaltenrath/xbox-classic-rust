use xbox_core::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_mem::{Acesso, Arena, BOOT_DMA_ADDRESS, Bus, GuestPhys, GuestVirt, Perm};
fn bus() -> Bus {
    Bus::com_arena(Arena::com_tamanho(0x4000).unwrap()).unwrap()
}
fn saved(v: &Bus) -> Vec<u8> {
    let mut w = SnapshotWriter::new();
    v.save(&mut w);
    w.finish()
}
fn load(v: &mut Bus, bytes: &[u8]) {
    let mut r = SnapshotReader::new(bytes).unwrap();
    v.load(&mut r).unwrap();
    assert_eq!(r.remaining(), 0);
}
#[test]
fn boot_reserva_frame_zero_com_unico_alias_rw_e_dma_observa_os_bytes() {
    let mut b = bus();
    b.reserve_boot_dma_page().unwrap();
    assert!(b.boot_dma_page().reserved());
    assert_eq!(b.frames().usados(), 1);
    assert_eq!(b.frames_mut().alocar(), Some(GuestPhys(4096)));
    assert_eq!(
        b.mmu().consultar(BOOT_DMA_ADDRESS),
        Some((GuestPhys(0), Perm::RW))
    );
    assert!(b.traduzir(BOOT_DMA_ADDRESS, Acesso::Execucao).is_err());
    assert!(
        b.mmu()
            .consultar(GuestVirt(BOOT_DMA_ADDRESS.0 + 4096))
            .is_none()
    );
    b.escrever_u32(BOOT_DMA_ADDRESS, 0x12345001).unwrap();
    assert_eq!(
        b.dma(GuestPhys(0), 4).unwrap(),
        &0x12345001u32.to_le_bytes()
    );
    b.reserve_boot_dma_page().unwrap();
    assert_eq!(b.ler_u32(BOOT_DMA_ADDRESS).unwrap(), 0x12345001);
    assert_eq!(b.frames().usados(), 2);
}
#[test]
fn instalacao_ocupada_recusa_sem_tomar_frame_alias_ou_zerar_dados() {
    for case in 0..3 {
        let mut b = bus();
        match case {
            0 => b.frames_mut().reservar(GuestPhys(0), 1).unwrap(),
            1 => b
                .mapear(BOOT_DMA_ADDRESS, GuestPhys(4096), 1, Perm::RW)
                .unwrap(),
            _ => b
                .mapear(GuestVirt(0x10000), GuestPhys(0), 1, Perm::RO)
                .unwrap(),
        }
        b.carregar_em_fisico(GuestPhys(0), b"XBEH").unwrap();
        let before = saved(&b);
        assert!(b.reserve_boot_dma_page().is_err());
        assert_eq!(saved(&b), before);
    }
}
#[test]
fn snapshot_preserva_reserva_e_recusa_marcador_sem_posse_versao_e_aliases() {
    let mut b = bus();
    b.reserve_boot_dma_page().unwrap();
    b.escrever_u32(BOOT_DMA_ADDRESS, 42).unwrap();
    let bytes = saved(&b);
    let mut dest = bus();
    load(&mut dest, &bytes);
    assert_eq!(saved(&dest), bytes);
    assert_eq!(dest.ler_u32(BOOT_DMA_ADDRESS).unwrap(), 42);
    for (offset, value) in [(4, 0u32), (4, 2), (8, 2)] {
        let mut bad = bytes.clone();
        let at = bad.len() - 9;
        if offset == 8 {
            bad[at + 8] = value as u8;
        } else {
            bad[at + offset..at + offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        assert!(bus().load(&mut SnapshotReader::new(&bad).unwrap()).is_err());
    }
    for end in bytes.len() - 9..bytes.len() {
        assert!(
            bus()
                .load(&mut SnapshotReader::new(&bytes[..end]).unwrap())
                .is_err()
        );
    }
    for case in 0..4 {
        let mut broken = bus();
        load(&mut broken, &bytes);
        match case {
            0 => broken.frames_mut().liberar(GuestPhys(0), 1),
            1 => broken.mmu_mut().desmapear(BOOT_DMA_ADDRESS, 1),
            2 => broken
                .mapear(BOOT_DMA_ADDRESS, GuestPhys(0), 1, Perm::RX)
                .unwrap(),
            _ => broken
                .mapear(GuestVirt(0x10000), GuestPhys(0), 1, Perm::RO)
                .unwrap(),
        }
        assert!(broken.reserve_boot_dma_page().is_err());
        assert!(
            bus()
                .load(&mut SnapshotReader::new(&saved(&broken)).unwrap())
                .is_err()
        );
    }
}
#[test]
fn legado_bus_v1_a_v5_preserva_headers_no_frame_zero_e_nao_herda_reserva() {
    let mut legacy = bus();
    legacy.frames_mut().reservar(GuestPhys(0), 1).unwrap();
    legacy
        .mapear(GuestVirt(0x10000), GuestPhys(0), 1, Perm::RO)
        .unwrap();
    legacy.carregar_em_fisico(GuestPhys(0), b"XBEH").unwrap();
    let current = saved(&legacy);
    for version in 1u32..=5 {
        let remove =
            9 + if version < 5 {
                xbox_mem::nv2a_pramin::PRAMIN_SIZE + 20
            } else {
                0
            } + if version < 4 { 20 } else { 0 }
                + if version < 3 { 16 } else { 0 }
                + if version < 2 { 12 } else { 0 };
        let mut old = current[..current.len() - remove].to_vec();
        old[16..20].copy_from_slice(&version.to_le_bytes());
        let mut dest = bus();
        dest.reserve_boot_dma_page().unwrap();
        load(&mut dest, &old);
        assert!(!dest.boot_dma_page().reserved());
        assert!(dest.mmu().consultar(BOOT_DMA_ADDRESS).is_none());
        assert_eq!(
            dest.ler_u32(GuestVirt(0x10000)).unwrap(),
            u32::from_le_bytes(*b"XBEH")
        );
        assert!(dest.reserve_boot_dma_page().is_err());
        let migrated = saved(&dest);
        load(&mut dest, &migrated);
        assert_eq!(saved(&dest), migrated);
    }
}
