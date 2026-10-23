use xbox_core::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_formats::fixtures::xbe_com_codigo;
use xbox_recomp::{
    analysis,
    ir::Operation,
    runtime::{INTERRUPT_FLAG, InterruptProfile, State, TickPublication},
};
fn saved(s: &State) -> Vec<u8> {
    let mut w = SnapshotWriter::new();
    s.save(&mut w);
    w.finish()
}
#[test]
fn sti_decodifica_e_prefixos_invalidos_continuam_recusados() {
    let p = analysis::analyze(&xbe_com_codigo(&[0xFB, 0xC3]).unwrap()).unwrap();
    assert_eq!(p.instructions[&0x11000].operation, Operation::SetInterrupts);
    for prefix in [0xF0, 0xF2, 0xF3] {
        assert!(analysis::analyze(&xbe_com_codigo(&[prefix, 0xFB, 0xC3]).unwrap()).is_err());
    }
}
#[test]
fn aotc_v9_migra_sem_ativar_if_ou_preempcao_ao_restaurar_e_regravar() {
    let current = saved(&State::new(0x11000));
    let mut old = current[..current.len() - 2].to_vec();
    old[16..20].copy_from_slice(&9u32.to_le_bytes());
    let mut s = State::new(0);
    s.load(&mut SnapshotReader::new(&old).unwrap()).unwrap();
    assert_eq!(s.interrupt_profile, InterruptProfile::LegacyBoundary);
    assert_eq!(s.eflags & INTERRUPT_FLAG, 0);
    assert_eq!(s.sti_shadow, 0);
    assert_eq!(s.tick_publication, TickPublication::ClockLinkedV1);
    let migrated = saved(&s);
    let mut r = State::new(0);
    r.load(&mut SnapshotReader::new(&migrated).unwrap())
        .unwrap();
    assert_eq!(r, s);
    assert_eq!(saved(&r), migrated);
}
#[test]
fn shadow_de_sti_persiste_e_recusa_estado_impossivel() {
    let mut s = State::new(0x11000);
    s.set_interrupts();
    s.finish_interrupt_step();
    assert!(!s.maskable_interrupts_enabled());
    let bytes = saved(&s);
    let mut r = State::new(0);
    r.load(&mut SnapshotReader::new(&bytes).unwrap()).unwrap();
    assert_eq!(r, s);
    r.finish_interrupt_step();
    assert!(r.maskable_interrupts_enabled());
    r.finish_interrupt_step();
    assert!(r.maskable_interrupts_enabled());
    for (profile, shadow, if_enabled) in [(2, 0, true), (1, 2, true), (0, 1, true), (1, 1, false)] {
        let mut bad = bytes.clone();
        let end = bad.len();
        bad[end - 2] = profile;
        bad[end - 1] = shadow;
        if !if_enabled {
            let flags = 20 + 8 * 4 + 4;
            bad[flags..flags + 4].copy_from_slice(&2u32.to_le_bytes());
        }
        assert!(r.load(&mut SnapshotReader::new(&bad).unwrap()).is_err());
    }
}
