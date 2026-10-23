use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_formats::{Xbe, fixtures::xbe_com_imports};
use xbox_kernel::dispatch::{
    Dispatcher, Erro, Irql, STATUS_ACCESS_VIOLATION, STATUS_INVALID_PARAMETER, STATUS_NO_MEMORY,
    VirtualMemory,
};
use xbox_kernel::imports;
use xbox_kernel::loader::{Image, STACK_BASE, STACK_SIZE};
use xbox_mem::{Acesso, Bus, GuestPhys, GuestVirt, Perm};

fn setup(image_size: Option<u32>) -> (Bus, Dispatcher, [u32; 8]) {
    let mut bytes = xbe_com_imports(&[0xC3], &[184]).unwrap();
    if let Some(size) = image_size {
        bytes[0x10C..0x110].copy_from_slice(&size.to_le_bytes());
    }
    let xbe = Xbe::parse(&bytes).unwrap();
    let kernel = Dispatcher::for_image(xbe.header.base_address, xbe.header.size_of_image).unwrap();
    let bus = Image::prepare_aot(&bytes).unwrap().image.bus;
    let mut regs = [0; 8];
    regs[4] = STACK_BASE + STACK_SIZE - 0x100;
    (bus, kernel, regs)
}
fn call(bus: &mut Bus, d: &mut Dispatcher, regs: &[u32; 8], args: [u32; 5]) -> Result<u32, Erro> {
    bus.escrever_u32(GuestVirt(regs[4]), 0x11000).unwrap();
    for (i, arg) in args.into_iter().enumerate() {
        bus.escrever_u32(GuestVirt(regs[4] + 4 + i as u32 * 4), arg)
            .unwrap();
    }
    d.atender(bus, imports::trampoline(184), regs)
        .map(|result| result.eax)
}
fn args(bus: &mut Bus, size: u32) -> [u32; 5] {
    bus.escrever_u32(GuestVirt(0x12100), 0).unwrap();
    bus.escrever_u32(GuestVirt(0x12104), size).unwrap();
    [0x12100, 0, 0x12104, 0x3000, 4]
}
fn saved(value: &impl Snapshot) -> Vec<u8> {
    let mut out = SnapshotWriter::new();
    value.save(&mut out);
    out.finish()
}

#[test]
fn reserva_commit_arredonda_tamanho_aloca_frames_fragmentados_e_zera_ram_suja() {
    let (mut bus, mut d, regs) = setup(None);
    let mut holes = Vec::new();
    for _ in 0..6 {
        let pa = bus.frames_mut().alocar().unwrap();
        bus.carregar_em_fisico(pa, &[0xAA; 4096]).unwrap();
        holes.push(pa);
    }
    for i in [0, 2, 4] {
        bus.frames_mut().liberar(holes[i], 1);
    }
    let used = bus.frames().usados();
    let pages = bus.mmu().paginas_mapeadas();
    let a = args(&mut bus, 0x2001);
    assert_eq!(call(&mut bus, &mut d, &regs, a).unwrap(), 0);
    let base = bus.ler_u32(GuestVirt(0x12100)).unwrap();
    assert_eq!(base % 65536, 0);
    assert_eq!(bus.ler_u32(GuestVirt(0x12104)).unwrap(), 0x3000);
    for (i, pa) in [holes[0], holes[2], holes[4]].into_iter().enumerate() {
        let at = base + i as u32 * 4096;
        assert_eq!(bus.mmu().consultar(GuestVirt(at)), Some((pa, Perm::RW)));
        assert!(bus.dma(pa, 4096).unwrap().iter().all(|byte| *byte == 0));
        assert!(bus.traduzir(GuestVirt(at), Acesso::Execucao).is_err());
    }
    bus.escrever_u32(GuestVirt(base + 4094), 0xCAFEBABE)
        .unwrap();
    assert_eq!(bus.ler_u32(GuestVirt(base + 4094)).unwrap(), 0xCAFEBABE);
    assert!(bus.mmu().consultar(GuestVirt(base + 0x3000)).is_none());
    assert_eq!(bus.frames().usados(), used + 3);
    assert_eq!(bus.mmu().paginas_mapeadas(), pages + 3);
    let a = args(&mut bus, 1);
    assert_eq!(call(&mut bus, &mut d, &regs, a).unwrap(), 0);
    assert!(bus.ler_u32(GuestVirt(0x12100)).unwrap() >= base + 0x10000);
    assert_eq!(d.virtual_memory().allocations().len(), 2);
}

#[test]
fn first_fit_preserva_buracos_da_imagem_e_pula_mapeamentos_alheios() {
    let (mut bus, mut d, regs) = setup(Some(0x48000)); // imagem inteira até 0x58000, com buracos.
    bus.mapear(GuestVirt(0x61000), GuestPhys(0xFD000000), 1, Perm::RW)
        .unwrap();
    let a = args(&mut bus, 8192);
    assert_eq!(call(&mut bus, &mut d, &regs, a).unwrap(), 0);
    assert_eq!(bus.ler_u32(GuestVirt(0x12100)).unwrap(), 0x70000);
    assert!(bus.mmu().consultar(GuestVirt(0x20000)).is_none());
    assert_eq!(
        bus.mmu().consultar(GuestVirt(0x61000)),
        Some((GuestPhys(0xFD000000), Perm::RW))
    );
}

#[test]
fn ponteiros_tamanhos_e_aliases_invalidos_nao_publicam_saida_ou_alocacao() {
    let (mut bus, mut d, regs) = setup(None);
    let pa = bus.mmu().consultar(GuestVirt(0x12000)).unwrap().0;
    bus.mapear(GuestVirt(0x40000000), pa, 1, Perm::RW).unwrap();
    bus.mapear(GuestVirt(0x50000000), GuestPhys(0xFD000000), 1, Perm::RW)
        .unwrap();
    let used = bus.frames().usados();
    let pages = bus.mmu().paginas_mapeadas();
    for pointer in [0, 0x11000, 0x12FFE, u32::MAX - 1, 0x60000000, 0x50000000] {
        let a = args(&mut bus, 4096);
        for index in [0, 2] {
            let mut bad = a;
            bad[index] = pointer;
            assert_eq!(
                call(&mut bus, &mut d, &regs, bad).unwrap(),
                STATUS_ACCESS_VIOLATION
            );
            assert_eq!(bus.ler_u32(GuestVirt(0x12100)).unwrap(), 0);
            assert_eq!(bus.ler_u32(GuestVirt(0x12104)).unwrap(), 4096);
        }
    }
    for size in [0, u32::MAX] {
        let a = args(&mut bus, size);
        assert_eq!(
            call(&mut bus, &mut d, &regs, a).unwrap(),
            STATUS_INVALID_PARAMETER
        );
        assert_eq!(bus.ler_u32(GuestVirt(0x12104)).unwrap(), size);
    }
    for output in [0x12100, 0x12102, 0x40000100] {
        let mut a = args(&mut bus, 4096);
        a[2] = output;
        assert_eq!(
            call(&mut bus, &mut d, &regs, a).unwrap(),
            STATUS_INVALID_PARAMETER
        );
    }
    assert_eq!(bus.frames().usados(), used);
    assert_eq!(bus.mmu().paginas_mapeadas(), pages);
    assert_eq!(d.virtual_memory().allocations(), []);
}

#[test]
fn falta_de_frames_e_variantes_recusadas_preservam_memoria_e_permissoes() {
    let (mut bus, mut d, regs) = setup(None);
    let used = bus.frames().usados();
    let mappings = bus.mmu().paginas_mapeadas();
    for (index, value) in [
        (1, 1),
        (3, 0x1000),
        (3, 0x2000),
        (3, 0x103000),
        (4, 2),
        (4, 0x40),
    ] {
        let mut a = args(&mut bus, 4096);
        a[index] = value;
        assert!(matches!(
            call(&mut bus, &mut d, &regs, a),
            Err(Erro::VarianteNaoImplementada { .. })
        ));
    }
    let a = args(&mut bus, 4096);
    bus.escrever_u32(GuestVirt(a[0]), 0x90000).unwrap();
    assert!(matches!(
        call(&mut bus, &mut d, &regs, a),
        Err(Erro::VarianteNaoImplementada { .. })
    ));
    let image = d.virtual_memory().image();
    let mut elevated = Dispatcher::with_irql(Irql::Dispatch);
    let a = args(&mut bus, 4096);
    assert!(call(&mut bus, &mut elevated, &regs, a).is_err());
    let mut unbound = Dispatcher::new();
    assert!(call(&mut bus, &mut unbound, &regs, a).is_err());
    let too_large = (bus.frames().livres() + 1) * 4096;
    let a = args(&mut bus, too_large);
    assert_eq!(call(&mut bus, &mut d, &regs, a).unwrap(), STATUS_NO_MEMORY);
    assert_eq!(bus.frames().usados(), used);
    assert_eq!(bus.mmu().paginas_mapeadas(), mappings);
    assert_eq!(d.virtual_memory().image(), image);
    assert_eq!(d.virtual_memory().allocations(), []);
}

#[test]
fn snapshot_logico_valida_intervalos_contagens_versoes_e_carga_transacional() {
    let (mut bus, mut d, regs) = setup(None);
    let a = args(&mut bus, 8192);
    call(&mut bus, &mut d, &regs, a).unwrap();
    let bytes = saved(&d);
    let mut restored = Dispatcher::new();
    let mut input = SnapshotReader::new(&bytes).unwrap();
    restored.load(&mut input).unwrap();
    assert_eq!(input.remaining(), 0);
    assert_eq!(saved(&restored), bytes);
    restored.restore_memory_context(None, &bus).unwrap();
    let at = bytes.windows(4).position(|tag| tag == b"VMEM").unwrap();
    // VMEM v1: tag/version, image bool, base/size, count, allocation base/size.
    for (offset, value) in [
        (4, 0u32),
        (4, 2),
        (13, u32::MAX),
        (17, 5000),
        (21, 0x21001),
        (21, 0),
        (25, 0),
        (25, u32::MAX),
    ] {
        let mut bad = bytes.clone();
        bad[at + offset..at + offset + 4].copy_from_slice(&value.to_le_bytes());
        assert!(
            restored
                .load(&mut SnapshotReader::new(&bad).unwrap())
                .is_err()
        );
        assert_eq!(saved(&restored), bytes);
    }
    for end in 12..bytes.len() {
        assert!(
            restored
                .load(&mut SnapshotReader::new(&bytes[..end]).unwrap())
                .is_err()
        );
        assert_eq!(saved(&restored), bytes);
    }
}

#[test]
fn snapshot_de_sessao_exige_coerencia_de_registro_mmu_e_frames() {
    let (mut bus, mut d, regs) = setup(None);
    let a = args(&mut bus, 8192);
    call(&mut bus, &mut d, &regs, a).unwrap();
    let region = d.virtual_memory().allocations()[0];
    let pa = bus.mmu().consultar(GuestVirt(region.base)).unwrap().0;
    let frame = bus
        .mmu()
        .consultar(GuestVirt(region.base + 4096))
        .unwrap()
        .0;
    let check = |d: &mut Dispatcher, bus: &Bus| d.restore_memory_context(None, bus);
    bus.mapear(GuestVirt(region.base), pa, 1, Perm::RO).unwrap();
    assert!(check(&mut d, &bus).is_err());
    bus.mapear(GuestVirt(region.base), pa, 1, Perm::RW).unwrap();
    bus.frames_mut().liberar(pa, 1);
    assert!(check(&mut d, &bus).is_err());
    bus.frames_mut().reservar(pa, 1).unwrap();
    bus.mapear(GuestVirt(region.base + 4096), pa, 1, Perm::RW)
        .unwrap();
    assert!(check(&mut d, &bus).is_err());
    bus.mapear(GuestVirt(region.base + 4096), frame, 1, Perm::RW)
        .unwrap();
    check(&mut d, &bus).unwrap();
    bus.mapear(GuestVirt(0x40000000), pa, 1, Perm::RW).unwrap();
    assert!(check(&mut d, &bus).is_err());
    bus.mmu_mut().desmapear(GuestVirt(0x40000000), 1);
    let header = bus.mmu().consultar(GuestVirt(0x10000)).unwrap().0;
    bus.frames_mut().liberar(header, 1);
    assert!(check(&mut d, &bus).is_err());
    bus.frames_mut().reservar(header, 1).unwrap();
    check(&mut d, &bus).unwrap();
    bus.mmu_mut().desmapear(GuestVirt(region.base), 1);
    assert!(check(&mut d, &bus).is_err());
}

#[test]
fn krnl_v1_a_v5_migram_sem_alocacoes_e_recuperam_contexto_do_xbe_validado() {
    let (bus, d, _) = setup(None);
    let image = d.virtual_memory().image();
    for version in 1u32..=5 {
        // Deriva o prefixo de um snapshot novo; a estrutura anterior é preservada.
        let mut bytes = saved(&d);
        let at = bytes.windows(4).position(|tag| tag == b"VMEM").unwrap();
        bytes.truncate(at);
        let stop = match version {
            1 => b"EEPR",
            2 => b"PCIC",
            3 => b"STIM",
            4 => b"IRQL",
            _ => b"VMEM",
        };
        if version < 5 {
            let at = bytes.windows(4).position(|tag| tag == stop).unwrap();
            bytes.truncate(at);
        }
        bytes[16..20].copy_from_slice(&version.to_le_bytes());
        let mut restored = Dispatcher::new();
        let mut input = SnapshotReader::new(&bytes).unwrap();
        restored.load(&mut input).unwrap();
        assert_eq!(input.remaining(), 0);
        assert_eq!(restored.virtual_memory().allocations(), []);
        assert!(restored.virtual_memory().image().is_none());
        restored.restore_memory_context(image, &bus).unwrap();
        assert_eq!(restored.virtual_memory().image(), image);
    }
    let mut memory = VirtualMemory::default();
    let bytes = saved(&memory);
    memory
        .load(&mut SnapshotReader::new(&bytes).unwrap())
        .unwrap();
}
