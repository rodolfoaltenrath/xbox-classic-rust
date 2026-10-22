use xbox_core::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_formats::fixtures::xbe_com_imports;
use xbox_kernel::{
    dispatch::{Dispatcher, Erro, Irql},
    imports,
    loader::{Image, STACK_BASE, STACK_SIZE},
};
use xbox_mem::{Acesso, Bus, GuestPhys, GuestVirt, Perm};
fn setup() -> (Bus, Dispatcher, [u32; 8]) {
    let image = xbe_com_imports(&[0xC3], &[166, 184]).unwrap();
    let mut regs = [0; 8];
    regs[4] = STACK_BASE + STACK_SIZE - 0x100;
    (
        Image::prepare_aot(&image).unwrap().image.bus,
        Dispatcher::new(),
        regs,
    )
}
fn args() -> [u32; 5] {
    [0x2001, 0x20000, 0x3FFFFFF, 4096, 0x404]
}
fn call(bus: &mut Bus, d: &mut Dispatcher, regs: &[u32; 8], a: [u32; 5]) -> Result<u32, Erro> {
    bus.escrever_u32(GuestVirt(regs[4]), 0x11000).unwrap();
    for (i, arg) in a.into_iter().enumerate() {
        bus.escrever_u32(GuestVirt(regs[4] + 4 + i as u32 * 4), arg)
            .unwrap();
    }
    d.atender(bus, imports::trampoline(166), regs)
        .map(|r| r.eax)
}
fn saved(value: &impl Snapshot) -> Vec<u8> {
    let mut out = SnapshotWriter::new();
    value.save(&mut out);
    out.finish()
}
#[test]
fn frames_vizinhos_zerados_mapa_restrito_rw_e_dma_observa_store_entre_paginas() {
    let (mut bus, mut d, regs) = setup();
    bus.carregar_em_fisico(GuestPhys(0x20000), &[0xAA; 0x3000])
        .unwrap();
    let used = bus.frames().usados();
    let pages = bus.mmu().paginas_mapeadas();
    assert_eq!(call(&mut bus, &mut d, &regs, args()).unwrap(), 0x80020000);
    let a = d.contiguous_memory().allocations()[0];
    assert_eq!(a.size, 0x3000);
    assert_eq!(a.protect, 0x404);
    for offset in [0, 4096, 8192] {
        assert_eq!(
            bus.mmu().consultar(GuestVirt(0x80020000 + offset)),
            Some((GuestPhys(0x20000 + offset), Perm::RW))
        );
    }
    assert!(bus.dma(a.physical, a.size).unwrap().iter().all(|b| *b == 0));
    assert!(bus.mmu().consultar(GuestVirt(0x8001F000)).is_none());
    assert!(bus.mmu().consultar(GuestVirt(0x80023000)).is_none());
    assert!(bus.mmu().consultar(GuestVirt(0x20000)).is_none());
    assert!(bus.traduzir(a.address(), Acesso::Execucao).is_err());
    bus.escrever_u32(GuestVirt(a.address().0 + 4094), 0xCAFEBABE)
        .unwrap();
    assert_eq!(
        bus.dma(GuestPhys(a.physical.0 + 4094), 4).unwrap(),
        &0xCAFEBABEu32.to_le_bytes()
    );
    assert_eq!(bus.frames().usados(), used + 3);
    assert_eq!(bus.mmu().paginas_mapeadas(), pages + 3);
}
#[test]
fn limite_fisico_inclusivo_alinhamento_e_colisao_de_va_sao_respeitados() {
    let (mut bus, mut d, regs) = setup();
    bus.mapear(GuestVirt(0x80020000), GuestPhys(0xFD000000), 1, Perm::RW)
        .unwrap();
    assert_eq!(
        call(&mut bus, &mut d, &regs, [4096, 0x20001, 0x2FFFF, 65536, 4]).unwrap(),
        0
    );
    assert_eq!(
        call(&mut bus, &mut d, &regs, [4096, 0x20000, 0x20FFE, 4096, 4]).unwrap(),
        0
    );
    assert_eq!(
        call(&mut bus, &mut d, &regs, [4096, 0x20000, 0x21FFF, 4096, 4]).unwrap(),
        0x80021000
    );
    assert_eq!(
        call(&mut bus, &mut d, &regs, [1, 0x30001, 0x4FFFF, 65536, 4]).unwrap(),
        0x80040000
    );
    assert_eq!(
        bus.mmu().consultar(GuestVirt(0x80020000)),
        Some((GuestPhys(0xFD000000), Perm::RW))
    );
}
#[test]
fn fragmentacao_oom_e_tamanhos_invalidos_nao_publicam_frames_ou_mapa() {
    let (mut bus, mut d, regs) = setup();
    for frame in (0x20..0x30).step_by(2) {
        bus.frames_mut()
            .reservar(GuestPhys(frame * 4096), 1)
            .unwrap();
    }
    // A leitura do frame ABI marca A/D normalmente; estabilizar antes de comparar.
    call(&mut bus, &mut d, &regs, [0, 0, u32::MAX, 4096, 4]).unwrap();
    let frames = saved(bus.frames());
    let mmu = saved(bus.mmu());
    for a in [
        [8192, 0x20000, 0x2FFFF, 4096, 4],
        [0, 0, u32::MAX, 4096, 4],
        [u32::MAX, 0, u32::MAX, 4096, 4],
        [4096, 0x40000, 0x3FFFF, 4096, 4],
        [128 * 1024 * 1024, 0, u32::MAX, 4096, 4],
    ] {
        assert_eq!(call(&mut bus, &mut d, &regs, a).unwrap(), 0);
        assert_eq!(saved(bus.frames()), frames);
        assert_eq!(saved(bus.mmu()), mmu);
    }
    assert!(d.contiguous_memory().allocations().is_empty());
}
#[test]
fn variantes_de_protecao_alinhamento_e_irql_recusam_sem_alocar() {
    let (mut bus, mut d, regs) = setup();
    let used = bus.frames().usados();
    for (index, value) in [(3, 16), (3, 0x3000), (4, 0x40), (4, 0x204), (4, 2)] {
        let mut a = args();
        a[index] = value;
        assert!(matches!(
            call(&mut bus, &mut d, &regs, a),
            Err(Erro::VarianteNaoImplementada { .. })
        ));
    }
    for irql in [Irql::Apc, Irql::Dispatch] {
        assert!(call(&mut bus, &mut Dispatcher::with_irql(irql), &regs, args()).is_err());
    }
    assert_eq!(bus.frames().usados(), used);
    assert!(d.contiguous_memory().allocations().is_empty());
}
#[test]
fn estado_preexistente_inconsistente_nao_e_sobrescrito_pela_alocacao() {
    let (mut bus, mut d, regs) = setup();
    bus.mapear(GuestVirt(0x50000000), GuestPhys(0x20000), 1, Perm::RW)
        .unwrap();
    let used = bus.frames().usados();
    assert!(matches!(
        call(&mut bus, &mut d, &regs, args()),
        Err(Erro::MemoriaContigua(_))
    ));
    assert_eq!(bus.frames().usados(), used);
    assert!(d.contiguous_memory().allocations().is_empty());
}
#[test]
fn snapshot_confere_contiguidade_permissoes_bitmap_e_aliases_sem_marcar_ad() {
    let (mut bus, mut d, regs) = setup();
    call(&mut bus, &mut d, &regs, args()).unwrap();
    let checkpoint = saved(&d);
    let mut restored = Dispatcher::new();
    Snapshot::load(
        &mut restored,
        &mut SnapshotReader::new(&checkpoint).unwrap(),
    )
    .unwrap();
    let before = saved(bus.mmu());
    restored.restore_memory_context(None, &bus).unwrap();
    assert_eq!(saved(bus.mmu()), before);
    assert_eq!(saved(&restored), checkpoint);
    for perm in [Perm::RO, Perm::RWX] {
        bus.mapear(GuestVirt(0x80020000), GuestPhys(0x20000), 1, perm)
            .unwrap();
        assert!(restored.restore_memory_context(None, &bus).is_err());
    }
    bus.mapear(GuestVirt(0x80020000), GuestPhys(0x20000), 1, Perm::RW)
        .unwrap();
    bus.mapear(GuestVirt(0x80021000), GuestPhys(0x20000), 1, Perm::RW)
        .unwrap();
    assert!(restored.restore_memory_context(None, &bus).is_err());
    bus.mapear(GuestVirt(0x80021000), GuestPhys(0x21000), 1, Perm::RW)
        .unwrap();
    bus.mapear(GuestVirt(0x60000000), GuestPhys(0x20000), 1, Perm::RW)
        .unwrap();
    assert!(restored.restore_memory_context(None, &bus).is_err());
    bus.mmu_mut().desmapear(GuestVirt(0x60000000), 1);
    bus.frames_mut().liberar(GuestPhys(0x20000), 1);
    assert!(restored.restore_memory_context(None, &bus).is_err());
}
#[test]
fn snapshot_rejeita_geometria_ordenacao_contagem_e_migra_v9_sem_recursos() {
    let (mut bus, mut d, regs) = setup();
    call(&mut bus, &mut d, &regs, args()).unwrap();
    call(&mut bus, &mut d, &regs, [4096, 0x40000, 0x4FFFF, 4096, 4]).unwrap();
    let checkpoint = saved(&d);
    let tag = checkpoint.windows(4).rposition(|v| v == b"CMEM").unwrap();
    for (offset, value) in [
        (4, 0u32),
        (8, 4097),
        (12, 0x20001),
        (16, 0),
        (20, 0x3000),
        (24, 0x40),
        (28, 0x21000),
    ] {
        let mut bad = checkpoint.clone();
        bad[tag + offset..tag + offset + 4].copy_from_slice(&value.to_le_bytes());
        assert!(
            Snapshot::load(
                &mut Dispatcher::new(),
                &mut SnapshotReader::new(&bad).unwrap()
            )
            .is_err()
        );
    }
    assert!(
        Snapshot::load(
            &mut Dispatcher::new(),
            &mut SnapshotReader::new(&checkpoint[..checkpoint.len() - 1]).unwrap()
        )
        .is_err()
    );
    let mut old = checkpoint[..tag].to_vec();
    let k = old.windows(4).position(|v| v == b"KRNL").unwrap();
    old[k + 4..k + 8].copy_from_slice(&9u32.to_le_bytes());
    Snapshot::load(&mut d, &mut SnapshotReader::new(&old).unwrap()).unwrap();
    assert!(d.contiguous_memory().allocations().is_empty());
    // Payload legado não pode esconder páginas novas já presentes no Bus.
    assert!(d.restore_memory_context(None, &bus).is_err());
}
#[test]
fn recursos_contiguos_e_privados_nao_compartilham_frames() {
    let bytes = xbe_com_imports(&[0xC3], &[166, 184]).unwrap();
    let xbe = xbox_formats::Xbe::parse(&bytes).unwrap();
    let (mut bus, _, regs) = setup();
    let mut d = Dispatcher::for_image(xbe.header.base_address, xbe.header.size_of_image).unwrap();
    call(&mut bus, &mut d, &regs, args()).unwrap();
    bus.escrever_u32(GuestVirt(0x12100), 0).unwrap();
    bus.escrever_u32(GuestVirt(0x12104), 4096).unwrap();
    for (i, arg) in [0x12100, 0, 0x12104, 0x3000, 4].into_iter().enumerate() {
        bus.escrever_u32(GuestVirt(regs[4] + 4 + i as u32 * 4), arg)
            .unwrap();
    }
    assert_eq!(
        d.atender(&mut bus, imports::trampoline(184), &regs)
            .unwrap()
            .eax,
        0
    );
    let base = bus.ler_u32(GuestVirt(0x12100)).unwrap();
    let pa = bus.traduzir(GuestVirt(base), Acesso::Leitura).unwrap();
    assert!(!(0x20000..0x23000).contains(&pa.0));
    d.restore_memory_context(d.virtual_memory().image(), &bus)
        .unwrap();
}
#[test]
fn alinhamento_zero_vira_pagina_e_aloca_igual_ao_de_4096() {
    let (mut bus, mut d, regs) = setup();
    let mut a = args();
    a[3] = 0;
    let com_zero = call(&mut bus, &mut d, &regs, a).unwrap();
    assert_ne!(com_zero, 0, "alinhamento 0 deveria alocar");

    let (mut outro_bus, mut outro, regs) = setup();
    let com_pagina = call(&mut outro_bus, &mut outro, &regs, args()).unwrap();
    assert_eq!(com_zero, com_pagina);
    assert_eq!(d.contiguous_memory().allocations()[0].alignment, 4096);
    assert_eq!(
        d.contiguous_memory().allocations(),
        outro.contiguous_memory().allocations()
    );
}
