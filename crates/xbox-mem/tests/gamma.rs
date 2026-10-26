use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_mem::{
    Arena, Bus, GuestPhys, GuestVirt, Perm,
    nv2a_gamma::{GammaProfile, SNAPSHOT_BYTES},
};

const INDEX: GuestVirt = GuestVirt(0x600003C8);
const DATA: GuestVirt = GuestVirt(0x600003C9);
fn bus() -> Bus {
    let mut bus = Bus::com_arena(Arena::com_tamanho(64 * 1024).unwrap()).unwrap();
    bus.mapear(GuestVirt(0x60000000), GuestPhys(0xFD681000), 1, Perm::RW)
        .unwrap();
    bus
}
fn saved(bus: &Bus) -> Vec<u8> {
    let mut out = SnapshotWriter::new();
    bus.save(&mut out);
    out.finish()
}
fn load(bus: &mut Bus, data: &[u8]) {
    let mut reader = SnapshotReader::new(data).unwrap();
    bus.load(&mut reader).unwrap();
    assert_eq!(reader.remaining(), 0);
}

#[test]
fn identidade_e_programacao_parcial_rgb_por_alias_sem_mudar_ram_ou_irq() {
    let mut bus = bus();
    for i in 0..256 {
        assert_eq!(bus.nv2a_gamma().palette().unwrap()[i], [i as u8; 3]);
    }
    bus.escrever_u8(INDEX, 42).unwrap();
    bus.escrever_fisico(GuestPhys(0xFD6813C9), &[7]).unwrap();
    assert_eq!(bus.nv2a_gamma().palette().unwrap()[42], [7, 42, 42]);
    assert_eq!(bus.nv2a_gamma().cursor(), 127);
    bus.escrever_u8(DATA, 8).unwrap();
    bus.escrever_u8(DATA, 9).unwrap();
    assert_eq!(bus.nv2a_gamma().palette().unwrap()[42], [7, 8, 9]);
    assert_eq!(bus.ler_u8(INDEX).unwrap(), 43);
    assert_eq!(bus.telemetria().total_de_acessos(), 0);
    assert!(bus.arena().bytes().iter().all(|v| *v == 0));
    assert!(!bus.gpu_irq_line_asserted());
}

#[test]
fn fluxo_completo_wrap_publico_e_indice_reinicia_canal_sem_apagar_outros() {
    let mut bus = bus();
    bus.escrever_u8(INDEX, 255).unwrap();
    for v in [1, 2, 3, 4, 5, 6] {
        bus.escrever_u8(DATA, v).unwrap();
    }
    assert_eq!(bus.nv2a_gamma().palette().unwrap()[255], [1, 2, 3]);
    assert_eq!(bus.nv2a_gamma().palette().unwrap()[0], [4, 5, 6]);
    assert_eq!(bus.ler_u8(INDEX).unwrap(), 1);
    bus.escrever_u8(DATA, 100).unwrap();
    bus.escrever_u8(INDEX, 1).unwrap();
    bus.escrever_u8(DATA, 200).unwrap();
    assert_eq!(bus.nv2a_gamma().palette().unwrap()[1], [200, 1, 1]);
    bus.escrever_u8(INDEX, 0).unwrap();
    for i in 0..=255u8 {
        for v in [255 - i, i, 0] {
            bus.escrever_u8(DATA, v).unwrap();
        }
    }
    assert_eq!(bus.nv2a_gamma().cursor(), 0);
    assert_eq!(bus.nv2a_gamma().palette().unwrap()[254], [1, 254, 0]);
}

#[test]
fn largura_invalida_e_leitura_sem_modelo_recusam_antes_de_efeitos() {
    let mut bus = bus();
    let before = bus.nv2a_gamma().clone();
    assert!(bus.escrever_u16(INDEX, 0xFFFF).is_err());
    assert!(bus.escrever_u32(DATA, 0xFFFFFFFF).is_err());
    assert!(bus.escrever_fisico(GuestPhys(0xFD6813C7), &[1, 2]).is_err());
    assert!(bus.ler_u16(INDEX).is_err());
    assert!(bus.ler_u8(DATA).is_err());
    bus.mapear(GuestVirt(0x1000), GuestPhys(0), 1, Perm::RW)
        .unwrap();
    bus.mapear(GuestVirt(0x2000), GuestPhys(0xFD681000), 1, Perm::RW)
        .unwrap();
    // Um byte em RAM + MMIO até incluir os offsets 0x3C8/0x3C9.
    assert!(bus.escrever_bytes(GuestVirt(0x1FFF), &[1; 971]).is_err());
    assert_eq!(bus.ler_u8(GuestVirt(0x1FFF)).unwrap(), 0);
    assert_eq!(bus.nv2a_gamma(), &before);
    assert_eq!(bus.telemetria().total_de_acessos(), 0);
}

#[test]
fn snapshot_no_meio_do_rgb_reproduz_cursor_paleta_e_continuacao() {
    let mut source = bus();
    source.escrever_u8(INDEX, 200).unwrap();
    source.escrever_u8(DATA, 13).unwrap();
    let checkpoint = saved(&source);
    let mut restored = bus();
    load(&mut restored, &checkpoint);
    assert_eq!(saved(&restored), checkpoint);
    for target in [&mut source, &mut restored] {
        target.escrever_u8(DATA, 14).unwrap();
        target.escrever_u8(DATA, 15).unwrap();
    }
    assert_eq!(saved(&restored), saved(&source));
    assert_eq!(restored.nv2a_gamma().palette().unwrap()[200], [13, 14, 15]);
}

#[test]
fn snapshot_recusa_versao_perfil_cursor_legado_incoerente_e_truncamento() {
    let data = saved(&bus());
    let at = data.len() - SNAPSHOT_BYTES;
    assert_eq!(&data[at..at + 4], b"GAMM");
    for (offset, bytes) in [
        (at + 4, 2u32.to_le_bytes()),
        (at + 4, 0u32.to_le_bytes()),
        (at + 8, 2u32.to_le_bytes()),
        (at + 8, 0u32.to_le_bytes()),
    ] {
        let mut bad = data.clone();
        bad[offset..offset + 4].copy_from_slice(&bytes);
        assert!(bus().load(&mut SnapshotReader::new(&bad).unwrap()).is_err());
    }
    let mut bad = data.clone();
    bad[at + 12..at + 14].copy_from_slice(&768u16.to_le_bytes());
    assert!(bus().load(&mut SnapshotReader::new(&bad).unwrap()).is_err());
    for end in [at, at + 4, at + 8, at + 12, at + 13, data.len() - 1] {
        assert!(
            bus()
                .load(&mut SnapshotReader::new(&data[..end]).unwrap())
                .is_err()
        );
    }
}

#[test]
fn bus_v9_migra_sem_gamma_e_preserva_ausencia_ao_regravar() {
    let mut source = bus();
    source.escrever_u8(DATA, 123).unwrap();
    let mut old = saved(&source);
    old.truncate(old.len() - SNAPSHOT_BYTES);
    old[16..20].copy_from_slice(&9u32.to_le_bytes());
    let mut restored = bus();
    load(&mut restored, &old);
    assert_eq!(
        restored.nv2a_gamma().profile(),
        GammaProfile::LegacyUnmodeled
    );
    assert!(restored.nv2a_gamma().palette().is_none());
    restored.escrever_u8(INDEX, 0).unwrap();
    restored.escrever_u8(DATA, 99).unwrap();
    assert_eq!(restored.telemetria().total_de_acessos(), 2);
    let checkpoint = saved(&restored);
    load(&mut source, &checkpoint);
    assert_eq!(saved(&source), checkpoint);
    assert!(source.nv2a_gamma().palette().is_none());
}
