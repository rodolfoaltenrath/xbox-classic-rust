use xbox_core::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_formats::{
    eeprom::{Idioma, PadraoDeVideo},
    fixtures::xbe_com_imports,
};
use xbox_kernel::{
    dispatch::{Dispatcher, EepromState, Erro, VideoState},
    imports,
    loader::{Image, STACK_BASE, STACK_SIZE},
};
use xbox_mem::{Bus, GuestPhys, GuestVirt, Perm};
fn setup_with(eeprom: EepromState) -> (Bus, Dispatcher, [u32; 8]) {
    let bytes = xbe_com_imports(&[0xC3], &[166, 3, 2]).unwrap();
    let mut bus = Image::prepare_aot(&bytes).unwrap().image.bus;
    let mut d = Dispatcher::with_eeprom(eeprom);
    let mut regs = [0; 8];
    regs[4] = STACK_BASE + STACK_SIZE - 0x100;
    assert_eq!(
        call(
            &mut bus,
            &mut d,
            &regs,
            166,
            &[0x180000, 0x20000, 0x3FFFFFF, 4096, 0x404]
        )
        .unwrap(),
        0x80020000
    );
    (bus, d, regs)
}
fn setup() -> (Bus, Dispatcher, [u32; 8]) {
    setup_with(EepromState::default())
}
fn call(
    bus: &mut Bus,
    d: &mut Dispatcher,
    regs: &[u32; 8],
    ordinal: u32,
    args: &[u32],
) -> Result<u32, Erro> {
    bus.escrever_u32(GuestVirt(regs[4]), 0x11000).unwrap();
    for (i, &arg) in args.iter().enumerate() {
        bus.escrever_u32(GuestVirt(regs[4] + 4 + i as u32 * 4), arg)
            .unwrap();
    }
    d.atender(bus, imports::trampoline(ordinal), regs)
        .map(|r| r.eax)
}
fn args() -> [u32; 6] {
    [0xFD000000, 0, 0x04010101, 0x12, 2560, 0x20000]
}
fn configure(bus: &mut Bus, d: &mut Dispatcher, regs: &[u32; 8]) {
    assert_eq!(call(bus, d, regs, 3, &args()).unwrap(), 0);
}
fn saved(value: &impl Snapshot) -> Vec<u8> {
    let mut out = SnapshotWriter::new();
    value.save(&mut out);
    out.finish()
}
#[test]
fn modo_ntsc_framebuffer_por_pa_e_linhas_bgra_com_pitch_e_padding() {
    let (mut bus, mut d, regs) = setup();
    assert!(d.framebuffer(&bus).unwrap().is_none());
    let mut a = args();
    a[4] = 2576;
    a[5] += 16;
    assert_eq!(call(&mut bus, &mut d, &regs, 3, &a).unwrap(), 0);
    bus.escrever_u32(GuestVirt(0x80020010), 0xFF123456).unwrap();
    bus.escrever_bytes(GuestVirt(0x80020010 + 2560), &[0xAA; 16])
        .unwrap();
    bus.escrever_u32(GuestVirt(0x80020010 + 2576), 0xFFABCDEF)
        .unwrap();
    let view = d.framebuffer(&bus).unwrap().unwrap();
    let mode = view.mode();
    assert_eq!(
        (mode.width(), mode.height(), mode.refresh_hz()),
        (640, 480, 60)
    );
    assert_eq!(
        (mode.id(), mode.format(), mode.pitch(), mode.framebuffer()),
        (0x04010101, 0x12, 2576, GuestPhys(0x20010))
    );
    assert_eq!(view.row(0).unwrap().len(), 2560);
    assert_eq!(&view.row(0).unwrap()[..4], &[0x56, 0x34, 0x12, 0xFF]);
    assert_eq!(&view.row(1).unwrap()[..4], &[0xEF, 0xCD, 0xAB, 0xFF]);
    assert!(view.row(480).is_none());
    assert!(view.row(479).is_some());
    assert!(!view.enabled());
}
#[test]
fn variantes_e_geometrias_invalidas_preservam_modo_anterior_e_ram() {
    let (mut bus, mut d, regs) = setup();
    configure(&mut bus, &mut d, &regs);
    let mode = d.video().mode();
    for (index, value) in [(0, 0), (1, 1), (2, 0x44030307), (3, 0x11)] {
        let mut a = args();
        a[index] = value;
        assert!(matches!(
            call(&mut bus, &mut d, &regs, 3, &a),
            Err(Erro::VarianteNaoImplementada { .. })
        ));
        assert_eq!(d.video().mode(), mode);
    }
    for (index, value) in [
        (4, 0),
        (4, 2559),
        (4, 2561),
        (4, u32::MAX),
        (4, 16388),
        (5, 0x80020000),
        (5, 0x20001),
        (5, 0x19F000),
        (5, 0xFFFFFFFC),
    ] {
        let mut a = args();
        a[index] = value;
        assert!(call(&mut bus, &mut d, &regs, 3, &a).is_err());
        assert_eq!(d.video().mode(), mode);
    }
    assert!(
        bus.dma(GuestPhys(0x20000), 0x180000)
            .unwrap()
            .iter()
            .all(|&b| b == 0)
    );
    let (mut bus, mut pal, regs) =
        setup_with(EepromState::synthetic(PadraoDeVideo::PalI, Idioma::Ingles));
    assert!(matches!(
        call(&mut bus, &mut pal, &regs, 3, &args()),
        Err(Erro::VarianteNaoImplementada { .. })
    ));
    assert!(pal.video().mode().is_none());
}
#[test]
fn snapshot_modo_cruza_cmem_eeprom_e_rejeita_metadados_sem_modelo() {
    let (mut bus, mut d, regs) = setup();
    configure(&mut bus, &mut d, &regs);
    let checkpoint = saved(&d);
    let mut restored = Dispatcher::new();
    Snapshot::load(
        &mut restored,
        &mut SnapshotReader::new(&checkpoint).unwrap(),
    )
    .unwrap();
    assert_eq!(saved(&restored), checkpoint);
    assert_eq!(restored.video().mode(), d.video().mode());
    let tag = checkpoint.windows(4).rposition(|v| v == b"AVST").unwrap();
    for (offset, value) in [(14, 0u32), (18, 0x11), (22, 0), (26, 0x1000)] {
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
    let mut bad = checkpoint;
    bad[tag + 13] = 2;
    assert!(
        Snapshot::load(
            &mut Dispatcher::new(),
            &mut SnapshotReader::new(&bad).unwrap()
        )
        .is_err()
    );
}
#[test]
fn framebuffer_e_restore_nao_marcam_ad_e_recusam_mapa_corrompido() {
    let (mut bus, mut d, regs) = setup();
    configure(&mut bus, &mut d, &regs);
    let before = saved(bus.mmu());
    d.restore_memory_context(None, &bus).unwrap();
    assert!(d.framebuffer(&bus).unwrap().unwrap().row(0).is_some());
    assert_eq!(saved(bus.mmu()), before);
    bus.mapear(GuestVirt(0x80020000), GuestPhys(0x20000), 1, Perm::RWX)
        .unwrap();
    assert!(d.framebuffer(&bus).is_err());
    assert!(d.restore_memory_context(None, &bus).is_err());
}
#[test]
fn avst_v1_migra_sem_modo_e_limpa_estado_da_sessao_de_destino() {
    let (mut bus, mut d, regs) = setup();
    configure(&mut bus, &mut d, &regs);
    let mut state = d.video().clone();
    let mut legacy = SnapshotWriter::new();
    legacy.section("AVST", 1);
    legacy.u32(1);
    legacy.bool(true);
    let bytes = legacy.finish();
    Snapshot::load(&mut state, &mut SnapshotReader::new(&bytes).unwrap()).unwrap();
    assert!(state.enabled());
    assert!(state.mode().is_none());
    let mut missing = SnapshotWriter::new();
    missing.section("AVST", 2);
    missing.u32(1);
    missing.bool(false);
    missing.bool(true);
    assert!(
        Snapshot::load(
            &mut VideoState::default(),
            &mut SnapshotReader::new(&missing.finish()).unwrap()
        )
        .is_err()
    );
}
#[test]
fn configurar_modo_preserva_blanking_e_reconfiguracao_troca_buffer() {
    let (mut bus, mut d, regs) = setup();
    call(&mut bus, &mut d, &regs, 2, &[0xFD000000, 9, 0, 0]).unwrap();
    configure(&mut bus, &mut d, &regs);
    assert!(d.framebuffer(&bus).unwrap().unwrap().enabled());
    let mut a = args();
    a[5] += 0x1000;
    assert_eq!(call(&mut bus, &mut d, &regs, 3, &a).unwrap(), 0);
    assert_eq!(d.video().mode().unwrap().framebuffer(), GuestPhys(0x21000));
    call(&mut bus, &mut d, &regs, 2, &[0xFD000000, 9, 1, 0]).unwrap();
    assert!(!d.framebuffer(&bus).unwrap().unwrap().enabled());
}
