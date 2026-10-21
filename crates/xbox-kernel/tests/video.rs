use xbox_core::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_formats::{
    eeprom::{Eeprom, Idioma, PadraoDeVideo},
    fixtures::xbe_com_imports,
};
use xbox_kernel::{
    dispatch::{Dispatcher, EepromState, Erro},
    imports,
    loader::{Image, STACK_BASE, STACK_SIZE},
};
use xbox_mem::{Bus, GuestPhys, GuestVirt, Perm};
const OUTPUT: u32 = 0x12101;
fn setup() -> (Bus, Dispatcher, [u32; 8]) {
    let bytes = xbe_com_imports(&[0xC3], &[2, 1]).unwrap();
    let bus = Image::prepare_aot(&bytes).unwrap().image.bus;
    let mut regs = [0; 8];
    regs[4] = STACK_BASE + STACK_SIZE - 0x100;
    (bus, Dispatcher::new(), regs)
}
fn call(bus: &mut Bus, d: &mut Dispatcher, regs: &[u32; 8], args: [u32; 4]) -> Result<u32, Erro> {
    bus.escrever_u32(GuestVirt(regs[4]), 0x11000).unwrap();
    for (i, arg) in args.into_iter().enumerate() {
        bus.escrever_u32(GuestVirt(regs[4] + 4 + i as u32 * 4), arg)
            .unwrap();
    }
    d.atender(bus, imports::trampoline(2), regs).map(|r| r.eax)
}
fn args() -> [u32; 4] {
    [0xFD000000, 6, 0, OUTPUT]
}
fn saved(value: &impl Snapshot) -> Vec<u8> {
    let mut out = SnapshotWriter::new();
    value.save(&mut out);
    out.finish()
}
#[test]
fn consulta_default_eeprom_cabo_padrao_escreve_long_desalinhado_com_guardas() {
    let (mut bus, mut d, regs) = setup();
    bus.escrever_bytes(GuestVirt(OUTPUT - 1), &[0xAA; 6])
        .unwrap();
    assert_eq!(call(&mut bus, &mut d, &regs, args()).unwrap(), 0);
    assert_eq!(bus.ler_u32(GuestVirt(OUTPUT)).unwrap(), 0x00480101);
    assert_eq!(bus.ler_u8(GuestVirt(OUTPUT - 1)).unwrap(), 0xAA);
    assert_eq!(bus.ler_u8(GuestVirt(OUTPUT + 4)).unwrap(), 0xAA);
    assert_eq!(d.atendidas(), 1);
}
#[test]
fn padrao_e_flags_vem_dos_bytes_autoritativos_da_eeprom_da_sessao() {
    let (mut bus, _, regs) = setup();
    for (standard, flags, expected) in [
        (PadraoDeVideo::NtscM, 0, 0x00400101),
        (PadraoDeVideo::NtscJ, 0x00010000, 0x00410201),
        (PadraoDeVideo::PalI, 0x00030000, 0x00830301),
        (PadraoDeVideo::PalI, 0x00400000, 0x00C00301),
    ] {
        let mut eeprom = Eeprom::sintetica(standard, Idioma::Ingles);
        eeprom.flags_de_video.0 = flags;
        let mut d = Dispatcher::with_eeprom(EepromState::supplied(eeprom.bytes()));
        call(&mut bus, &mut d, &regs, args()).unwrap();
        assert_eq!(bus.ler_u32(GuestVirt(OUTPUT)).unwrap(), expected);
    }
}
#[test]
fn base_opcao_parametro_e_configuracao_sem_modelo_recusam_sem_escrever() {
    let (mut bus, mut d, regs) = setup();
    bus.escrever_u32(GuestVirt(OUTPUT), 0xDEADBEEF).unwrap();
    for (index, value) in [
        (0, 0),
        (0, 0xFE000000),
        (1, 11),
        (1, 14),
        (1, u32::MAX),
        (2, 1),
    ] {
        let mut a = args();
        a[index] = value;
        assert!(matches!(
            call(&mut bus, &mut d, &regs, a),
            Err(Erro::VarianteNaoImplementada { .. })
        ));
        assert_eq!(bus.ler_u32(GuestVirt(OUTPUT)).unwrap(), 0xDEADBEEF);
    }
    for (offset, value) in [(0x58, 0xDEADu32), (0x94, 1)] {
        let mut bytes = *EepromState::default().bytes();
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        let mut d = Dispatcher::with_eeprom(EepromState::supplied(bytes));
        assert!(call(&mut bus, &mut d, &regs, args()).is_err());
        assert_eq!(bus.ler_u32(GuestVirt(OUTPUT)).unwrap(), 0xDEADBEEF);
    }
}
#[test]
fn result_invalido_ro_mmio_wrap_ou_cruzando_pagina_nao_aplica_store_parcial() {
    let (mut bus, mut d, regs) = setup();
    bus.mapear(GuestVirt(0x50000000), GuestPhys(0xFD000000), 1, Perm::RW)
        .unwrap();
    bus.escrever_u16(GuestVirt(0x12FFE), 0xBEEF).unwrap();
    for output in [0, 0x11000, 0x12FFE, u32::MAX - 1, 0x60000000, 0x50000000] {
        let mut a = args();
        a[3] = output;
        assert!(matches!(
            call(&mut bus, &mut d, &regs, a),
            Err(Erro::ArgumentoInvalido { .. })
        ));
        assert_eq!(bus.ler_u16(GuestVirt(0x12FFE)).unwrap(), 0xBEEF);
    }
    let pa = bus.mmu().consultar(GuestVirt(0x12000)).unwrap().0;
    bus.mapear(GuestVirt(0x12000), pa, 1, Perm::RO).unwrap();
    assert!(call(&mut bus, &mut d, &regs, args()).is_err());
}
#[test]
fn snapshot_preserva_origem_dos_settings_sem_herdar_eeprom_do_destino() {
    let (mut bus, _, regs) = setup();
    let mut original =
        Dispatcher::with_eeprom(EepromState::synthetic(PadraoDeVideo::PalI, Idioma::Ingles));
    let checkpoint = saved(&original);
    let mut restored = Dispatcher::new();
    Snapshot::load(
        &mut restored,
        &mut SnapshotReader::new(&checkpoint).unwrap(),
    )
    .unwrap();
    assert_eq!(saved(&restored), checkpoint);
    call(&mut bus, &mut original, &regs, args()).unwrap();
    let expected = bus.ler_u32(GuestVirt(OUTPUT)).unwrap();
    call(&mut bus, &mut restored, &regs, args()).unwrap();
    assert_eq!(bus.ler_u32(GuestVirt(OUTPUT)).unwrap(), expected);
    assert_eq!(expected, 0x00880301);
}

#[test]
fn videoenable_param_e_blanking_e_variantes_recusadas_nao_alteram_estado() {
    let (mut bus, mut d, regs) = setup();
    assert!(!d.video().enabled());
    assert_eq!(
        call(&mut bus, &mut d, &regs, [0xFD000000, 9, 0, 0]).unwrap(),
        0
    );
    assert!(d.video().enabled());
    for args in [
        [0xFD000000, 9, 2, 0],
        [0xFD000000, 9, 1, OUTPUT],
        [0, 9, 1, 0],
    ] {
        assert!(call(&mut bus, &mut d, &regs, args).is_err());
        assert!(d.video().enabled());
    }
    call(&mut bus, &mut d, &regs, [0xFD000000, 9, 1, 0]).unwrap();
    assert!(!d.video().enabled());
}

#[test]
fn snapshot_video_preserva_blanking_migra_cold_boot_e_recusa_perfil_invalido() {
    let (mut bus, mut d, regs) = setup();
    call(&mut bus, &mut d, &regs, [0xFD000000, 9, 0, 0]).unwrap();
    let snapshot = saved(&d);
    let mut restored = Dispatcher::new();
    Snapshot::load(&mut restored, &mut SnapshotReader::new(&snapshot).unwrap()).unwrap();
    assert!(restored.video().enabled());
    assert_eq!(saved(&restored), snapshot);
    let av = snapshot.windows(4).rposition(|v| v == b"AVST").unwrap();
    let mut old = snapshot[..av].to_vec();
    let krnl = old.windows(4).position(|v| v == b"KRNL").unwrap();
    old[krnl + 4..krnl + 8].copy_from_slice(&8u32.to_le_bytes());
    Snapshot::load(&mut restored, &mut SnapshotReader::new(&old).unwrap()).unwrap();
    assert!(!restored.video().enabled());
    for offset in [av + 4, av + 8, av + 12] {
        let mut bad = snapshot.clone();
        bad[offset] = 0xFF;
        assert!(Snapshot::load(&mut restored, &mut SnapshotReader::new(&bad).unwrap()).is_err());
    }
    assert!(
        Snapshot::load(
            &mut restored,
            &mut SnapshotReader::new(&snapshot[..av + 12]).unwrap()
        )
        .is_err()
    );
}

#[test]
fn saved_data_ausente_no_cold_boot_nao_inventa_framebuffer_ou_toca_argumentos() {
    let (mut bus, mut d, regs) = setup();
    bus.escrever_u32(GuestVirt(regs[4]), 0x11000).unwrap();
    assert_eq!(
        d.atender(&mut bus, imports::trampoline(1), &regs)
            .unwrap()
            .eax,
        0
    );
    call(&mut bus, &mut d, &regs, [0xFD000000, 9, 0, 0]).unwrap();
    assert_eq!(
        d.atender(&mut bus, imports::trampoline(1), &regs)
            .unwrap()
            .eax,
        0
    );
    assert!(d.video().enabled());
}

#[test]
fn filtros_guardam_pedidos_independentes_sem_tocar_ram_ou_blanking() {
    let (mut bus, mut d, regs) = setup();
    assert_eq!(d.video().flicker_filter(), None);
    assert_eq!(d.video().soften_filter(), None);
    call(&mut bus, &mut d, &regs, [0xFD000000, 9, 0, 0]).unwrap();
    bus.escrever_u32(GuestVirt(OUTPUT), 0xDEADBEEF).unwrap();
    for level in 0..=7 {
        assert_eq!(
            call(&mut bus, &mut d, &regs, [0xFD000000, 11, level, 0]).unwrap(),
            0
        );
        assert_eq!(d.video().flicker_filter(), Some(level as u8));
        assert_eq!(d.video().soften_filter(), None);
    }
    for enabled in [0, 1, 1, 0] {
        call(&mut bus, &mut d, &regs, [0xFD000000, 14, enabled, 0]).unwrap();
        assert_eq!(d.video().soften_filter(), Some(enabled != 0));
        assert_eq!(d.video().flicker_filter(), Some(7));
    }
    assert!(d.video().enabled());
    assert!(d.video().mode().is_none());
    assert_eq!(bus.ler_u32(GuestVirt(OUTPUT)).unwrap(), 0xDEADBEEF);
}

#[test]
fn variantes_de_filtro_recusam_sem_truncar_parametro_ou_desreferenciar_result() {
    let (mut bus, mut d, regs) = setup();
    call(&mut bus, &mut d, &regs, [0xFD000000, 11, 5, 0]).unwrap();
    call(&mut bus, &mut d, &regs, [0xFD000000, 14, 1, 0]).unwrap();
    let video = d.video().clone();
    bus.escrever_u32(GuestVirt(OUTPUT), 0x12345678).unwrap();
    for args in [
        [0xFD000000, 11, 8, 0],
        [0xFD000000, 11, u32::MAX, 0],
        [0xFD000000, 14, 2, 0],
        [0xFD000000, 14, u32::MAX, 0],
        [0xFD000000, 11, 0, OUTPUT],
        [0xFD000000, 14, 0, 0x60000000],
        [0, 11, 0, 0],
        [0xFE000000, 14, 0, 0],
        [0xFD000000, 12, 0, 0],
    ] {
        assert!(matches!(
            call(&mut bus, &mut d, &regs, args),
            Err(Erro::VarianteNaoImplementada { .. })
        ));
        assert_eq!(d.video(), &video);
        assert_eq!(bus.ler_u32(GuestVirt(OUTPUT)).unwrap(), 0x12345678);
    }
}

#[test]
fn snapshot_de_filtros_preserva_ausencia_e_pedidos_e_carga_e_transacional() {
    let (mut bus, mut d, regs) = setup();
    for requests in [None, Some((0, 0)), Some((7, 1))] {
        if let Some((flicker, soften)) = requests {
            call(&mut bus, &mut d, &regs, [0xFD000000, 11, flicker, 0]).unwrap();
            call(&mut bus, &mut d, &regs, [0xFD000000, 14, soften, 0]).unwrap();
        }
        let checkpoint = saved(&d);
        let mut restored = Dispatcher::new();
        Snapshot::load(
            &mut restored,
            &mut SnapshotReader::new(&checkpoint).unwrap(),
        )
        .unwrap();
        assert_eq!(saved(&restored), checkpoint);
        assert_eq!(restored.video(), d.video());
    }
    let video = d.video().clone();
    let bytes = saved(&video);
    for (offset, value) in [(14, 8), (14, 0xFE), (15, 2), (15, 0xFE)] {
        let mut bad = bytes.clone();
        bad[offset] = value;
        let mut destination = video.clone();
        assert!(Snapshot::load(&mut destination, &mut SnapshotReader::new(&bad).unwrap()).is_err());
        assert_eq!(destination, video);
    }
    for len in [14, 15] {
        let mut destination = video.clone();
        assert!(
            Snapshot::load(
                &mut destination,
                &mut SnapshotReader::new(&bytes[..len]).unwrap()
            )
            .is_err()
        );
        assert_eq!(destination, video);
    }
}

#[test]
fn avst_v1_v2_nao_herdam_filtros_e_v2_preserva_modo_configurado() {
    let (mut bus, mut d, regs) = setup();
    call(&mut bus, &mut d, &regs, [0xFD000000, 11, 5, 0]).unwrap();
    call(&mut bus, &mut d, &regs, [0xFD000000, 14, 1, 0]).unwrap();
    for version in [1, 2] {
        let mut state = d.video().clone();
        let mut out = SnapshotWriter::new();
        out.section("AVST", version);
        out.u32(1);
        out.bool(true);
        if version == 2 {
            out.bool(true);
            for field in [0x04010101, 0x12, 2560, 0x50000] {
                out.u32(field);
            }
        }
        Snapshot::load(&mut state, &mut SnapshotReader::new(&out.finish()).unwrap()).unwrap();
        assert!(state.enabled());
        assert_eq!(state.mode().is_some(), version == 2);
        assert_eq!(state.flicker_filter(), None);
        assert_eq!(state.soften_filter(), None);
    }
}
