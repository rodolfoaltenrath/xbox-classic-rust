use xbox_core::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_formats::fixtures::xbe_com_imports;
use xbox_kernel::{
    dispatch::{Dispatcher, Erro, Irql},
    imports,
    loader::{Image, STACK_BASE, STACK_SIZE},
};
use xbox_mem::{Acesso, Bus, GuestPhys, GuestVirt, Perm};

fn setup() -> (Bus, Dispatcher, [u32; 8]) {
    let image = xbe_com_imports(&[0xC3], &[166, 168, 171, 3, 2]).unwrap();
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
    regs: &[u32; 8],
    ordinal: u32,
    args: &[u32],
) -> Result<u32, Erro> {
    for (i, value) in std::iter::once(0x11000)
        .chain(args.iter().copied())
        .enumerate()
    {
        bus.escrever_u32(GuestVirt(regs[4] + i as u32 * 4), value)
            .unwrap();
    }
    d.atender(bus, imports::trampoline(ordinal), regs)
        .map(|r| r.eax)
}
fn allocate(bus: &mut Bus, d: &mut Dispatcher, regs: &[u32; 8], base: u32, size: u32) -> u32 {
    call(bus, d, regs, 166, &[size, base, base + size - 1, 4096, 4]).unwrap()
}
fn saved(value: &impl Snapshot) -> Vec<u8> {
    let mut out = SnapshotWriter::new();
    value.save(&mut out);
    out.finish()
}
fn resources(bus: &Bus, d: &Dispatcher) -> [Vec<u8>; 3] {
    [
        saved(bus.frames()),
        saved(bus.mmu()),
        saved(d.contiguous_memory()),
    ]
}
#[test]
fn libera_todas_as_paginas_preserva_vizinhos_e_reuso_zera_bytes() {
    let (mut bus, mut d, r) = setup();
    let used = bus.frames().usados();
    let pages = bus.mmu().paginas_mapeadas();
    for (base, size) in [(0x20000, 4096), (0x21000, 8192), (0x23000, 4096)] {
        assert_eq!(
            allocate(&mut bus, &mut d, &r, base, size),
            0x80000000 + base
        );
        bus.escrever_u32(GuestVirt(0x80000000 + base), 0xCAFEBABE)
            .unwrap();
    }
    assert_eq!(call(&mut bus, &mut d, &r, 171, &[0x80021000]).unwrap(), 0);
    assert_eq!(bus.frames().usados(), used + 2);
    assert_eq!(bus.mmu().paginas_mapeadas(), pages + 2);
    for base in [0x21000, 0x22000] {
        assert!(!bus.frames().em_uso(base / 4096));
        assert!(bus.mmu().consultar(GuestVirt(0x80000000 + base)).is_none());
        assert!(
            bus.traduzir(GuestVirt(0x80000000 + base), Acesso::Leitura)
                .is_err()
        );
    }
    assert_eq!(
        bus.dma(GuestPhys(0x21000), 4).unwrap(),
        &0xCAFEBABEu32.to_le_bytes()
    );
    for base in [0x20000, 0x23000] {
        assert_eq!(
            bus.ler_u32(GuestVirt(0x80000000 + base)).unwrap(),
            0xCAFEBABE
        );
    }
    d.restore_memory_context(None, &bus).unwrap();
    assert_eq!(allocate(&mut bus, &mut d, &r, 0x21000, 8192), 0x80021000);
    assert!(
        bus.dma(GuestPhys(0x21000), 8192)
            .unwrap()
            .iter()
            .all(|&b| b == 0)
    );
    assert_eq!(bus.frames().usados(), used + 4);
}
#[test]
fn base_interior_null_desconhecida_e_double_free_recusam_sem_liberar() {
    let (mut bus, mut d, r) = setup();
    allocate(&mut bus, &mut d, &r, 0x20000, 8192);
    let before = resources(&bus, &d);
    for base in [0, 0x20000, 0x80020001, 0x80021000, 0x80030000, 0x80000000] {
        assert!(matches!(
            call(&mut bus, &mut d, &r, 171, &[base]),
            Err(Erro::ArgumentoInvalido { .. })
        ));
        assert_eq!(resources(&bus, &d), before);
    }
    call(&mut bus, &mut d, &r, 171, &[0x80020000]).unwrap();
    let empty = resources(&bus, &d);
    assert!(matches!(
        call(&mut bus, &mut d, &r, 171, &[0x80020000]),
        Err(Erro::ArgumentoInvalido { .. })
    ));
    assert_eq!(resources(&bus, &d), empty);
    assert!(bus.boot_dma_page().reserved());
    bus.boot_dma_page().validate(&bus).unwrap();
}
#[test]
fn irql_e_posse_corrompida_recusam_antes_de_mutar_os_recursos() {
    let (mut bus, mut d, r) = setup();
    allocate(&mut bus, &mut d, &r, 0x20000, 8192);
    let before = resources(&bus, &d);
    for irql in [Irql::Apc, Irql::Dispatch] {
        assert!(matches!(
            call(
                &mut bus,
                &mut Dispatcher::with_irql(irql),
                &r,
                171,
                &[0x80020000]
            ),
            Err(Erro::VarianteNaoImplementada { .. })
        ));
        assert_eq!(resources(&bus, &d), before);
    }
    for corruption in 0..3 {
        match corruption {
            0 => bus.frames_mut().liberar(GuestPhys(0x21000), 1),
            1 => bus
                .mapear(GuestVirt(0x80021000), GuestPhys(0x21000), 1, Perm::RO)
                .unwrap(),
            _ => bus
                .mapear(GuestVirt(0x50000000), GuestPhys(0x20000), 1, Perm::RW)
                .unwrap(),
        }
        let corrupt = resources(&bus, &d);
        assert!(matches!(
            call(&mut bus, &mut d, &r, 171, &[0x80020000]),
            Err(Erro::MemoriaContigua(_))
        ));
        assert_eq!(resources(&bus, &d), corrupt);
        match corruption {
            0 => bus.frames_mut().reservar(GuestPhys(0x21000), 1).unwrap(),
            1 => bus
                .mapear(GuestVirt(0x80021000), GuestPhys(0x21000), 1, Perm::RW)
                .unwrap(),
            _ => bus.mmu_mut().desmapear(GuestVirt(0x50000000), 1),
        }
    }
    call(&mut bus, &mut d, &r, 171, &[0x80020000]).unwrap();
}
#[test]
fn reserva_de_instancia_gpu_permanece_possuida_e_consulta_repetida_preserva_bytes() {
    let (mut bus, mut d, r) = setup();
    call(&mut bus, &mut d, &r, 168, &[u32::MAX, 0x12100]).unwrap();
    bus.escrever_u32(GuestVirt(0x83FE1000), 0xCAFEBABE).unwrap();
    let before = resources(&bus, &d);
    assert!(matches!(
        call(&mut bus, &mut d, &r, 171, &[0x83FE0000]),
        Err(Erro::VarianteNaoImplementada { .. })
    ));
    assert_eq!(resources(&bus, &d), before);
    d.restore_memory_context(None, &bus).unwrap();
    assert_eq!(
        call(&mut bus, &mut d, &r, 168, &[u32::MAX, 0x12100]).unwrap(),
        0x83FF0000
    );
    assert_eq!(bus.ler_u32(GuestVirt(0x83FE1000)).unwrap(), 0xCAFEBABE);
}
#[test]
fn framebuffer_com_offset_permanece_valido_ate_troca_de_modo_mesmo_em_blanking() {
    let (mut bus, mut d, r) = setup();
    for base in [0x20000, 0x200000] {
        allocate(&mut bus, &mut d, &r, base, 0x180000);
    }
    for (framebuffer, freed) in [(0x20010, None), (0x200010, Some(0x80020000))] {
        call(
            &mut bus,
            &mut d,
            &r,
            3,
            &[0xFD000000, 0, 0x04010101, 0x12, 2560, framebuffer],
        )
        .unwrap();
        for enabled in [1, 0] {
            call(&mut bus, &mut d, &r, 2, &[0xFD000000, 9, enabled, 0]).unwrap();
            let before = resources(&bus, &d);
            assert!(matches!(
                call(
                    &mut bus,
                    &mut d,
                    &r,
                    171,
                    &[0x80000000 + (framebuffer & !0xFFF)]
                ),
                Err(Erro::VarianteNaoImplementada { .. })
            ));
            assert_eq!(resources(&bus, &d), before);
            assert!(d.framebuffer(&bus).unwrap().is_some());
            d.restore_memory_context(None, &bus).unwrap();
        }
        if let Some(base) = freed {
            call(&mut bus, &mut d, &r, 171, &[base]).unwrap();
        }
    }
}
#[test]
fn snapshot_depois_da_liberacao_restaura_ausencia_de_mapa_e_reuso_deterministico() {
    let (mut bus, mut d, r) = setup();
    allocate(&mut bus, &mut d, &r, 0x20000, 8192);
    bus.escrever_u32(GuestVirt(0x80020000), 0xCAFEBABE).unwrap();
    call(&mut bus, &mut d, &r, 171, &[0x80020000]).unwrap();
    let bus_bytes = saved(&bus);
    let kernel_bytes = saved(&d);
    let (mut restored_bus, mut restored_kernel, _) = setup();
    restored_bus
        .load(&mut SnapshotReader::new(&bus_bytes).unwrap())
        .unwrap();
    restored_kernel
        .load(&mut SnapshotReader::new(&kernel_bytes).unwrap())
        .unwrap();
    restored_kernel
        .restore_memory_context(None, &restored_bus)
        .unwrap();
    assert_eq!(saved(&restored_bus), bus_bytes);
    assert_eq!(saved(&restored_kernel), kernel_bytes);
    for (b, k) in [
        (&mut bus, &mut d),
        (&mut restored_bus, &mut restored_kernel),
    ] {
        assert!(b.mmu().consultar(GuestVirt(0x80020000)).is_none());
        assert!(!b.frames().em_uso(0x20));
        assert_eq!(allocate(b, k, &r, 0x20000, 8192), 0x80020000);
    }
    assert_eq!(saved(&restored_bus), saved(&bus));
    assert_eq!(saved(&restored_kernel), saved(&d));
}

#[test]
fn scanout_pcrtc_seleciona_buffer_real_e_protege_backing_ativo_no_snapshot() {
    let (mut bus, mut d, r) = setup();
    for base in [0x20000, 0x200000] {
        allocate(&mut bus, &mut d, &r, base, 0x180000);
    }
    call(
        &mut bus,
        &mut d,
        &r,
        3,
        &[0xFD000000, 0, 0x04010101, 0x12, 2560, 0x20000],
    )
    .unwrap();
    assert_eq!(bus.nv2a_pgraph().scanout(), Some(0x20000));
    bus.escrever_u32(GuestVirt(0x80200010), 0xFF123456).unwrap();
    bus.escrever_u32(GuestVirt(0xFD600800), 0x200010).unwrap();
    let view = d.framebuffer(&bus).unwrap().unwrap();
    assert_eq!(view.mode().framebuffer(), GuestPhys(0x200010));
    assert_eq!(&view.row(0).unwrap()[..4], &0xFF123456u32.to_le_bytes());
    let before = resources(&bus, &d);
    for base in [0x80020000, 0x80200000] {
        assert!(matches!(
            call(&mut bus, &mut d, &r, 171, &[base]),
            Err(Erro::VarianteNaoImplementada { .. })
        ));
        assert_eq!(resources(&bus, &d), before);
    }
    let bus_bytes = saved(&bus);
    let kernel_bytes = saved(&d);
    let (mut restored, mut kd, _) = setup();
    restored
        .load(&mut SnapshotReader::new(&bus_bytes).unwrap())
        .unwrap();
    kd.load(&mut SnapshotReader::new(&kernel_bytes).unwrap())
        .unwrap();
    kd.restore_memory_context(None, &restored).unwrap();
    assert_eq!(
        kd.framebuffer(&restored).unwrap().unwrap().copiar_visivel(),
        d.framebuffer(&bus).unwrap().unwrap().copiar_visivel()
    );
    restored
        .escrever_u32(GuestVirt(0xFD600800), 0x3FFFFFC)
        .unwrap();
    assert!(kd.framebuffer(&restored).is_err());
    assert!(kd.restore_memory_context(None, &restored).is_err());
}
