use xbox_core::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_formats::fixtures::xbe_com_codigo;
use xbox_recomp::runtime::{SchedulingProfile, Session};
fn loaded() -> Session {
    Session::load(&xbe_com_codigo(&[0xC3]).unwrap()).unwrap()
}
fn saved(s: &Session) -> Vec<u8> {
    let mut w = SnapshotWriter::new();
    s.save(&mut w);
    w.finish()
}
#[test]
fn aots_v3_migra_bootstrap_sem_ativar_scheduler_no_restore_ou_replay() {
    let mut old = saved(&loaded());
    let at = old.windows(4).rposition(|v| v == b"SCHD").unwrap();
    old.truncate(at);
    old[16..20].copy_from_slice(&3u32.to_le_bytes());
    let mut r = loaded();
    r.load(&mut SnapshotReader::new(&old).unwrap()).unwrap();
    assert_eq!(r.scheduling_profile(), SchedulingProfile::LegacyBootstrap);
    let migrated = saved(&r);
    let mut destination = loaded();
    destination
        .load(&mut SnapshotReader::new(&migrated).unwrap())
        .unwrap();
    assert_eq!(
        destination.scheduling_profile(),
        SchedulingProfile::LegacyBootstrap
    );
    assert_eq!(saved(&destination), migrated);
}
#[test]
fn snapshot_do_scheduler_preserva_perfil_e_recusa_estados_nao_canonicos() {
    let bytes = saved(&loaded());
    let at = bytes.windows(4).rposition(|v| v == b"SCHD").unwrap();
    let payload = at + 32;
    for (offset, value) in [
        (payload, 2),
        (payload + 1, 2),
        (payload + 2, 255),
        (payload + 10, 0),
        (payload + 19, 1),
        (payload + 28, 1),
    ] {
        let mut bad = bytes.clone();
        bad[offset] = value;
        assert!(
            loaded()
                .load(&mut SnapshotReader::new(&bad).unwrap())
                .is_err(),
            "offset={offset}"
        );
    }
    let mut r = loaded();
    r.load(&mut SnapshotReader::new(&bytes).unwrap()).unwrap();
    assert_eq!(r.scheduling_profile(), SchedulingProfile::TwoThreadV1);
    assert_eq!(saved(&r), bytes);
}
