use xbox_core::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_formats::fixtures::xbe_com_imports;
use xbox_kernel::{
    dispatch::{
        Dispatcher, Erro, Irql, MAX_LINK_HANDLES, STATUS_ACCESS_VIOLATION,
        STATUS_INSUFFICIENT_RESOURCES, STATUS_INVALID_HANDLE, STATUS_OBJECT_NAME_COLLISION,
        STATUS_OBJECT_NAME_NOT_FOUND,
    },
    imports,
    loader::{Image, STACK_BASE, STACK_SIZE},
};
use xbox_mem::{Bus, GuestVirt};
fn setup() -> (Bus, Dispatcher, [u32; 8]) {
    let bytes = xbe_com_imports(&[0xC3], &[67, 203, 187, 192, 255, 233]).unwrap();
    let mut bus = Image::prepare_aot(&bytes).unwrap().image.bus;
    let mut regs = [0; 8];
    regs[4] = STACK_BASE + STACK_SIZE - 0x100;
    string(&mut bus, 0x12100, 0x12200, b"\\??\\D:");
    string(&mut bus, 0x12108, 0x12240, b"\\Device\\CdRom0\\");
    for (at, value) in [
        (0x12110, 0),
        (0x12114, 0x12100),
        (0x12118, 0x40),
        (0x12120, 0xDEADBEEF),
    ] {
        bus.escrever_u32(GuestVirt(at), value).unwrap();
    }
    (bus, Dispatcher::new(), regs)
}
fn string(bus: &mut Bus, at: u32, buffer: u32, bytes: &[u8]) {
    bus.escrever_u16(GuestVirt(at), bytes.len() as u16).unwrap();
    bus.escrever_u16(GuestVirt(at + 2), bytes.len() as u16)
        .unwrap();
    bus.escrever_u32(GuestVirt(at + 4), buffer).unwrap();
    bus.escrever_bytes(GuestVirt(buffer), bytes).unwrap();
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
fn mount(bus: &mut Bus, d: &mut Dispatcher, regs: &[u32; 8]) {
    assert_eq!(call(bus, d, regs, 67, &[0x12100, 0x12108]).unwrap(), 0);
}
fn open(bus: &mut Bus, d: &mut Dispatcher, regs: &[u32; 8]) -> u32 {
    assert_eq!(call(bus, d, regs, 203, &[0x12120, 0x12110]).unwrap(), 0);
    bus.ler_u32(GuestVirt(0x12120)).unwrap()
}
fn saved(value: &impl Snapshot) -> Vec<u8> {
    let mut out = SnapshotWriter::new();
    value.save(&mut out);
    out.finish()
}
#[test]
fn consulta_ausente_criacao_copia_destino_open_close_sem_desmontar() {
    let (mut bus, mut d, regs) = setup();
    let used = bus.frames().usados();
    assert_eq!(
        call(&mut bus, &mut d, &regs, 203, &[0x12120, 0x12110]).unwrap(),
        STATUS_OBJECT_NAME_NOT_FOUND
    );
    assert_eq!(bus.ler_u32(GuestVirt(0x12120)).unwrap(), 0xDEADBEEF);
    assert!(d.drive_d_target().is_none());
    mount(&mut bus, &mut d, &regs);
    assert_eq!(d.drive_d_target(), Some("\\Device\\CdRom0\\"));
    bus.escrever_bytes(GuestVirt(0x12240), b"XXXXXXXX").unwrap();
    assert_eq!(d.drive_d_target(), Some("\\Device\\CdRom0\\"));
    let handle = open(&mut bus, &mut d, &regs);
    assert_eq!(handle, 4);
    assert_eq!(call(&mut bus, &mut d, &regs, 187, &[handle]).unwrap(), 0);
    assert_eq!(
        call(&mut bus, &mut d, &regs, 187, &[handle]).unwrap(),
        STATUS_INVALID_HANDLE
    );
    assert!(d.drive_d_target().is_some());
    assert_eq!(open(&mut bus, &mut d, &regs), 8);
    assert_eq!(bus.frames().usados(), used);
}
#[test]
fn duplicata_nao_substitui_destino_e_busca_respeita_perfil_case_insensitive() {
    let (mut bus, mut d, regs) = setup();
    mount(&mut bus, &mut d, &regs);
    string(&mut bus, 0x12108, 0x12240, b"\\Device\\CdRom0\\other\\");
    assert_eq!(
        call(&mut bus, &mut d, &regs, 67, &[0x12100, 0x12108]).unwrap(),
        STATUS_OBJECT_NAME_COLLISION
    );
    assert_eq!(d.drive_d_target(), Some("\\Device\\CdRom0\\"));
    string(&mut bus, 0x12100, 0x12200, b"\\??\\d:");
    open(&mut bus, &mut d, &regs);
    bus.escrever_u32(GuestVirt(0x12118), 0).unwrap();
    assert!(matches!(
        call(&mut bus, &mut d, &regs, 203, &[0x12120, 0x12110]),
        Err(Erro::VarianteNaoImplementada { .. })
    ));
}
#[test]
fn saidas_invalidas_e_objetos_fora_do_perfil_nao_criam_handle() {
    let (mut bus, mut d, regs) = setup();
    mount(&mut bus, &mut d, &regs);
    for output in [0, 0x11000, 0x12FFE, u32::MAX - 1] {
        assert_eq!(
            call(&mut bus, &mut d, &regs, 203, &[output, 0x12110]).unwrap(),
            STATUS_ACCESS_VIOLATION
        );
    }
    for attributes in [0, 0x12FFC, 0x50000000] {
        assert_eq!(
            call(&mut bus, &mut d, &regs, 203, &[0x12120, attributes]).unwrap(),
            STATUS_ACCESS_VIOLATION
        );
    }
    for (at, value) in [(0x12110, 4), (0x12118, 0x140)] {
        bus.escrever_u32(GuestVirt(at), value).unwrap();
        assert!(call(&mut bus, &mut d, &regs, 203, &[0x12120, 0x12110]).is_err());
        bus.escrever_u32(GuestVirt(at), if at == 0x12118 { 0x40 } else { 0 })
            .unwrap();
    }
    string(&mut bus, 0x12100, 0x12200, b"\\??\\E:");
    assert!(call(&mut bus, &mut d, &regs, 203, &[0x12120, 0x12110]).is_err());
    string(&mut bus, 0x12100, 0x12200, b"\\??\\D:");
    assert_eq!(open(&mut bus, &mut d, &regs), 4);
}
#[test]
fn strings_nao_terminadas_funcionam_malformadas_e_alvos_sem_modelo_recusam() {
    let (mut bus, mut d, regs) = setup();
    for target in [
        b"\\Device\\Harddisk0\\Partition1".as_slice(),
        b"\\Device\\CdRom00",
        b"\\Device\\CdRom0\\..\\secret",
        b"C:\\host",
    ] {
        string(&mut bus, 0x12108, 0x12240, target);
        assert!(call(&mut bus, &mut d, &regs, 67, &[0x12100, 0x12108]).is_err());
        assert!(d.drive_d_target().is_none());
    }
    string(&mut bus, 0x12108, 0x12240, b"\\Device\\CdRom0\\");
    for (at, value) in [(0x12108, 0u16), (0x12108, 261), (0x1210A, 1)] {
        let old = bus.ler_u16(GuestVirt(at)).unwrap();
        bus.escrever_u16(GuestVirt(at), value).unwrap();
        assert!(call(&mut bus, &mut d, &regs, 67, &[0x12100, 0x12108]).is_err());
        bus.escrever_u16(GuestVirt(at), old).unwrap();
    }
    bus.escrever_u32(GuestVirt(0x1210C), 0x12FFF).unwrap();
    assert!(call(&mut bus, &mut d, &regs, 67, &[0x12100, 0x12108]).is_err());
    string(&mut bus, 0x12108, 0x12240, b"\\Device\\CdRom0\\");
    mount(&mut bus, &mut d, &regs);
}
#[test]
fn contador_compartilhado_tipo_close_limitado_e_limite_de_recursos() {
    let (mut bus, mut d, regs) = setup();
    mount(&mut bus, &mut d, &regs);
    call(&mut bus, &mut d, &regs, 192, &[0x12124, 0, 0]).unwrap();
    let mutant = bus.ler_u32(GuestVirt(0x12124)).unwrap();
    let handle = open(&mut bus, &mut d, &regs);
    assert_eq!(handle, mutant + 4);
    assert!(call(&mut bus, &mut d, &regs, 233, &[handle, 0, 0]).is_err());
    assert!(call(&mut bus, &mut d, &regs, 187, &[mutant]).is_err());
    assert!(d.mutant(mutant).is_some());
    for _ in 1..MAX_LINK_HANDLES {
        open(&mut bus, &mut d, &regs);
    }
    assert_eq!(
        call(&mut bus, &mut d, &regs, 203, &[0x12120, 0x12110]).unwrap(),
        STATUS_INSUFFICIENT_RESOURCES
    );
    assert_eq!(call(&mut bus, &mut d, &regs, 187, &[handle]).unwrap(), 0);
    assert_eq!(
        open(&mut bus, &mut d, &regs),
        handle + MAX_LINK_HANDLES as u32 * 4
    );
}
#[test]
fn snapshot_guarda_namespace_handles_e_migra_v7_sem_herdar_destino() {
    let (mut bus, mut d, regs) = setup();
    mount(&mut bus, &mut d, &regs);
    let handle = open(&mut bus, &mut d, &regs);
    let checkpoint = saved(&d);
    let mut restored = Dispatcher::new();
    Snapshot::load(
        &mut restored,
        &mut SnapshotReader::new(&checkpoint).unwrap(),
    )
    .unwrap();
    assert_eq!(saved(&restored), checkpoint);
    assert_eq!(
        call(&mut bus, &mut restored, &regs, 187, &[handle]).unwrap(),
        0
    );
    let at = checkpoint.windows(4).rposition(|v| v == b"SYML").unwrap();
    let mut old = checkpoint[..at].to_vec();
    let kernel = old.windows(4).position(|v| v == b"KRNL").unwrap();
    old[kernel + 4..kernel + 8].copy_from_slice(&7u32.to_le_bytes());
    Snapshot::load(&mut restored, &mut SnapshotReader::new(&old).unwrap()).unwrap();
    assert!(restored.drive_d_target().is_none());
    assert_eq!(
        call(&mut bus, &mut restored, &regs, 203, &[0x12120, 0x12110]).unwrap(),
        STATUS_OBJECT_NAME_NOT_FOUND
    );
    let mut bad = checkpoint;
    let last = bad.len() - 4;
    bad[last..].copy_from_slice(&0xFFFFFFFCu32.to_le_bytes());
    assert!(Snapshot::load(&mut restored, &mut SnapshotReader::new(&bad).unwrap()).is_err());
}
#[test]
fn saida_sobre_atributos_e_irql_sem_efeito_no_namespace() {
    let (mut bus, mut d, regs) = setup();
    mount(&mut bus, &mut d, &regs);
    assert_eq!(
        call(&mut bus, &mut d, &regs, 203, &[0x12114, 0x12110]).unwrap(),
        0
    );
    assert_eq!(bus.ler_u32(GuestVirt(0x12114)).unwrap(), 4);
    for level in [Irql::Apc, Irql::Dispatch] {
        let mut other = Dispatcher::with_irql(level);
        assert!(call(&mut bus, &mut other, &regs, 67, &[0x12100, 0x12108]).is_err());
        assert!(other.drive_d_target().is_none());
    }
}
