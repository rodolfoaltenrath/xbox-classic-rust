use xbox_core::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_formats::{Xbe, fixtures::xbe_com_imports};
use xbox_kernel::{
    dispatch::{Dispatcher, Erro, STATUS_ACCESS_VIOLATION, STATUS_INVALID_PARAMETER},
    imports,
    loader::{Image, STACK_BASE, STACK_SIZE},
};
use xbox_mem::{Bus, GuestPhys, GuestVirt, Perm};

fn setup() -> (Bus, Dispatcher, [u32; 8]) {
    let bytes = xbe_com_imports(&[0xC3], &[184, 199, 217, 255]).unwrap();
    let xbe = Xbe::parse(&bytes).unwrap();
    let d = Dispatcher::for_image(xbe.header.base_address, xbe.header.size_of_image).unwrap();
    let mut r = [0; 8];
    r[4] = STACK_BASE + STACK_SIZE - 0x100;
    (Image::prepare_aot(&bytes).unwrap().image.bus, d, r)
}
fn call(
    bus: &mut Bus,
    d: &mut Dispatcher,
    r: &[u32; 8],
    ordinal: u32,
    args: &[u32],
) -> Result<u32, Erro> {
    for (i, v) in std::iter::once(0x11000)
        .chain(args.iter().copied())
        .enumerate()
    {
        bus.escrever_u32(GuestVirt(r[4] + i as u32 * 4), v).unwrap();
    }
    d.atender(bus, imports::trampoline(ordinal), r)
        .map(|a| a.eax)
}
fn allocate(bus: &mut Bus, d: &mut Dispatcher, r: &[u32; 8], size: u32) -> u32 {
    bus.escrever_u32(GuestVirt(0x12100), 0).unwrap();
    bus.escrever_u32(GuestVirt(0x12104), size).unwrap();
    assert_eq!(
        call(bus, d, r, 184, &[0x12100, 0, 0x12104, 0x3000, 4]).unwrap(),
        0
    );
    bus.ler_u32(GuestVirt(0x12100)).unwrap()
}
fn release(bus: &mut Bus, d: &mut Dispatcher, r: &[u32; 8], base: u32) -> Result<u32, Erro> {
    bus.escrever_u32(GuestVirt(0x12100), base).unwrap();
    bus.escrever_u32(GuestVirt(0x12104), 0).unwrap();
    call(bus, d, r, 199, &[0x12100, 0x12104, 0x8000])
}
fn saved(s: &impl Snapshot) -> Vec<u8> {
    let mut w = SnapshotWriter::new();
    s.save(&mut w);
    w.finish()
}
fn resources(bus: &Bus, d: &Dispatcher) -> [Vec<u8>; 3] {
    [
        saved(bus.frames()),
        saved(bus.mmu()),
        saved(d.virtual_memory()),
    ]
}

#[test]
fn query_interior_reports_page_remaining_bytes_and_exact_xbox_layout() {
    let (mut bus, mut d, r) = setup();
    let base = allocate(&mut bus, &mut d, &r, 0x2001);
    bus.escrever_bytes(GuestVirt(0x12200), &[0xAA; 32]).unwrap();
    assert_eq!(
        call(&mut bus, &mut d, &r, 217, &[base + 4097, 0x12200]).unwrap(),
        0
    );
    let expected = [base + 4096, base, 4, 8192, 0x1000, 4, 0x20000];
    for (i, v) in expected.into_iter().enumerate() {
        assert_eq!(bus.ler_u32(GuestVirt(0x12200 + i as u32 * 4)).unwrap(), v);
    }
    assert_eq!(bus.ler_u32(GuestVirt(0x1221C)).unwrap(), 0xAAAAAAAA);
}
#[test]
fn release_returns_frames_preserves_neighbors_and_reuse_zeroes_ram() {
    let (mut bus, mut d, r) = setup();
    let used = bus.frames().usados();
    let base = allocate(&mut bus, &mut d, &r, 4097);
    let other = allocate(&mut bus, &mut d, &r, 1);
    bus.escrever_u32(GuestVirt(base), 0xCAFEBABE).unwrap();
    bus.escrever_u32(GuestVirt(other), 42).unwrap();
    assert_eq!(release(&mut bus, &mut d, &r, base).unwrap(), 0);
    assert_eq!(bus.ler_u32(GuestVirt(0x12100)).unwrap(), base);
    assert_eq!(bus.ler_u32(GuestVirt(0x12104)).unwrap(), 8192);
    assert_eq!(bus.frames().usados(), used + 1);
    assert!(bus.mmu().consultar(GuestVirt(base)).is_none());
    assert_eq!(bus.ler_u32(GuestVirt(other)).unwrap(), 42);
    assert_eq!(allocate(&mut bus, &mut d, &r, 4097), base);
    assert_eq!(bus.ler_u32(GuestVirt(base)).unwrap(), 0);
}
#[test]
fn unmapped_readonly_mmio_and_overflow_outputs_leave_resources_unchanged() {
    let (mut bus, mut d, r) = setup();
    let base = allocate(&mut bus, &mut d, &r, 4096);
    bus.mapear(GuestVirt(0x50000000), GuestPhys(0xFD000000), 1, Perm::RW)
        .unwrap();
    // Prime the ABI trampolines before comparing their A/D bits.
    for ordinal in [199, 217] {
        imports::Call::read(&bus, imports::trampoline(ordinal), &r).unwrap();
    }
    let before = resources(&bus, &d);
    for out in [0, 0x11000, 0x12FFA, 0x50000000, u32::MAX - 3] {
        assert_eq!(
            call(&mut bus, &mut d, &r, 217, &[base, out]).unwrap(),
            STATUS_ACCESS_VIOLATION
        );
        assert_eq!(resources(&bus, &d), before);
        let free_out = if out == 0x12FFA { 0x12FFE } else { out };
        assert_eq!(
            call(&mut bus, &mut d, &r, 199, &[free_out, 0x12104, 0x8000]).unwrap(),
            STATUS_ACCESS_VIOLATION
        );
        assert_eq!(resources(&bus, &d), before);
    }
}
#[test]
fn release_rejects_output_aliases_self_outputs_and_unsupported_variants() {
    let (mut bus, mut d, r) = setup();
    let base = allocate(&mut bus, &mut d, &r, 8192);
    let pa = bus.mmu().consultar(GuestVirt(0x12000)).unwrap().0;
    bus.mapear(GuestVirt(0x40000000), pa, 1, Perm::RW).unwrap();
    for size_out in [0x12100, 0x12102, 0x40000100] {
        assert_eq!(
            call(&mut bus, &mut d, &r, 199, &[0x12100, size_out, 0x8000]).unwrap(),
            STATUS_INVALID_PARAMETER
        );
    }
    for flags in [0, 0x4000, 0xC000] {
        assert!(matches!(
            call(&mut bus, &mut d, &r, 199, &[0x12100, 0x12104, flags]),
            Err(Erro::VarianteNaoImplementada { .. })
        ));
    }
    bus.escrever_u32(GuestVirt(base), base).unwrap();
    bus.escrever_u32(GuestVirt(base + 4), 0).unwrap();
    assert!(call(&mut bus, &mut d, &r, 199, &[base, base + 4, 0x8000]).is_err());
    assert_eq!(d.virtual_memory().allocations().len(), 1);
    assert!(release(&mut bus, &mut d, &r, base + 4096).is_err());
    assert_eq!(release(&mut bus, &mut d, &r, base).unwrap(), 0);
    assert!(release(&mut bus, &mut d, &r, base).is_err());
}
#[test]
fn child_stack_tls_and_initial_image_cannot_be_released() {
    let (mut bus, mut d, r) = setup();
    let args = [0x12110, 0, 65536, 20, 0, 0x11000, 0, 0, 0, 0x11000];
    assert_eq!(call(&mut bus, &mut d, &r, 255, &args).unwrap(), 0);
    let child = d.child_thread().unwrap();
    for base in [0x10000, STACK_BASE, child.region.base] {
        assert!(release(&mut bus, &mut d, &r, base).is_err());
    }
    let base = allocate(&mut bus, &mut d, &r, 4096);
    let mut alternate = r;
    alternate[4] = base + 512;
    assert!(release(&mut bus, &mut d, &alternate, base).is_err());
    d.restore_memory_context(None, &bus).unwrap();
    assert!(bus.mmu().consultar(GuestVirt(child.tib())).is_some());
}
#[test]
fn snapshot_replay_query_and_release_and_corrupt_ownership_refusal() {
    let (mut bus, mut d, r) = setup();
    let base = allocate(&mut bus, &mut d, &r, 8192);
    let state = saved(&d);
    let mut other = Dispatcher::new();
    other
        .load(&mut SnapshotReader::new(&state).unwrap())
        .unwrap();
    other.restore_memory_context(None, &bus).unwrap();
    assert_eq!(
        call(&mut bus, &mut other, &r, 217, &[base, 0x12200]).unwrap(),
        0
    );
    let pa = bus.mmu().consultar(GuestVirt(base + 4096)).unwrap().0;
    bus.frames_mut().liberar(pa, 1);
    assert!(matches!(
        release(&mut bus, &mut other, &r, base),
        Err(Erro::MemoriaVirtual(_))
    ));
    assert!(bus.mmu().consultar(GuestVirt(base)).is_some());
    assert_eq!(other.virtual_memory().allocations().len(), 1);
    bus.frames_mut().reservar(pa, 1).unwrap();
    assert_eq!(release(&mut bus, &mut other, &r, base).unwrap(), 0);
    other.restore_memory_context(None, &bus).unwrap();
}
