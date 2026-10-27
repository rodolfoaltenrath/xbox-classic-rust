//! `HalWriteSMBusValue` para o LED do SMC (ADR-0090).
use xbox_core::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_formats::fixtures::xbe_com_imports;
use xbox_kernel::{
    dispatch::{Dispatcher, Erro, Smbus},
    imports,
    loader::{Image, STACK_BASE, STACK_SIZE},
};
use xbox_mem::{Bus, GuestVirt};

const HAL_WRITE_SMBUS_VALUE: u32 = 50;

fn setup() -> (Bus, Dispatcher, [u32; 8]) {
    let image = xbe_com_imports(&[0xC3], &[HAL_WRITE_SMBUS_VALUE]).unwrap();
    let mut regs = [0; 8];
    regs[4] = STACK_BASE + STACK_SIZE - 0x100;
    (
        Image::prepare_aot(&image).unwrap().image.bus,
        Dispatcher::new(),
        regs,
    )
}

fn escrever(
    bus: &mut Bus,
    d: &mut Dispatcher,
    regs: &[u32; 8],
    args: [u32; 4],
) -> Result<u32, Erro> {
    for (i, value) in std::iter::once(0x11000).chain(args).enumerate() {
        bus.escrever_u32(GuestVirt(regs[4] + i as u32 * 4), value)
            .unwrap();
    }
    d.atender(bus, imports::trampoline(HAL_WRITE_SMBUS_VALUE), regs)
        .map(|r| r.eax)
}

fn saved(value: &impl Snapshot) -> Vec<u8> {
    let mut out = SnapshotWriter::new();
    value.save(&mut out);
    out.finish()
}

#[test]
fn sample_led_programa_modo_manual_e_sequencia() {
    let (mut bus, mut d, r) = setup();
    assert_eq!(d.smbus(), &Smbus::default());
    // XSetCustomLED(XLED_GREEN, XLED_RED, XLED_OFF, XLED_OFF), como em lib/hal/led.c.
    assert_eq!(
        escrever(&mut bus, &mut d, &r, [0x20, 0x07, 0, 1]).unwrap(),
        0
    );
    assert_eq!(
        escrever(&mut bus, &mut d, &r, [0x20, 0x08, 0, 0x48]).unwrap(),
        0
    );
    assert_eq!(
        (
            d.smbus().modo(),
            d.smbus().sequencia(),
            d.smbus().escritas()
        ),
        (Some(1), Some(0x48), 2)
    );
    assert_eq!(
        Smbus::descrever_sequencia(0x48),
        "verde,vermelho,apagado,apagado"
    );
    // XResetLED volta ao modo automático e preserva a última sequência.
    assert_eq!(
        escrever(&mut bus, &mut d, &r, [0x20, 0x07, 0, 0]).unwrap(),
        0
    );
    assert_eq!(d.smbus().modo(), Some(0));
    assert_eq!(d.atendidas(), 3);
}

#[test]
fn variantes_sem_modelo_recusam_sem_mudar_estado() {
    let (mut bus, mut d, r) = setup();
    escrever(&mut bus, &mut d, &r, [0x20, 0x07, 0, 1]).unwrap();
    let antes = *d.smbus();
    for args in [
        [0x8A, 0x07, 0, 1],     // encoder de vídeo
        [0x20, 0x07, 1, 1],     // escrita de palavra
        [0x20, 0x0C, 0, 1],     // outro comando do SMC
        [0x20, 0x07, 0, 2],     // modo inexistente
        [0x20, 0x08, 0, 0x100], // sequência maior que um byte
    ] {
        let erro = escrever(&mut bus, &mut d, &r, args).unwrap_err();
        assert!(
            matches!(erro, Erro::VarianteNaoImplementada { ordinal: 50, .. }),
            "{args:?}: {erro}"
        );
        assert_eq!(d.smbus(), &antes, "{args:?}");
    }
    assert_eq!(d.recusadas(), 5);
}

#[test]
fn snapshot_guarda_o_led_e_krnl_v14_migra_sem_estado() {
    let (mut bus, mut d, r) = setup();
    escrever(&mut bus, &mut d, &r, [0x20, 0x07, 0, 1]).unwrap();
    escrever(&mut bus, &mut d, &r, [0x20, 0x08, 0, 0x48]).unwrap();
    let atual = saved(&d);
    let mut restaurado = Dispatcher::new();
    Snapshot::load(&mut restaurado, &mut SnapshotReader::new(&atual).unwrap()).unwrap();
    assert_eq!(restaurado.smbus(), d.smbus());
    assert_eq!(saved(&restaurado), atual);
    // KRNL v14: sem a seção SMBC no fim.
    let at = atual.windows(4).rposition(|v| v == b"SMBC").unwrap();
    let mut antigo = atual[..at].to_vec();
    let kernel = antigo.windows(4).position(|v| v == b"KRNL").unwrap();
    antigo[kernel + 4..kernel + 8].copy_from_slice(&14u32.to_le_bytes());
    let mut migrado = Dispatcher::new();
    Snapshot::load(&mut migrado, &mut SnapshotReader::new(&antigo).unwrap()).unwrap();
    assert_eq!(migrado.smbus(), &Smbus::default());
    // Estado incoerente é recusado: modo presente com contador zerado.
    let mut ruim = atual.clone();
    let fim = ruim.len();
    ruim[fim - 8..].copy_from_slice(&0u64.to_le_bytes());
    assert!(
        Snapshot::load(
            &mut Dispatcher::new(),
            &mut SnapshotReader::new(&ruim).unwrap()
        )
        .is_err()
    );
}
