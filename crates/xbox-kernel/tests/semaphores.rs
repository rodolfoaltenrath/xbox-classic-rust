use xbox_core::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_formats::fixtures::xbe_com_imports;
use xbox_kernel::{
    dispatch::{Dispatcher, Erro, Irql},
    imports,
    loader::{Image, STACK_BASE, STACK_SIZE},
};
use xbox_mem::{Bus, GuestVirt};
fn setup() -> (Bus, Dispatcher, [u32; 8]) {
    let image = xbe_com_imports(&[0xC3], &[193, 222, 233, 187, 189]).unwrap();
    let mut r = [0; 8];
    r[4] = STACK_BASE + STACK_SIZE - 256;
    (
        Image::prepare_aot(&image).unwrap().image.bus,
        Dispatcher::new(),
        r,
    )
}
fn call(
    b: &mut Bus,
    d: &mut Dispatcher,
    r: &[u32; 8],
    ordinal: u32,
    args: &[u32],
) -> Result<u32, Erro> {
    for (i, v) in std::iter::once(0x11000)
        .chain(args.iter().copied())
        .enumerate()
    {
        b.escrever_u32(GuestVirt(r[4] + i as u32 * 4), v).unwrap();
    }
    d.atender(b, imports::trampoline(ordinal), r).map(|a| a.eax)
}
fn saved(s: &impl Snapshot) -> Vec<u8> {
    let mut w = SnapshotWriter::new();
    s.save(&mut w);
    w.finish()
}
fn create(b: &mut Bus, d: &mut Dispatcher, r: &[u32; 8], initial: u32, max: u32) -> u32 {
    assert_eq!(call(b, d, r, 193, &[0x12100, 0, initial, max]).unwrap(), 0);
    b.ler_u32(GuestVirt(0x12100)).unwrap()
}
#[test]
fn counts_wait_release_previous_and_close_use_real_guest_handles() {
    let (mut b, mut d, r) = setup();
    let h = create(&mut b, &mut d, &r, 2, 3);
    assert_eq!(d.semaphore(h), Some((2, 3)));
    for count in [1, 0] {
        assert_eq!(call(&mut b, &mut d, &r, 233, &[h, 0, 0]).unwrap(), 0);
        assert_eq!(d.semaphore(h), Some((count, 3)));
    }
    assert!(call(&mut b, &mut d, &r, 233, &[h, 0, 0]).is_err());
    assert_eq!(d.semaphore(h), Some((0, 3)));
    assert_eq!(call(&mut b, &mut d, &r, 222, &[h, 3, 0x12104]).unwrap(), 0);
    assert_eq!(b.ler_u32(GuestVirt(0x12104)).unwrap(), 0);
    assert_eq!(d.semaphore(h), Some((3, 3)));
    assert_eq!(
        call(&mut b, &mut d, &r, 222, &[h, 1, 0x12104]).unwrap(),
        0xC0000047
    );
    assert_eq!(d.semaphore(h), Some((3, 3)));
    assert_eq!(call(&mut b, &mut d, &r, 187, &[h]).unwrap(), 0);
    assert_eq!(d.semaphore(h), None);
    assert_eq!(
        call(&mut b, &mut d, &r, 222, &[h, 1, 0]).unwrap(),
        0xC0000008
    );
    let next = create(&mut b, &mut d, &r, 0, 1);
    assert!(next > h);
}
#[test]
fn invalid_counts_attributes_and_outputs_do_not_consume_a_handle() {
    let (mut b, mut d, r) = setup();
    for (initial, max) in [(0, 0), (2, 1), (u32::MAX, 1), (0, u32::MAX)] {
        assert_eq!(
            call(&mut b, &mut d, &r, 193, &[0x12100, 0, initial, max]).unwrap(),
            0xC000000D
        );
    }
    for ptr in [0, 0x11000, 0x12FFE, u32::MAX - 1] {
        assert_eq!(
            call(&mut b, &mut d, &r, 193, &[ptr, 0, 0, 1]).unwrap(),
            0xC0000005
        );
    }
    b.escrever_u32(GuestVirt(0x12110), 1).unwrap();
    assert!(matches!(
        call(&mut b, &mut d, &r, 193, &[0x12100, 0x12110, 0, 1]),
        Err(Erro::VarianteNaoImplementada { .. })
    ));
    assert_eq!(create(&mut b, &mut d, &r, 0, 1), 4);
}
#[test]
fn release_bad_pointer_negative_zero_and_excess_preserve_count_and_output() {
    let (mut b, mut d, r) = setup();
    let h = create(&mut b, &mut d, &r, 1, 2);
    b.escrever_u32(GuestVirt(0x12104), 0xCAFEBABE).unwrap();
    for (count, output, expected) in [
        (1, 0x12FFE, 0xC0000005),
        (0, 0x12104, 0xC000000D),
        (u32::MAX, 0x12104, 0xC000000D),
        (2, 0x12104, 0xC0000047),
    ] {
        assert_eq!(
            call(&mut b, &mut d, &r, 222, &[h, count, output]).unwrap(),
            expected
        );
        assert_eq!(d.semaphore(h), Some((1, 2)));
        assert_eq!(b.ler_u32(GuestVirt(0x12104)).unwrap(), 0xCAFEBABE);
    }
    assert_eq!(call(&mut b, &mut d, &r, 222, &[h, 1, 0x12104]).unwrap(), 0);
    assert_eq!(b.ler_u32(GuestVirt(0x12104)).unwrap(), 1);
}
#[test]
fn irql_and_snapshot_migration_preserve_counts_and_detect_colliding_handles() {
    let (mut b, mut d, r) = setup();
    let h = create(&mut b, &mut d, &r, 1, 3);
    let bytes = saved(&d);
    let mut restored = Dispatcher::new();
    restored
        .load(&mut SnapshotReader::new(&bytes).unwrap())
        .unwrap();
    assert_eq!(saved(&restored), bytes);
    call(&mut b, &mut restored, &r, 233, &[h, 0, 0]).unwrap();
    assert_eq!(restored.semaphore(h), Some((0, 3)));
    let at = bytes.windows(4).rposition(|v| v == b"SEMA").unwrap();
    let mut old = bytes[..at].to_vec();
    old[16..20].copy_from_slice(&15u32.to_le_bytes());
    restored
        .load(&mut SnapshotReader::new(&old).unwrap())
        .unwrap();
    assert_eq!(restored.semaphore(h), None);
    let again = saved(&restored);
    restored
        .load(&mut SnapshotReader::new(&again).unwrap())
        .unwrap();
    assert_eq!(restored.semaphore(h), None);
    for (offset, value) in [
        (at + 4, 2u32),
        (at + 8, 4097),
        (at + 12, 0),
        (at + 16, 4),
        (at + 20, 0),
    ] {
        let mut bad = bytes.clone();
        bad[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        assert!(
            Dispatcher::new()
                .load(&mut SnapshotReader::new(&bad).unwrap())
                .is_err()
        );
    }
    // Colisão com um evento vivo, sem usar um handle inexistente como atalho.
    assert_eq!(
        call(&mut b, &mut d, &r, 189, &[0x12110, 0, 0, 0]).unwrap(),
        0
    );
    let event = b.ler_u32(GuestVirt(0x12110)).unwrap();
    let mut bad = saved(&d);
    let at = bad.windows(4).rposition(|v| v == b"SEMA").unwrap();
    bad[at + 12..at + 16].copy_from_slice(&event.to_le_bytes());
    let before = saved(&restored);
    assert!(
        restored
            .load(&mut SnapshotReader::new(&bad).unwrap())
            .is_err()
    );
    assert_eq!(
        saved(&restored),
        before,
        "colisão não pode publicar estado candidato"
    );
    let mut elevated = Dispatcher::with_irql(Irql::Dispatch);
    assert!(call(&mut b, &mut elevated, &r, 193, &[0x12100, 0, 0, 1]).is_err());
}
