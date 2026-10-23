use xbox_core::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_formats::fixtures::xbe_com_imports;
use xbox_kernel::{
    dispatch::{Dispatcher, Erro, Irql},
    imports,
    loader::{Image, STACK_BASE, STACK_SIZE},
};
use xbox_mem::{Acesso, Bus, GuestPhys, GuestVirt, Perm};
fn setup() -> (Bus, Dispatcher, [u32; 8]) {
    let image = xbe_com_imports(&[0xC3], &[168]).unwrap();
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
    size: u32,
    out: u32,
) -> Result<u32, Erro> {
    for (i, value) in [0x11000, size, out].into_iter().enumerate() {
        bus.escrever_u32(GuestVirt(regs[4] + i as u32 * 4), value)
            .unwrap();
    }
    d.atender(bus, imports::trampoline(168), regs)
        .map(|v| v.eax)
}
fn saved(v: &impl Snapshot) -> Vec<u8> {
    let mut w = SnapshotWriter::new();
    v.save(&mut w);
    w.finish()
}
#[test]
fn consulta_provisiona_frames_reais_padding_e_repeticao_preserva_bytes() {
    let (mut bus, mut d, r) = setup();
    let used = bus.frames().usados();
    assert_eq!(
        call(&mut bus, &mut d, &r, u32::MAX, 0x12101).unwrap(),
        0x83FF0000
    );
    assert_eq!(bus.ler_u32(GuestVirt(0x12101)).unwrap(), 65536);
    assert_eq!(bus.frames().usados(), used + 32);
    let region = d.contiguous_memory().allocations()[0];
    assert_eq!(region.physical, GuestPhys(0x03FE0000));
    assert_eq!(region.size, 0x20000);
    for offset in (0..0x20000).step_by(4096) {
        let pa = GuestPhys(0x03FE0000 + offset);
        assert!(bus.frames().em_uso(pa.frame()));
        assert_eq!(
            bus.mmu().consultar(GuestVirt(0x80000000 + pa.0)),
            Some((pa, Perm::RW))
        );
        assert!(bus.dma(pa, 4096).unwrap().iter().all(|v| *v == 0));
    }
    assert!(bus.mmu().consultar(GuestVirt(0x83FDF000)).is_none());
    assert!(bus.mmu().consultar(GuestVirt(0x84000000)).is_none());
    assert!(
        bus.traduzir(GuestVirt(0x83FE0000), Acesso::Execucao)
            .is_err()
    );
    bus.escrever_u32(GuestVirt(0x83FEB000), 0xCAFEBABE).unwrap();
    assert_eq!(
        call(&mut bus, &mut d, &r, u32::MAX, 0x12101).unwrap(),
        0x83FF0000
    );
    assert_eq!(bus.frames().usados(), used + 32);
    assert_eq!(d.contiguous_memory().allocations().len(), 1);
    assert_eq!(
        bus.dma(GuestPhys(0x03FEB000), 4).unwrap(),
        &0xCAFEBABEu32.to_le_bytes()
    );
}
#[test]
fn saida_invalida_recusa_antes_da_reserva_e_colisao_nao_desloca_pool() {
    let (mut bus, mut d, r) = setup();
    let used = bus.frames().usados();
    bus.mapear(GuestVirt(0x50000000), GuestPhys(0xFD000000), 1, Perm::RW)
        .unwrap();
    for out in [0, 0x11000, 0x12FFE, u32::MAX - 1, 0x50000000] {
        assert!(matches!(
            call(&mut bus, &mut d, &r, u32::MAX, out),
            Err(Erro::ArgumentoInvalido { .. })
        ));
    }
    assert_eq!(bus.frames().usados(), used);
    assert!(!d.gpu_instance_memory().provisioned());
    bus.frames_mut().reservar(GuestPhys(0x03FE1000), 1).unwrap();
    bus.escrever_u32(GuestVirt(0x12100), 0xAABBCCDD).unwrap();
    assert_eq!(call(&mut bus, &mut d, &r, u32::MAX, 0x12100).unwrap(), 0);
    assert_eq!(bus.frames().usados(), used + 1);
    assert_eq!(bus.ler_u32(GuestVirt(0x12100)).unwrap(), 0xAABBCCDD);
    assert_eq!(d.contiguous_memory().allocations(), []);
}
#[test]
fn resize_e_prioridade_sem_modelo_recusam_sem_alocar() {
    let (mut bus, mut d, r) = setup();
    let used = bus.frames().usados();
    for size in [0, 1, 0x5000, 0x10000] {
        assert!(matches!(
            call(&mut bus, &mut d, &r, size, 0x12100),
            Err(Erro::VarianteNaoImplementada { .. })
        ));
    }
    let mut elevated = Dispatcher::with_irql(Irql::Dispatch);
    assert!(call(&mut bus, &mut elevated, &r, u32::MAX, 0x12100).is_err());
    assert_eq!(bus.frames().usados(), used);
}
#[test]
fn snapshot_preserva_reserva_e_rejeita_marcador_sem_recurso_transacionalmente() {
    let (mut bus, mut d, r) = setup();
    let cold = saved(&d);
    call(&mut bus, &mut d, &r, u32::MAX, 0x12100).unwrap();
    let active = saved(&d);
    let mut restored = Dispatcher::new();
    restored
        .load(&mut SnapshotReader::new(&active).unwrap())
        .unwrap();
    restored.restore_memory_context(None, &bus).unwrap();
    assert_eq!(saved(&restored), active);
    for (version, value) in [(0u32, 0u8), (2, 0), (1, 2)] {
        let mut bad = active.clone();
        let at = bad.len() - 9;
        bad[at + 4..at + 8].copy_from_slice(&version.to_le_bytes());
        bad[at + 8] = value;
        assert!(
            restored
                .load(&mut SnapshotReader::new(&bad).unwrap())
                .is_err()
        );
        assert_eq!(saved(&restored), active);
    }
    let mut orphan = cold;
    *orphan.last_mut().unwrap() = 1;
    assert!(
        restored
            .load(&mut SnapshotReader::new(&orphan).unwrap())
            .is_err()
    );
    assert_eq!(saved(&restored), active);
    let pa = GuestPhys(0x03FE0000);
    bus.frames_mut().liberar(pa, 1);
    assert!(restored.restore_memory_context(None, &bus).is_err());
}
#[test]
fn krnl_v12_migra_para_nao_provisionado_sem_herdar_destino() {
    let (mut bus, mut d, r) = setup();
    let mut old = saved(&d);
    old.truncate(old.len() - 9);
    old[16..20].copy_from_slice(&12u32.to_le_bytes());
    call(&mut bus, &mut d, &r, u32::MAX, 0x12100).unwrap();
    let mut reader = SnapshotReader::new(&old).unwrap();
    d.load(&mut reader).unwrap();
    assert_eq!(reader.remaining(), 0);
    assert!(!d.gpu_instance_memory().provisioned());
    assert_eq!(d.contiguous_memory().allocations(), []);
}
