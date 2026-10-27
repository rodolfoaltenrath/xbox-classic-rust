use xbox_core::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_formats::fixtures::xbe_com_imports;
use xbox_kernel::{
    dispatch::{Dispatcher, Erro},
    imports,
    loader::{Image, STACK_BASE, STACK_SIZE},
};
use xbox_mem::{Bus, GuestPhys, GuestVirt, Perm};
const ATTR: u32 = 0x12100;
const STR: u32 = 0x12120;
const IO: u32 = 0x12130;
const HANDLE: u32 = 0x12140;
const MASK: u32 = 0x12150;
const NAME: u32 = 0x12200;
const OUT: u32 = 0x12400;
fn setup() -> (Bus, Dispatcher, [u32; 8], Vec<u8>) {
    let image = xbe_com_imports(&[0xc3], &[202, 207, 187, 190, 67, 192]).unwrap();
    let b = Image::prepare_aot(&image).unwrap().image.bus;
    let mut d = Dispatcher::new();
    d.mount_executable_media(&image).unwrap();
    let mut r = [0; 8];
    r[4] = STACK_BASE + STACK_SIZE - 0x100;
    (b, d, r, image)
}
fn call(
    b: &mut Bus,
    d: &mut Dispatcher,
    r: &[u32; 8],
    ord: u32,
    args: &[u32],
) -> Result<u32, Erro> {
    b.escrever_u32(GuestVirt(r[4]), 0x11000).unwrap();
    for (i, &v) in args.iter().enumerate() {
        b.escrever_u32(GuestVirt(r[4] + 4 + i as u32 * 4), v)
            .unwrap();
    }
    d.atender(b, imports::trampoline(ord), r).map(|a| a.eax)
}
fn string(b: &mut Bus, at: u32, ptr: u32, value: &[u8]) {
    b.escrever_u16(GuestVirt(at), value.len() as u16).unwrap();
    b.escrever_u16(GuestVirt(at + 2), value.len() as u16)
        .unwrap();
    b.escrever_u32(GuestVirt(at + 4), ptr).unwrap();
    b.escrever_bytes(GuestVirt(ptr), value).unwrap();
}
fn path(b: &mut Bus, p: &[u8], root: u32, flags: u32) {
    string(b, STR, NAME, p);
    for (i, v) in [root, STR, flags].into_iter().enumerate() {
        b.escrever_u32(GuestVirt(ATTR + i as u32 * 4), v).unwrap();
    }
}
fn args() -> [u32; 6] {
    [HANDLE, 0x100001, ATTR, IO, 1, 0x21]
}
fn open(b: &mut Bus, d: &mut Dispatcher, r: &[u32; 8]) -> u32 {
    path(b, b"\\Device\\CdRom0\\", 0, 0x40);
    assert_eq!(call(b, d, r, 202, &args()).unwrap(), 0);
    word(b, HANDLE)
}
fn word(b: &Bus, at: u32) -> u32 {
    b.ler_u32(GuestVirt(at)).unwrap()
}
fn query(
    b: &mut Bus,
    d: &mut Dispatcher,
    r: &[u32; 8],
    h: u32,
    mask: u32,
    restart: u32,
) -> Result<u32, Erro> {
    call(b, d, r, 207, &[h, 0, 0, 0, IO, OUT, 128, 1, mask, restart])
}
fn saved(d: &Dispatcher) -> Vec<u8> {
    let mut w = SnapshotWriter::new();
    d.save(&mut w);
    w.finish()
}
fn load(d: &mut Dispatcher, bytes: &[u8]) {
    d.load(&mut SnapshotReader::new(bytes).unwrap()).unwrap();
}

#[test]
fn enumera_nome_ansi_tamanho_real_cauda_preservada_e_fim_sem_repetir() {
    let (mut b, mut d, r, image) = setup();
    let h = open(&mut b, &mut d, &r);
    b.escrever_bytes(GuestVirt(OUT), &[0xa5; 128]).unwrap();
    assert_eq!(query(&mut b, &mut d, &r, h, 0, 0).unwrap(), 0);
    assert_eq!(word(&b, IO + 4), 75);
    assert_eq!(word(&b, OUT), 0);
    assert_eq!(word(&b, OUT + 40), image.len() as u32);
    assert_eq!(word(&b, OUT + 56), 1);
    assert_eq!(word(&b, OUT + 60), 11);
    let mut name = [0; 12];
    b.ler_bytes(GuestVirt(OUT + 64), &mut name).unwrap();
    assert_eq!(&name[..11], b"default.xbe");
    assert_eq!(name[11], 0xa5);
    let mut before = [0; 128];
    b.ler_bytes(GuestVirt(OUT), &mut before).unwrap();
    assert_eq!(query(&mut b, &mut d, &r, h, 0, 0).unwrap(), 0x80000006);
    assert_eq!(word(&b, IO + 4), 0);
    let mut after = [0; 128];
    b.ler_bytes(GuestVirt(OUT), &mut after).unwrap();
    assert_eq!(before, after);
    assert_eq!(call(&mut b, &mut d, &r, 187, &[h]).unwrap(), 0);
    assert_eq!(query(&mut b, &mut d, &r, h, 0, 0).unwrap(), 0xc0000008);
}
#[test]
fn filtros_por_handle_restart_preserva_filtro_inicial_e_nunca_le_ptr_posterior() {
    let (mut b, mut d, r, _) = setup();
    let h = open(&mut b, &mut d, &r);
    let other = open(&mut b, &mut d, &r);
    string(&mut b, MASK, NAME + 128, b"D?FAULT.*");
    assert_eq!(query(&mut b, &mut d, &r, h, MASK, 0).unwrap(), 0);
    assert_eq!(
        query(&mut b, &mut d, &r, h, 0xdeadbeef, 0).unwrap(),
        0x80000006
    );
    assert_eq!(query(&mut b, &mut d, &r, h, 0xdeadbeef, 1).unwrap(), 0);
    string(&mut b, MASK, NAME + 128, b"missing.*");
    assert_eq!(
        query(&mut b, &mut d, &r, other, MASK, 1).unwrap(),
        0xc000000f
    );
    assert_eq!(query(&mut b, &mut d, &r, other, 0, 0).unwrap(), 0x80000006);
    assert_eq!(query(&mut b, &mut d, &r, h, 0, 0).unwrap(), 0x80000006);
}
#[test]
fn opens_validam_namespace_share_e_alocador_global_sem_inventar_hd() {
    let (mut b, mut d, r, _) = setup();
    let h = open(&mut b, &mut d, &r);
    assert_eq!(h, 4);
    let mut arguments = args();
    arguments[4] = 0;
    assert_eq!(
        call(&mut b, &mut d, &r, 202, &arguments).unwrap(),
        0xc0000043
    );
    assert_eq!(call(&mut b, &mut d, &r, 187, &[h]).unwrap(), 0);
    path(&mut b, b"\\Device\\Harddisk0\\Partition2\\", 0, 0x40);
    assert!(matches!(
        call(&mut b, &mut d, &r, 202, &args()),
        Err(Erro::VarianteNaoImplementada { .. })
    ));
    path(&mut b, b"D:\\", 0xfffffffd, 0x40);
    assert_eq!(call(&mut b, &mut d, &r, 202, &args()).unwrap(), 0xc0000034);
    path(&mut b, b"\\device\\cdrom0", 0, 0x40);
    assert_eq!(call(&mut b, &mut d, &r, 202, &args()).unwrap(), 0);
    assert_eq!(word(&b, HANDLE), 8);
    path(&mut b, b"\\device\\cdrom0\\", 0, 0);
    assert_eq!(call(&mut b, &mut d, &r, 202, &args()).unwrap(), 0xc0000034);
    path(&mut b, b"\\Device\\CdRom0\\..\\", 0, 0x40);
    assert!(call(&mut b, &mut d, &r, 202, &args()).is_err());
}
#[test]
fn preflight_e_alias_fisico_nao_consumem_cursor_nem_publicam_buffer() {
    let (mut b, mut d, r, _) = setup();
    let h = open(&mut b, &mut d, &r);
    let before = saved(&d);
    for (length, buf) in [(128, 0xdead0000), (4096, 0x12ff0)] {
        assert_eq!(
            call(
                &mut b,
                &mut d,
                &r,
                207,
                &[h, 0, 0, 0, IO, buf, length, 1, 0, 1]
            )
            .unwrap(),
            0xc0000005
        );
        assert_eq!(&saved(&d)[36..], &before[36..]);
    }
    // Mapeamento de saída para o mesmo frame do IOSB por outro endereço virtual.
    let pa = b
        .traduzir(GuestVirt(0x12000), xbox_mem::Acesso::Leitura)
        .unwrap();
    b.mapear(GuestVirt(0x50000000), GuestPhys(pa.0), 1, Perm::RW)
        .unwrap();
    assert!(
        call(
            &mut b,
            &mut d,
            &r,
            207,
            &[h, 0, 0, 0, IO, 0x50000100, 128, 1, 0, 1]
        )
        .is_err()
    );
    assert_eq!(&saved(&d)[36..], &before[36..]);
    assert_eq!(
        query(&mut b, &mut d, &r, h, 0xdeadbeef, 1).unwrap(),
        0xc0000005
    );
    assert_eq!(&saved(&d)[36..], &before[36..]);
    for (index, value) in [(1, 1), (7, 2), (9, 2), (6, 74)] {
        let mut arguments = [h, 0, 0, 0, IO, OUT, 128, 1, 0, 1];
        arguments[index] = value;
        assert!(call(&mut b, &mut d, &r, 207, &arguments).is_err());
        assert_eq!(&saved(&d)[36..], &before[36..]);
    }
    assert_eq!(query(&mut b, &mut d, &r, h, 0, 0).unwrap(), 0);
}
#[test]
fn snapshot_replay_entre_entrada_e_fim_e_corrupcao_dos_diretorios() {
    let (mut b, mut d, r, _) = setup();
    let h = open(&mut b, &mut d, &r);
    assert_eq!(query(&mut b, &mut d, &r, h, 0, 0).unwrap(), 0);
    let state = saved(&d);
    assert_eq!(query(&mut b, &mut d, &r, h, 0, 0).unwrap(), 0x80000006);
    let expected = saved(&d);
    load(&mut d, &state);
    assert_eq!(query(&mut b, &mut d, &r, h, 0, 0).unwrap(), 0x80000006);
    assert_eq!(saved(&d), expected);
    let at = state.windows(4).rposition(|v| v == b"DIRE").unwrap();
    for (off, value) in [
        (4, 2),
        (8, 2),
        (9, 255),
        (13, 0),
        (17, 8),
        (21, 2),
        (22, 0),
        (23, 0),
    ] {
        let mut bad = state.clone();
        bad[at + off] = value;
        assert!(
            Dispatcher::new()
                .load(&mut SnapshotReader::new(&bad).unwrap())
                .is_err(),
            "accepted offset {off}"
        );
    }
}
#[test]
fn cdfs_v1_migra_com_midia_sem_ativar_diretorio_e_regrava_sem_promocao() {
    let (mut b, mut d, r, image) = setup();
    let mut old = saved(&d);
    let at = old.windows(4).rposition(|v| v == b"DIRE").unwrap();
    old.drain(at..at + 13);
    let cdfs = old.windows(4).rposition(|v| v == b"CDFS").unwrap();
    old[cdfs + 4..cdfs + 8].copy_from_slice(&1u32.to_le_bytes());
    load(&mut d, &old);
    assert_eq!(d.executable_media(), Some(image.as_slice()));
    path(&mut b, b"\\Device\\CdRom0\\", 0, 0x40);
    assert!(call(&mut b, &mut d, &r, 202, &args()).is_err());
    let again = saved(&d);
    load(&mut d, &again);
    assert!(call(&mut b, &mut d, &r, 202, &args()).is_err());
}

#[test]
fn falha_na_abertura_nao_reserva_handle_e_namespace_d_usa_link_declarado() {
    let (mut b, mut d, r, _) = setup();
    path(&mut b, b"\\Device\\CdRom0\\", 0, 0x40);
    let before = saved(&d);
    let mut arguments = args();
    arguments[0] = 0xdead0000;
    assert_eq!(
        call(&mut b, &mut d, &r, 202, &arguments).unwrap(),
        0xc0000005
    );
    assert_eq!(&saved(&d)[36..], &before[36..]);
    arguments[0] = IO;
    assert!(call(&mut b, &mut d, &r, 202, &arguments).is_err());
    assert_eq!(&saved(&d)[36..], &before[36..]);
    string(&mut b, STR, NAME, b"\\??\\D:");
    string(&mut b, MASK, NAME + 128, b"\\Device\\CdRom0\\");
    assert_eq!(call(&mut b, &mut d, &r, 67, &[STR, MASK]).unwrap(), 0);
    path(&mut b, b"D:\\", 0xfffffffd, 0x40);
    assert_eq!(call(&mut b, &mut d, &r, 202, &args()).unwrap(), 0);
    assert_eq!(word(&b, HANDLE), 4);
    let first = word(&b, HANDLE);
    for _ in 1..256 {
        assert_eq!(call(&mut b, &mut d, &r, 202, &args()).unwrap(), 0);
    }
    let before = saved(&d);
    assert_eq!(call(&mut b, &mut d, &r, 202, &args()).unwrap(), 0xc000009a);
    assert_eq!(&saved(&d)[36..], &before[36..]);
    assert_eq!(call(&mut b, &mut d, &r, 187, &[first]).unwrap(), 0);
    assert_eq!(call(&mut b, &mut d, &r, 202, &args()).unwrap(), 0);
    assert_eq!(word(&b, HANDLE), 1028);
}

#[test]
fn snapshot_recusa_handle_colidindo_com_arquivo_e_filtro_invalido() {
    let (mut b, mut d, r, _) = setup();
    let dir = open(&mut b, &mut d, &r);
    assert_eq!(query(&mut b, &mut d, &r, dir, 0, 0).unwrap(), 0);
    path(&mut b, b"\\Device\\CdRom0\\default.xbe", 0, 0x40);
    assert_eq!(
        call(
            &mut b,
            &mut d,
            &r,
            190,
            &[HANDLE, 0x80100080, ATTR, IO, 0, 0x80, 1, 1, 0x60]
        )
        .unwrap(),
        0
    );
    let file = word(&b, HANDLE);
    let state = saved(&d);
    let at = state.windows(4).rposition(|v| v == b"DIRE").unwrap();
    let mut bad = state.clone();
    bad[at + 13..at + 17].copy_from_slice(&file.to_le_bytes());
    assert!(
        Dispatcher::new()
            .load(&mut SnapshotReader::new(&bad).unwrap())
            .is_err()
    );
    let mut bad = state.clone();
    bad[at + 25] = b'\\';
    assert!(
        Dispatcher::new()
            .load(&mut SnapshotReader::new(&bad).unwrap())
            .is_err()
    );
    assert_eq!(
        call(
            &mut b,
            &mut d,
            &r,
            207,
            &[file, 0, 0, 0, IO, OUT, 128, 1, 0, 1]
        )
        .unwrap(),
        0xc0000008
    );
}
