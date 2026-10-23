use xbox_core::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_recomp::runtime::{State, TickPublication};
fn saved(s: &State) -> Vec<u8> {
    let mut w = SnapshotWriter::new();
    s.save(&mut w);
    w.finish()
}
#[test]
fn aotc_v8_migra_tick_em_fronteiras_e_nao_promove_ao_regravar() {
    let current = saved(&State::new(0x11000));
    let mut old = current[..current.len() - 3].to_vec();
    old[16..20].copy_from_slice(&8u32.to_le_bytes());
    let mut s = State::new(0);
    s.load(&mut SnapshotReader::new(&old).unwrap()).unwrap();
    assert_eq!(s.tick_publication, TickPublication::BoundariesOnly);
    let migrated = saved(&s);
    let mut r = State::new(0);
    r.load(&mut SnapshotReader::new(&migrated).unwrap())
        .unwrap();
    assert_eq!(r, s);
    assert_eq!(saved(&r), migrated);
}
#[test]
fn aotc_v9_preserva_tick_e_recusa_perfil_desconhecido() {
    let s = State::new(0x11000);
    let bytes = saved(&s);
    let mut r = State::new(0);
    r.load(&mut SnapshotReader::new(&bytes).unwrap()).unwrap();
    assert_eq!(r, s);
    for id in [2, 255] {
        let mut bad = bytes.clone();
        let tick = bad.len() - 3;
        bad[tick] = id;
        assert!(r.load(&mut SnapshotReader::new(&bad).unwrap()).is_err());
    }
}
