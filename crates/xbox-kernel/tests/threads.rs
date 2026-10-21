use xbox_core::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_formats::{Xbe, fixtures::xbe_com_imports};
use xbox_kernel::dispatch::{
    Dispatcher, Erro, STATUS_ACCESS_VIOLATION, STATUS_INVALID_PARAMETER, STATUS_MUTANT_NOT_OWNED,
    STATUS_NO_MEMORY, ThreadPhase,
};
use xbox_kernel::imports;
use xbox_kernel::loader::{Image, RETURN_SENTINEL, STACK_BASE, STACK_SIZE};
use xbox_mem::{Acesso, Bus, GuestVirt, Perm};

fn setup() -> (Bus, Dispatcher, [u32; 8]) {
    let bytes = xbe_com_imports(&[0xC3], &[255, 104, 258, 192, 233, 221, 291, 306, 294]).unwrap();
    let xbe = Xbe::parse(&bytes).unwrap();
    let d = Dispatcher::for_image(xbe.header.base_address, xbe.header.size_of_image).unwrap();
    let bus = Image::prepare_aot(&bytes).unwrap().image.bus;
    let mut regs = [0; 8];
    regs[4] = STACK_BASE + STACK_SIZE - 0x100;
    (bus, d, regs)
}
fn args() -> [u32; 10] {
    [
        0x12100, 0, 65536, 20, 0x12104, 0x11000, 0x12345678, 0, 0, 0x11000,
    ]
}
fn call(
    bus: &mut Bus,
    d: &mut Dispatcher,
    regs: &[u32; 8],
    ordinal: u32,
    args: &[u32],
) -> Result<u32, Erro> {
    bus.escrever_u32(GuestVirt(regs[4]), 0x11000).unwrap();
    for (i, arg) in args.iter().enumerate() {
        bus.escrever_u32(GuestVirt(regs[4] + 4 + i as u32 * 4), *arg)
            .unwrap();
    }
    d.atender(bus, imports::trampoline(ordinal), regs)
        .map(|r| r.eax)
}
fn saved(value: &impl Snapshot) -> Vec<u8> {
    let mut out = SnapshotWriter::new();
    value.save(&mut out);
    out.finish()
}

#[test]
fn cria_recursos_reais_zero_tls_abi_e_prefixo_kthread_com_pagina_seguinte_ausente() {
    let (mut bus, mut d, regs) = setup();
    let used = bus.frames().usados();
    assert_eq!(call(&mut bus, &mut d, &regs, 255, &args()).unwrap(), 0);
    let child = d.child_thread().unwrap();
    assert_eq!(bus.frames().usados(), used + 17);
    assert_eq!(child.region.base % 65536, 0);
    assert_eq!(child.phase, ThreadPhase::Ready);
    assert_eq!(d.current_thread(), 1);
    assert_eq!(bus.ler_u32(GuestVirt(0x12100)).unwrap(), child.handle);
    assert_eq!(bus.ler_u32(GuestVirt(0x12104)).unwrap(), 2);
    for (at, value) in [
        (child.esp(), RETURN_SENTINEL),
        (child.esp() + 4, 0x11000),
        (child.esp() + 8, 0x12345678),
        (child.tib() + 0x18, child.tib()),
        (child.object() + 0x28, child.tls()),
        (child.tls(), 0),
    ] {
        assert_eq!(bus.ler_u32(GuestVirt(at)).unwrap(), value);
    }
    assert_eq!(child.tls() % 16, 12); // self-slot; dados começam em endereço alinhado.
    assert!(
        bus.traduzir(GuestVirt(child.region.base), Acesso::Execucao)
            .is_err()
    );
    assert!(
        bus.traduzir(GuestVirt(child.object() + 0x2C), Acesso::Leitura)
            .is_err()
    );
    assert!(d.start_ready_thread().is_some());
    assert!(d.start_ready_thread().is_none());
    assert_eq!(d.current_thread(), 2);
    assert_eq!(
        call(&mut bus, &mut d, &regs, 104, &[]).unwrap(),
        child.object()
    );
}

#[test]
fn saidas_invalidas_aliases_e_tamanhos_nao_publicam_recursos_ou_handle() {
    let (mut bus, mut d, regs) = setup();
    let pa = bus.mmu().consultar(GuestVirt(0x12000)).unwrap().0;
    bus.mapear(GuestVirt(0x40000000), pa, 1, Perm::RW).unwrap();
    let used = bus.frames().usados();
    let pages = bus.mmu().paginas_mapeadas();
    for ptr in [0, 0x11000, 0x12FFE, u32::MAX - 1] {
        let mut a = args();
        a[0] = ptr;
        assert_eq!(
            call(&mut bus, &mut d, &regs, 255, &a).unwrap(),
            STATUS_ACCESS_VIOLATION
        );
    }
    for (index, value) in [
        (4, 0x12100),
        (4, 0x12102),
        (4, 0x40000100),
        (2, 0),
        (2, u32::MAX),
        (3, 16),
        (3, 0x10004),
    ] {
        let mut a = args();
        a[index] = value;
        assert_eq!(
            call(&mut bus, &mut d, &regs, 255, &a).unwrap(),
            STATUS_INVALID_PARAMETER
        );
    }
    assert_eq!(bus.frames().usados(), used);
    assert_eq!(bus.mmu().paginas_mapeadas(), pages);
    assert!(d.child_thread().is_none());
    assert!(d.virtual_memory().allocations().is_empty());
    assert_eq!(bus.ler_u32(GuestVirt(0x12100)).unwrap(), 0);
    assert_eq!(bus.ler_u32(GuestVirt(0x12104)).unwrap(), 0);
}

#[test]
fn variantes_e_rotinas_fora_de_codigo_imutavel_sao_recusadas_antes_de_alocar() {
    let (mut bus, mut d, regs) = setup();
    let used = bus.frames().usados();
    for (index, value) in [
        (1, 4),
        (7, 1),
        (8, 1),
        (9, 0),
        (9, 0x12100),
        (5, 0x50000000),
    ] {
        let mut a = args();
        a[index] = value;
        assert!(matches!(
            call(&mut bus, &mut d, &regs, 255, &a),
            Err(Erro::VarianteNaoImplementada { .. })
        ));
    }
    assert_eq!(bus.frames().usados(), used);
    assert!(d.child_thread().is_none());
    assert!(call(&mut bus, &mut d, &regs, 104, &[]).is_err());
    assert!(call(&mut bus, &mut d, &regs, 258, &[0]).is_err());
}

#[test]
fn oom_preserva_saidas_bitmap_e_regioes() {
    let (mut bus, mut d, regs) = setup();
    while bus.frames().livres() > 16 {
        bus.frames_mut().alocar().unwrap();
    }
    let used = bus.frames().usados();
    let pages = bus.mmu().paginas_mapeadas();
    assert_eq!(
        call(&mut bus, &mut d, &regs, 255, &args()).unwrap(),
        STATUS_NO_MEMORY
    );
    assert_eq!(bus.frames().usados(), used);
    assert_eq!(bus.mmu().paginas_mapeadas(), pages);
    assert!(d.child_thread().is_none());
    assert!(d.virtual_memory().allocations().is_empty());
    assert_eq!(bus.ler_u32(GuestVirt(0x12100)).unwrap(), 0);
}

#[test]
fn handles_compartilhados_e_mutants_respeitam_identidade_da_filha() {
    let (mut bus, mut d, regs) = setup();
    call(&mut bus, &mut d, &regs, 192, &[0x12108, 0, 1]).unwrap();
    let parent_mutex = bus.ler_u32(GuestVirt(0x12108)).unwrap();
    call(&mut bus, &mut d, &regs, 255, &args()).unwrap();
    let child = d.start_ready_thread().unwrap();
    assert_ne!(parent_mutex, child.handle);
    assert!(call(&mut bus, &mut d, &regs, 233, &[parent_mutex, 0, 0]).is_err());
    assert_eq!(
        call(&mut bus, &mut d, &regs, 221, &[parent_mutex, 0]).unwrap(),
        STATUS_MUTANT_NOT_OWNED
    );
    call(&mut bus, &mut d, &regs, 192, &[0x1210C, 0, 1]).unwrap();
    let own = bus.ler_u32(GuestVirt(0x1210C)).unwrap();
    assert_ne!(own, child.handle);
    assert_eq!(d.mutant(own).unwrap().owner, Some(2));
    assert!(call(&mut bus, &mut d, &regs, 258, &[42]).is_err());
    assert_eq!(d.current_thread(), 2);
    assert_eq!(call(&mut bus, &mut d, &regs, 221, &[own, 0]).unwrap(), 0);
    call(&mut bus, &mut d, &regs, 258, &[42]).unwrap();
    assert_eq!(d.current_thread(), 1);
    assert_eq!(d.child_thread().unwrap().phase, ThreadPhase::Terminated(42));
    assert_eq!(
        call(&mut bus, &mut d, &regs, 233, &[child.handle, 0, 0]).unwrap(),
        0
    );
    assert_eq!(d.mutant(parent_mutex).unwrap().owner, Some(1));
}

#[test]
fn secao_critica_nao_permite_recursao_ou_liberacao_pela_outra_thread() {
    let (mut bus, mut d, regs) = setup();
    call(&mut bus, &mut d, &regs, 291, &[0x12200]).unwrap();
    assert_eq!(call(&mut bus, &mut d, &regs, 306, &[0x12200]).unwrap(), 1);
    call(&mut bus, &mut d, &regs, 255, &args()).unwrap();
    d.start_ready_thread().unwrap();
    assert_eq!(call(&mut bus, &mut d, &regs, 306, &[0x12200]).unwrap(), 0);
    assert!(call(&mut bus, &mut d, &regs, 294, &[0x12200]).is_err());
    assert_eq!(bus.ler_u32(GuestVirt(0x12218)).unwrap(), 1);
}

#[test]
fn snapshot_valida_objetos_posse_tib_regiao_e_guarda_sem_marcar_ad() {
    let (mut bus, mut d, regs) = setup();
    call(&mut bus, &mut d, &regs, 255, &args()).unwrap();
    let child = d.start_ready_thread().unwrap();
    call(&mut bus, &mut d, &regs, 192, &[0x12108, 0, 1]).unwrap();
    let checkpoint = saved(&d);
    let mut restored = Dispatcher::default();
    Snapshot::load(
        &mut restored,
        &mut SnapshotReader::new(&checkpoint).unwrap(),
    )
    .unwrap();
    let before = saved(bus.mmu());
    restored
        .restore_memory_context(d.virtual_memory().image(), &bus)
        .unwrap();
    assert_eq!(saved(bus.mmu()), before);
    assert_eq!(saved(&restored), checkpoint);
    bus.escrever_u32(GuestVirt(child.object() + 0x28), child.tls() + 4)
        .unwrap();
    assert!(
        restored
            .restore_memory_context(d.virtual_memory().image(), &bus)
            .is_err()
    );
    let mut bad = checkpoint;
    let t = bad.windows(4).rposition(|v| v == b"THRD").unwrap();
    bad[t + 9..t + 13].copy_from_slice(&4u32.to_le_bytes()); // colide com mutant, se presente.
    // Uma geometria impossível é sempre recusada, mesmo antes de consultar o Bus.
    bad[t + 21..t + 25].copy_from_slice(&4096u32.to_le_bytes());
    assert!(Snapshot::load(&mut restored, &mut SnapshotReader::new(&bad).unwrap()).is_err());
}

#[test]
fn segunda_criacao_e_espera_de_thread_viva_sao_erros_explicitos() {
    let (mut bus, mut d, regs) = setup();
    call(&mut bus, &mut d, &regs, 255, &args()).unwrap();
    let child = d.start_ready_thread().unwrap();
    let used = bus.frames().usados();
    assert!(call(&mut bus, &mut d, &regs, 255, &args()).is_err());
    assert_eq!(bus.frames().usados(), used);
    assert!(call(&mut bus, &mut d, &regs, 233, &[child.handle, 0, 0]).is_err());
}
