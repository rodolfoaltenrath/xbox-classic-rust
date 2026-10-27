use xbox_core::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_formats::fixtures::xbe_com_imports;
use xbox_kernel::{
    dispatch::{Dispatcher, Erro, Irql},
    imports,
    loader::{Image, STACK_BASE, STACK_SIZE},
};
use xbox_mem::{Bus, GuestPhys, GuestVirt, Perm};

fn setup() -> (Bus, Dispatcher, [u32; 8]) {
    let image = xbe_com_imports(&[0xC3], &[165, 166, 171, 180]).unwrap();
    let mut regs = [0; 8];
    regs[4] = STACK_BASE + STACK_SIZE - 0x100;
    (
        Image::prepare_aot(&image).unwrap().image.bus,
        Dispatcher::new(),
        regs,
    )
}
fn call(
    bus: &mut Bus,
    d: &mut Dispatcher,
    r: &[u32; 8],
    ordinal: u32,
    args: &[u32],
) -> Result<u32, Erro> {
    for (i, value) in std::iter::once(0x11000)
        .chain(args.iter().copied())
        .enumerate()
    {
        bus.escrever_u32(GuestVirt(r[4] + i as u32 * 4), value)
            .unwrap();
    }
    d.atender(bus, imports::trampoline(ordinal), r)
        .map(|a| a.eax)
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
        saved(d.contiguous_memory()),
    ]
}

#[test]
fn simple_rounds_owns_pages_and_query_does_not_touch_memory() {
    let (mut bus, mut d, r) = setup();
    let used = bus.frames().usados();
    let base = call(&mut bus, &mut d, &r, 165, &[4097]).unwrap();
    assert_ne!(base, 0);
    assert_eq!(base % 4096, 0);
    assert_eq!(bus.frames().usados(), used + 2);
    bus.escrever_u32(GuestVirt(base + 4094), 0xCAFEBABE)
        .unwrap();
    let before = resources(&bus, &d);
    assert_eq!(call(&mut bus, &mut d, &r, 180, &[base]).unwrap(), 8192);
    assert_eq!(resources(&bus, &d), before);
    assert_eq!(bus.ler_u32(GuestVirt(base + 4094)).unwrap(), 0xCAFEBABE);
    call(&mut bus, &mut d, &r, 171, &[base]).unwrap();
    assert_eq!(bus.frames().usados(), used);
    assert!(bus.mmu().consultar(GuestVirt(base)).is_none());
    assert!(call(&mut bus, &mut d, &r, 180, &[base]).is_err());
}
#[test]
fn zero_overflow_and_exhaustion_do_not_allocate() {
    let (mut bus, mut d, r) = setup();
    // A fronteira ABI acessa a pilha e o trampolim, independentemente do serviço.
    // Fotografar após esses acessos, sem executar o alocador.
    bus.escrever_u32(GuestVirt(r[4]), 0x11000).unwrap();
    bus.escrever_u32(GuestVirt(r[4] + 4), 0).unwrap();
    imports::Call::read(&bus, imports::trampoline(165), &r).unwrap();
    let before = resources(&bus, &d);
    for size in [0, u32::MAX, 128 * 1024 * 1024] {
        assert_eq!(call(&mut bus, &mut d, &r, 165, &[size]).unwrap(), 0);
        assert_eq!(resources(&bus, &d), before);
    }
}
#[test]
fn query_accepts_ex_allocations_and_rejects_interior_unowned_and_corruption() {
    let (mut bus, mut d, r) = setup();
    let base = call(
        &mut bus,
        &mut d,
        &r,
        166,
        &[8192, 0x20000, 0x21FFF, 4096, 0x404],
    )
    .unwrap();
    assert_eq!(call(&mut bus, &mut d, &r, 180, &[base]).unwrap(), 8192);
    let before = resources(&bus, &d);
    for address in [0, 0x11000, 0x80000000, base + 1, base + 4096, u32::MAX] {
        assert!(matches!(
            call(&mut bus, &mut d, &r, 180, &[address]),
            Err(Erro::VarianteNaoImplementada { .. })
        ));
        assert_eq!(resources(&bus, &d), before);
    }
    bus.mapear(GuestVirt(base + 4096), GuestPhys(0x21000), 1, Perm::RO)
        .unwrap();
    let corrupt = resources(&bus, &d);
    assert!(matches!(
        call(&mut bus, &mut d, &r, 180, &[base]),
        Err(Erro::MemoriaContigua(_))
    ));
    assert_eq!(resources(&bus, &d), corrupt);
}
#[test]
fn irql_refusal_and_snapshot_preserve_the_real_resource() {
    let (mut bus, mut d, r) = setup();
    let base = call(&mut bus, &mut d, &r, 165, &[1]).unwrap();
    let bytes = saved(&d);
    let before = resources(&bus, &d);
    for irql in [Irql::Apc, Irql::Dispatch] {
        let mut other = Dispatcher::with_irql(irql);
        for ordinal in [165, 180] {
            assert!(matches!(
                call(&mut bus, &mut other, &r, ordinal, &[base]),
                Err(Erro::VarianteNaoImplementada { .. })
            ));
            assert_eq!(resources(&bus, &d), before);
        }
    }
    let mut restored = Dispatcher::new();
    restored
        .load(&mut SnapshotReader::new(&bytes).unwrap())
        .unwrap();
    restored.restore_memory_context(None, &bus).unwrap();
    assert_eq!(saved(&restored), bytes);
    assert_eq!(
        call(&mut bus, &mut restored, &r, 180, &[base]).unwrap(),
        4096
    );
    call(&mut bus, &mut restored, &r, 171, &[base]).unwrap();
    assert!(restored.contiguous_memory().allocations().is_empty());
}
