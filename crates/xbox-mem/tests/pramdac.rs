use xbox_core::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_mem::{Arena, Bus, GuestPhys, GuestVirt, MotivoDeFalta, Perm, PramdacProfile};
const PLL: GuestVirt = GuestVirt(0x50000500);
fn bus() -> Bus {
    let mut bus = Bus::com_arena(Arena::com_tamanho(0x4000).unwrap()).unwrap();
    bus.mapear(GuestVirt(0x50000000), GuestPhys(0xFD680000), 1, Perm::RW)
        .unwrap();
    bus.mapear(GuestVirt(0x60000000), GuestPhys(0xFD680000), 1, Perm::RW)
        .unwrap();
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
fn reset_publico_e_frequencia_derivada_sem_telemetria_ou_modificar_ram_irq() {
    let bus = bus();
    assert_eq!(
        bus.nv2a_pramdac().profile(),
        PramdacProfile::PublicReferenceV1
    );
    assert_eq!(bus.ler_u32(PLL).unwrap(), 0x00011C01);
    assert_eq!(bus.nv2a_pramdac().core_clock_hz(), Some(233333324));
    let mut physical = [0; 4];
    bus.ler_fisico(GuestPhys(0xFD680500), &mut physical)
        .unwrap();
    assert_eq!(physical, [1, 28, 1, 0]);
    assert_eq!(bus.telemetria().total_de_acessos(), 0);
    assert!(!bus.nv2a_irq().line_asserted());
    assert!(bus.arena().bytes().iter().all(|&v| v == 0));
    assert_eq!(bus.ler_u32(GuestVirt(PLL.0 + 4)).unwrap(), 0);
    assert_eq!(bus.telemetria().total_de_acessos(), 1);
}
#[test]
fn programacao_por_alias_le_raw_e_calcula_m_n_p_sem_dividir_por_zero() {
    let mut bus = bus();
    for (coefficient, hz) in [
        (0, 0),
        (0x00011C00, 0),
        (1, 0),
        (0x00010902, 37499998),
        (u32::MAX, 130208),
    ] {
        bus.escrever_u32(GuestVirt(0x60000500), coefficient)
            .unwrap();
        assert_eq!(bus.ler_u32(PLL).unwrap(), coefficient);
        assert_eq!(bus.nv2a_pramdac().core_clock_hz(), Some(hz));
    }
    assert_eq!(bus.telemetria().total_de_acessos(), 0);
}
#[test]
fn larguras_e_sobreposicoes_sem_modelo_recusam_sem_programar() {
    let mut bus = bus();
    for (offset, len) in [(0, 1), (0, 2), (0, 8), (1, 4), (3, 1), (-1, 4), (-4, 8)] {
        let address = GuestVirt(PLL.0.wrapping_add_signed(offset));
        let mut bytes = vec![0xAA; len];
        assert!(matches!(
            bus.ler_bytes(address, &mut bytes).unwrap_err().motivo,
            MotivoDeFalta::NaoImplementado { .. }
        ));
        assert!(matches!(
            bus.escrever_bytes(address, &bytes).unwrap_err().motivo,
            MotivoDeFalta::NaoImplementado { .. }
        ));
        assert_eq!(bus.nv2a_pramdac().coefficient(), Some(0x00011C01));
    }
}
#[test]
fn preflight_recusa_mmio_depois_de_ram_sem_store_parcial() {
    let mut bus = bus();
    let frame = bus.frames_mut().alocar().unwrap();
    bus.mapear(GuestVirt(0x1F000), frame, 1, Perm::RW).unwrap();
    bus.mapear(GuestVirt(0x20000), GuestPhys(0xFD680000), 1, Perm::RW)
        .unwrap();
    bus.escrever_u16(GuestVirt(0x1FFFE), 0xBEEF).unwrap();
    let bytes = vec![0xAA; 0x506];
    assert!(bus.escrever_bytes(GuestVirt(0x1FFFE), &bytes).is_err());
    assert_eq!(bus.ler_u16(GuestVirt(0x1FFFE)).unwrap(), 0xBEEF);
    assert_eq!(bus.nv2a_pramdac().coefficient(), Some(0x00011C01));
    assert_eq!(bus.telemetria().total_de_acessos(), 0);
}
#[test]
fn bus_v3_replay_preserva_coeficiente_e_recusa_perfil_versao_truncamento() {
    let mut source = bus();
    source.escrever_u32(PLL, 0x00010902).unwrap();
    source.nv2a_irq_mut().raise_vblank();
    let bytes = saved(&source);
    let mut destination = bus();
    load(&mut destination, &bytes);
    assert_eq!(saved(&destination), bytes);
    assert_eq!(destination.nv2a_pramdac().core_clock_hz(), Some(37499998));
    assert_eq!(destination.nv2a_irq(), source.nv2a_irq());
    let tag = bytes.len() - 9 - xbox_mem::nv2a_pramin::PRAMIN_SIZE - 20 - 20 - 16;
    assert_eq!(&bytes[tag..tag + 4], b"NPLL");
    for (offset, value) in [(tag + 4, 0u32), (tag + 4, 2), (tag + 8, 9), (tag + 8, 0)] {
        let mut bad = bytes.clone();
        bad[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        assert!(bus().load(&mut SnapshotReader::new(&bad).unwrap()).is_err());
    }
    for end in tag..tag + 16 {
        assert!(
            bus()
                .load(&mut SnapshotReader::new(&bytes[..end]).unwrap())
                .is_err()
        );
    }
}
#[test]
fn bus_v1_v2_migram_sem_herdar_perfil_e_o_legado_sobrevive_novo_snapshot() {
    let mut source = bus();
    source.nv2a_irq_mut().raise_vblank();
    let current = saved(&source);
    for version in [1u32, 2] {
        let mut old = current
            [..current.len() - 9 - xbox_mem::nv2a_pramin::PRAMIN_SIZE - 20 - 20 - 16]
            .to_vec();
        if version == 1 {
            old.truncate(old.len() - 12);
        }
        old[16..20].copy_from_slice(&version.to_le_bytes());
        let mut destination = bus();
        destination.escrever_u32(PLL, 0x00010902).unwrap();
        load(&mut destination, &old);
        assert_eq!(
            destination.nv2a_pramdac().profile(),
            PramdacProfile::LegacyUnmodeled
        );
        assert_eq!(destination.nv2a_pramdac().core_clock_hz(), None);
        assert_eq!(destination.ler_u32(PLL).unwrap(), 0);
        destination.escrever_u32(PLL, 0x00011C01).unwrap();
        assert_eq!(destination.ler_u32(PLL).unwrap(), 0);
        assert_eq!(destination.telemetria().total_de_acessos(), 3);
        let checkpoint = saved(&destination);
        let mut replay = bus();
        load(&mut replay, &checkpoint);
        assert_eq!(saved(&replay), checkpoint);
        assert_eq!(
            replay.nv2a_pramdac().profile(),
            PramdacProfile::LegacyUnmodeled
        );
    }
}
