//! `NtSetInformationFile` (posição), `NtWriteFile` (somente leitura) e
//! `NtQueryFullAttributesFile` sobre a mídia de executável (ADR-0102, P30: valores provisórios).
use xbox_core::{Snapshot, SnapshotWriter};
use xbox_formats::fixtures::xbe_com_imports;
use xbox_kernel::{
    dispatch::{Dispatcher, Erro, STATUS_ACCESS_VIOLATION, STATUS_INVALID_HANDLE},
    imports,
    loader::{Image, STACK_BASE, STACK_SIZE},
};
use xbox_mem::{Bus, GuestVirt};

const ATTR: u32 = 0x12100;
const STR: u32 = 0x12120;
const NAME: u32 = 0x12200;
const HANDLE: u32 = 0x12140;
const IO: u32 = 0x12150;
const OUT: u32 = 0x12400;
const INFO: u32 = 0x12500;
const ACCESS_DENIED: u32 = 0xC0000022;
const INVALID_PARAMETER: u32 = 0xC000000D;
const NOT_FOUND: u32 = 0xC0000034;

fn setup() -> (Bus, Dispatcher, [u32; 8], usize) {
    let image = xbe_com_imports(&[0xC3], &[190, 219, 211, 187, 210, 226, 236]).unwrap();
    let bus = Image::prepare_aot(&image).unwrap().image.bus;
    let mut d = Dispatcher::new();
    d.mount_executable_media(&image).unwrap();
    let mut regs = [0; 8];
    regs[4] = STACK_BASE + STACK_SIZE - 0x100;
    (bus, d, regs, image.len())
}
fn call(
    bus: &mut Bus,
    d: &mut Dispatcher,
    regs: &[u32; 8],
    ordinal: u32,
    args: &[u32],
) -> Result<u32, Erro> {
    bus.escrever_u32(GuestVirt(regs[4]), 0x11000).unwrap();
    for (i, &v) in args.iter().enumerate() {
        bus.escrever_u32(GuestVirt(regs[4] + 4 + i as u32 * 4), v)
            .unwrap();
    }
    d.atender(bus, imports::trampoline(ordinal), regs)
        .map(|r| r.eax)
}
fn word(bus: &Bus, at: u32) -> u32 {
    bus.ler_u32(GuestVirt(at)).unwrap()
}
fn path(bus: &mut Bus, text: &[u8], flags: u32) {
    bus.escrever_u16(GuestVirt(STR), text.len() as u16).unwrap();
    bus.escrever_u16(GuestVirt(STR + 2), text.len() as u16)
        .unwrap();
    bus.escrever_u32(GuestVirt(STR + 4), NAME).unwrap();
    bus.escrever_bytes(GuestVirt(NAME), text).unwrap();
    for (i, v) in [0, STR, flags].into_iter().enumerate() {
        bus.escrever_u32(GuestVirt(ATTR + i as u32 * 4), v).unwrap();
    }
}
fn open(bus: &mut Bus, d: &mut Dispatcher, regs: &[u32; 8]) -> u32 {
    path(bus, b"\\Device\\CdRom0\\default.xbe", 0x40);
    let args = [HANDLE, 0x80100080, ATTR, IO, 0, 0x80, 1, 1, 0x60];
    assert_eq!(call(bus, d, regs, 190, &args).unwrap(), 0);
    word(bus, HANDLE)
}
fn set_position(
    bus: &mut Bus,
    d: &mut Dispatcher,
    regs: &[u32; 8],
    handle: u32,
    value: i64,
) -> u32 {
    bus.escrever_bytes(GuestVirt(INFO), &value.to_le_bytes())
        .unwrap();
    call(bus, d, regs, 226, &[handle, IO, INFO, 8, 14]).unwrap()
}
fn position(bus: &mut Bus, d: &mut Dispatcher, regs: &[u32; 8], handle: u32) -> u64 {
    assert_eq!(
        call(bus, d, regs, 211, &[handle, IO, OUT, 8, 14]).unwrap(),
        0
    );
    u64::from(word(bus, OUT)) | (u64::from(word(bus, OUT + 4)) << 32)
}
fn saved(d: &Dispatcher) -> Vec<u8> {
    let mut w = SnapshotWriter::new();
    d.save(&mut w);
    w.finish()
}

#[test]
fn posicao_definida_vale_para_a_proxima_leitura_e_nao_afeta_outro_handle() {
    let (mut b, mut d, r, len) = setup();
    let first = open(&mut b, &mut d, &r);
    let second = open(&mut b, &mut d, &r);
    assert_eq!(set_position(&mut b, &mut d, &r, first, 4), 0);
    assert_eq!((word(&b, IO), word(&b, IO + 4)), (0, 0));
    assert_eq!(position(&mut b, &mut d, &r, first), 4);
    assert_eq!(position(&mut b, &mut d, &r, second), 0);
    assert_eq!(
        call(&mut b, &mut d, &r, 219, &[first, 0, 0, 0, IO, OUT, 4, 0]).unwrap(),
        0
    );
    let image = d.executable_media().unwrap();
    assert_eq!(
        word(&b, OUT),
        u32::from_le_bytes(image[4..8].try_into().unwrap())
    );
    // Depois do fim: a posição é guardada e a leitura devolve fim de arquivo.
    assert_eq!(
        set_position(&mut b, &mut d, &r, first, i64::try_from(len).unwrap() + 10),
        0
    );
    assert_eq!(
        call(&mut b, &mut d, &r, 219, &[first, 0, 0, 0, IO, OUT, 4, 0]).unwrap(),
        0xC0000011
    );
}

#[test]
fn posicao_invalida_e_recusada_sem_alterar_o_estado() {
    let (mut b, mut d, r, _) = setup();
    let h = open(&mut b, &mut d, &r);
    assert_eq!(set_position(&mut b, &mut d, &r, h, 7), 0);
    let before = saved(&d);
    assert_eq!(
        set_position(&mut b, &mut d, &r, h, -1),
        INVALID_PARAMETER,
        "posição negativa"
    );
    assert_eq!(word(&b, IO), INVALID_PARAMETER);
    assert_eq!(
        call(&mut b, &mut d, &r, 226, &[h, IO, INFO, 4, 14]).unwrap(),
        0xC0000004,
        "tamanho diferente de 8"
    );
    assert_eq!(
        call(&mut b, &mut d, &r, 226, &[99, IO, INFO, 8, 14]).unwrap(),
        STATUS_INVALID_HANDLE
    );
    assert_eq!(
        call(&mut b, &mut d, &r, 226, &[h, 0x10, INFO, 8, 14]).unwrap(),
        STATUS_ACCESS_VIOLATION
    );
    assert!(matches!(
        call(&mut b, &mut d, &r, 226, &[h, IO, INFO, 8, 20]),
        Err(Erro::VarianteNaoImplementada { .. })
    ));
    assert_eq!(saved(&d)[36..], before[36..]);
    assert_eq!(position(&mut b, &mut d, &r, h), 7);
}

#[test]
fn escrita_em_midia_somente_leitura_e_acesso_negado_sem_efeito() {
    let (mut b, mut d, r, _) = setup();
    let h = open(&mut b, &mut d, &r);
    b.escrever_u32(GuestVirt(OUT), 0xAABBCCDD).unwrap();
    let before = saved(&d);
    assert_eq!(
        call(&mut b, &mut d, &r, 236, &[h, 0, 0, 0, IO, OUT, 4, 0]).unwrap(),
        ACCESS_DENIED
    );
    assert_eq!((word(&b, IO), word(&b, IO + 4)), (ACCESS_DENIED, 0));
    assert_eq!(
        saved(&d)[36..],
        before[36..],
        "a mídia e as posições não mudam"
    );
    assert_eq!(
        call(&mut b, &mut d, &r, 236, &[99, 0, 0, 0, IO, OUT, 4, 0]).unwrap(),
        STATUS_INVALID_HANDLE
    );
    assert!(matches!(
        call(&mut b, &mut d, &r, 236, &[h, 4, 0, 0, IO, OUT, 4, 0]),
        Err(Erro::VarianteNaoImplementada { .. })
    ));
    assert_eq!(
        call(&mut b, &mut d, &r, 236, &[h, 0, 0, 0, 0x10, OUT, 4, 0]).unwrap(),
        STATUS_ACCESS_VIOLATION
    );
}

#[test]
fn atributos_completos_batem_com_a_classe_34_e_o_nome_ausente_nao_escreve() {
    let (mut b, mut d, r, len) = setup();
    let h = open(&mut b, &mut d, &r);
    assert_eq!(
        call(&mut b, &mut d, &r, 211, &[h, IO, OUT, 56, 34]).unwrap(),
        0
    );
    let mut by_handle = [0u8; 56];
    b.ler_bytes(GuestVirt(OUT), &mut by_handle).unwrap();

    b.escrever_bytes(GuestVirt(INFO), &[0xEE; 56]).unwrap();
    path(&mut b, b"\\Device\\CdRom0\\default.xbe", 0x40);
    assert_eq!(call(&mut b, &mut d, &r, 210, &[ATTR, INFO]).unwrap(), 0);
    let mut by_name = [0u8; 56];
    b.ler_bytes(GuestVirt(INFO), &mut by_name).unwrap();
    assert_eq!(by_name, by_handle);
    assert_eq!(
        u64::from_le_bytes(by_name[40..48].try_into().unwrap()),
        len as u64
    );

    b.escrever_bytes(GuestVirt(INFO), &[0xEE; 56]).unwrap();
    path(&mut b, b"\\Device\\CdRom0\\outro.xbe", 0x40);
    assert_eq!(
        call(&mut b, &mut d, &r, 210, &[ATTR, INFO]).unwrap(),
        NOT_FOUND
    );
    let mut untouched = [0u8; 56];
    b.ler_bytes(GuestVirt(INFO), &mut untouched).unwrap();
    assert_eq!(untouched, [0xEE; 56]);
    assert_eq!(
        call(&mut b, &mut d, &r, 210, &[ATTR, 0x10]).unwrap(),
        STATUS_ACCESS_VIOLATION
    );
}
