use xbox_core::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_formats::fixtures::xbe_com_imports;
use xbox_kernel::{
    dispatch::{
        Dispatcher, Erro, MAX_FILE_HANDLES, STATUS_ACCESS_VIOLATION, STATUS_INVALID_HANDLE,
    },
    imports,
    loader::{Image, STACK_BASE, STACK_SIZE},
};
use xbox_mem::{Acesso, Bus, GuestVirt, Perm};
const ATTR: u32 = 0x12100;
const STR: u32 = 0x12120;
const NAME: u32 = 0x12200;
const HANDLE: u32 = 0x12140;
const IO: u32 = 0x12150;
const OUT: u32 = 0x12400;
const OFFSET: u32 = 0x12180;
fn setup() -> (Bus, Dispatcher, [u32; 8], Vec<u8>) {
    let image = xbe_com_imports(&[0xC3], &[190, 219, 211, 187, 67, 192]).unwrap();
    let bus = Image::prepare_aot(&image).unwrap().image.bus;
    let mut d = Dispatcher::new();
    d.mount_executable_media(&image).unwrap();
    let mut regs = [0; 8];
    regs[4] = STACK_BASE + STACK_SIZE - 0x100;
    (bus, d, regs, image)
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
fn string(bus: &mut Bus, at: u32, buffer: u32, text: &[u8]) {
    bus.escrever_u16(GuestVirt(at), text.len() as u16).unwrap();
    bus.escrever_u16(GuestVirt(at + 2), text.len() as u16)
        .unwrap();
    bus.escrever_u32(GuestVirt(at + 4), buffer).unwrap();
    bus.escrever_bytes(GuestVirt(buffer), text).unwrap();
}
fn path(bus: &mut Bus, text: &[u8], root: u32, flags: u32) {
    string(bus, STR, NAME, text);
    for (i, v) in [root, STR, flags].into_iter().enumerate() {
        bus.escrever_u32(GuestVirt(ATTR + i as u32 * 4), v).unwrap();
    }
}
fn args() -> [u32; 9] {
    [HANDLE, 0x80100080, ATTR, IO, 0, 0x80, 1, 1, 0x60]
}
fn open(bus: &mut Bus, d: &mut Dispatcher, regs: &[u32; 8]) -> u32 {
    path(bus, b"\\Device\\CdRom0\\default.xbe", 0, 0x40);
    assert_eq!(call(bus, d, regs, 190, &args()).unwrap(), 0);
    assert_eq!(word(bus, IO), 0);
    assert_eq!(word(bus, IO + 4), 1);
    word(bus, HANDLE)
}
fn word(bus: &Bus, at: u32) -> u32 {
    bus.ler_u32(GuestVirt(at)).unwrap()
}
fn read(bus: &mut Bus, d: &mut Dispatcher, regs: &[u32; 8], handle: u32, n: u32, at: u32) -> u32 {
    call(bus, d, regs, 219, &[handle, 0, 0, 0, IO, OUT, n, at]).unwrap()
}
fn saved(d: &impl Snapshot) -> Vec<u8> {
    let mut w = SnapshotWriter::new();
    d.save(&mut w);
    w.finish()
}
fn load(d: &mut Dispatcher, data: &[u8]) {
    d.load(&mut SnapshotReader::new(data).unwrap()).unwrap();
}
fn position(bus: &mut Bus, d: &mut Dispatcher, regs: &[u32; 8], handle: u32) -> u64 {
    assert_eq!(
        call(bus, d, regs, 211, &[handle, IO, OUT, 8, 14]).unwrap(),
        0
    );
    u64::from(word(bus, OUT)) | (u64::from(word(bus, OUT + 4)) << 32)
}
#[test]
fn opens_independentes_leem_bytes_originais_e_close_nao_reutiliza_handle() {
    let (mut b, mut d, r, image) = setup();
    let first = open(&mut b, &mut d, &r);
    let second = open(&mut b, &mut d, &r);
    assert_eq!((first, second), (4, 8));
    assert_eq!(read(&mut b, &mut d, &r, first, 4, 0), 0);
    assert_eq!(word(&b, OUT), u32::from_le_bytes(*b"XBEH"));
    assert_eq!(read(&mut b, &mut d, &r, first, 4, 0), 0);
    assert_eq!(
        word(&b, OUT),
        u32::from_le_bytes(image[4..8].try_into().unwrap())
    );
    assert_eq!(position(&mut b, &mut d, &r, second), 0);
    assert_eq!(position(&mut b, &mut d, &r, first), 8);
    assert_eq!(call(&mut b, &mut d, &r, 187, &[first]).unwrap(), 0);
    assert_eq!(
        call(&mut b, &mut d, &r, 187, &[first]).unwrap(),
        STATUS_INVALID_HANDLE
    );
    assert_eq!(read(&mut b, &mut d, &r, first, 4, 0), STATUS_INVALID_HANDLE);
    assert_eq!(open(&mut b, &mut d, &r), 12);
    assert_eq!(d.executable_media(), Some(image.as_slice()));
}
#[test]
fn leitura_curta_eof_offsets_explicitos_e_sentinela_de_posicao() {
    let (mut b, mut d, r, image) = setup();
    let h = open(&mut b, &mut d, &r);
    b.escrever_bytes(GuestVirt(OFFSET), &((image.len() - 2) as u64).to_le_bytes())
        .unwrap();
    b.escrever_bytes(GuestVirt(OUT), &[0xAB; 8]).unwrap();
    assert_eq!(read(&mut b, &mut d, &r, h, 8, OFFSET), 0);
    assert_eq!(word(&b, IO + 4), 2);
    let mut bytes = [0; 8];
    b.ler_bytes(GuestVirt(OUT), &mut bytes).unwrap();
    assert_eq!(&bytes[..2], &image[image.len() - 2..]);
    assert_eq!(&bytes[2..], &[0xAB; 6]);
    assert_eq!(read(&mut b, &mut d, &r, h, 8, 0), 0xC0000011);
    assert_eq!(word(&b, IO), 0xC0000011);
    assert_eq!(word(&b, IO + 4), 0);
    b.escrever_bytes(GuestVirt(OFFSET), &0u64.to_le_bytes())
        .unwrap();
    assert_eq!(read(&mut b, &mut d, &r, h, 4, OFFSET), 0);
    b.escrever_bytes(GuestVirt(OFFSET), &(-2i64).to_le_bytes())
        .unwrap();
    assert_eq!(read(&mut b, &mut d, &r, h, 4, OFFSET), 0);
    assert_eq!(position(&mut b, &mut d, &r, h), 8);
}
#[test]
fn query_standard_network_position_limites_e_campos_deterministicos() {
    let (mut b, mut d, r, image) = setup();
    let h = open(&mut b, &mut d, &r);
    for (class, size, eof_at) in [(5, 24, 8), (34, 56, 40)] {
        b.escrever_bytes(GuestVirt(OUT), &[0xAD; 64]).unwrap();
        assert_eq!(
            call(&mut b, &mut d, &r, 211, &[h, IO, OUT, size, class]).unwrap(),
            0
        );
        assert_eq!(word(&b, OUT + eof_at), image.len() as u32);
        assert_eq!(word(&b, IO + 4), size);
        assert_eq!(b.ler_u8(GuestVirt(OUT + size)).unwrap(), 0xAD);
    }
    assert_eq!(word(&b, OUT + 48), 1);
    assert_eq!(word(&b, OUT), 0);
    b.escrever_u32(GuestVirt(OUT), 0x12345678).unwrap();
    assert_eq!(
        call(&mut b, &mut d, &r, 211, &[h, IO, OUT, 7, 14]).unwrap(),
        0xC0000004
    );
    assert_eq!(word(&b, OUT), 0x12345678);
    assert!(matches!(
        call(&mut b, &mut d, &r, 211, &[h, IO, OUT, 64, 1]),
        Err(Erro::VarianteNaoImplementada { .. })
    ));
}
#[test]
fn preflight_invalido_preserva_buffer_posicao_e_proximo_handle() {
    let (mut b, mut d, r, _) = setup();
    let h = open(&mut b, &mut d, &r);
    let before = saved(&d);
    b.escrever_bytes(GuestVirt(0x12FFE), &[0xAD; 2]).unwrap();
    for buffer in [0x12FFE, 0xFD000000, 0x11000] {
        assert_eq!(
            call(&mut b, &mut d, &r, 219, &[h, 0, 0, 0, IO, buffer, 8, 0]).unwrap(),
            STATUS_ACCESS_VIOLATION
        );
        assert_eq!(b.ler_u16(GuestVirt(0x12FFE)).unwrap(), 0xADAD);
        assert_eq!(&saved(&d)[36..], &before[36..]);
    }
    let mut invalid = args();
    invalid[0] = 0x12FFE;
    assert_eq!(
        call(&mut b, &mut d, &r, 190, &invalid).unwrap(),
        STATUS_ACCESS_VIOLATION
    );
    assert_eq!(open(&mut b, &mut d, &r), 8);
}
#[test]
fn variantes_assincronas_escrita_traversal_e_saidas_sobrepostas_recusam() {
    let (mut b, mut d, r, _) = setup();
    let h = open(&mut b, &mut d, &r);
    let before = saved(&d);
    for (at, value) in [(1, 4), (2, 0x11000), (6, 0), (6, 0x100001)] {
        let mut args = [h, 0, 0, 0, IO, OUT, 4, 0];
        args[at] = value;
        assert!(matches!(
            call(&mut b, &mut d, &r, 219, &args),
            Err(Erro::VarianteNaoImplementada { .. })
        ));
    }
    assert!(call(&mut b, &mut d, &r, 219, &[h, 0, 0, 0, IO, IO, 4, 0]).is_err());
    for text in [
        b"\\Device\\CdRom0\\..\\default.xbe".as_slice(),
        b"D:/default.xbe",
        b"\\Device\\Harddisk0\\Partition1\\default.xbe",
        b"D:\\bad\0name",
    ] {
        path(&mut b, text, 0, 0x40);
        assert!(call(&mut b, &mut d, &r, 190, &args()).is_err());
    }
    assert_eq!(&saved(&d)[36..], &before[36..]);
}
#[test]
fn namespace_case_sensitive_ou_insensitive_e_dos_devices_resolvem_link_real() {
    let (mut b, mut d, r, _) = setup();
    path(&mut b, b"\\DEVICE\\CDROM0\\DEFAULT.XBE", 0, 0);
    assert_eq!(call(&mut b, &mut d, &r, 190, &args()).unwrap(), 0xC0000034);
    path(&mut b, b"\\DEVICE\\CDROM0\\DEFAULT.XBE", 0, 0x40);
    assert_eq!(call(&mut b, &mut d, &r, 190, &args()).unwrap(), 0);
    path(&mut b, b"D:\\default.xbe", 0xFFFFFFFD, 0x40);
    assert_eq!(call(&mut b, &mut d, &r, 190, &args()).unwrap(), 0xC0000034);
    string(&mut b, 0x121A0, 0x12300, b"\\??\\D:");
    string(&mut b, 0x121A8, 0x12340, b"\\Device\\CdRom0\\");
    assert_eq!(
        call(&mut b, &mut d, &r, 67, &[0x121A0, 0x121A8]).unwrap(),
        0
    );
    for (root, text) in [
        (0xFFFFFFFD, b"d:\\DEFAULT.XBE".as_slice()),
        (0, b"\\??\\D:\\default.xbe"),
    ] {
        path(&mut b, text, root, 0x40);
        assert_eq!(call(&mut b, &mut d, &r, 190, &args()).unwrap(), 0);
    }
}
#[test]
fn compartilhamento_e_limite_nao_consumem_handles_e_outros_objetos_nao_colidem() {
    let (mut b, mut d, r, _) = setup();
    path(&mut b, b"\\Device\\CdRom0\\default.xbe", 0, 0x40);
    let mut exclusive = args();
    exclusive[6] = 0;
    assert_eq!(call(&mut b, &mut d, &r, 190, &exclusive).unwrap(), 0);
    let h = word(&b, HANDLE);
    assert_eq!(call(&mut b, &mut d, &r, 190, &args()).unwrap(), 0xC0000043);
    assert_eq!(call(&mut b, &mut d, &r, 187, &[h]).unwrap(), 0);
    for _ in 0..MAX_FILE_HANDLES {
        open(&mut b, &mut d, &r);
    }
    let before = saved(&d);
    assert_eq!(call(&mut b, &mut d, &r, 190, &args()).unwrap(), 0xC000009A);
    assert_eq!(&saved(&d)[36..], &before[36..]);
    assert_eq!(call(&mut b, &mut d, &r, 192, &[HANDLE, 0, 0]).unwrap(), 0);
    assert_eq!(word(&b, HANDLE), (MAX_FILE_HANDLES as u32 + 2) * 4);
}
#[test]
fn snapshot_preserva_midia_cursor_e_replay_e_krnl_v13_nao_herda_midia() {
    let (mut b, mut d, r, _) = setup();
    let h = open(&mut b, &mut d, &r);
    read(&mut b, &mut d, &r, h, 4, 0);
    let checkpoint = saved(&d);
    read(&mut b, &mut d, &r, h, 8, 0);
    let end = saved(&d);
    let mut restored = Dispatcher::new();
    load(&mut restored, &checkpoint);
    assert_eq!(saved(&restored), checkpoint);
    read(&mut b, &mut restored, &r, h, 8, 0);
    assert_eq!(saved(&restored), end);
    // KRNL v13 sem CDFS nem handles: formato real anterior.
    let (_, empty, _, _) = setup();
    let mut old = saved(&empty);
    let at = old.windows(4).rposition(|v| v == b"CDFS").unwrap();
    old.truncate(at);
    old[16..20].copy_from_slice(&13u32.to_le_bytes());
    load(&mut restored, &old);
    assert!(restored.executable_media().is_none());
    let again = saved(&restored);
    load(&mut restored, &again);
    assert!(restored.executable_media().is_none());
}
#[test]
fn snapshot_corrompido_recusa_versao_tamanho_contagem_posicao_share_e_handles() {
    let (mut b, mut d, r, image) = setup();
    open(&mut b, &mut d, &r);
    let source = saved(&d);
    let at = source.windows(4).rposition(|v| v == b"CDFS").unwrap();
    let row = at + 17 + image.len();
    for (offset, bytes) in [
        (at + 4, 3u32.to_le_bytes().to_vec()),
        (at + 9, 0xFFFFFFFFu32.to_le_bytes().to_vec()),
        (row - 4, 257u32.to_le_bytes().to_vec()),
        (row, 0u32.to_le_bytes().to_vec()),
        (row + 4, u64::MAX.to_le_bytes().to_vec()),
        (row + 12, 8u32.to_le_bytes().to_vec()),
    ] {
        let mut bad = source.clone();
        bad[offset..offset + bytes.len()].copy_from_slice(&bytes);
        assert!(
            Dispatcher::new()
                .load(&mut SnapshotReader::new(&bad).unwrap())
                .is_err()
        );
    }
    assert!(
        Dispatcher::new()
            .load(&mut SnapshotReader::new(&source[..source.len() - 1]).unwrap())
            .is_err()
    );
    assert!(d.mount_executable_media(&image).is_err());
    assert!(Dispatcher::new().mount_executable_media(&[]).is_err());
}

#[test]
fn alias_fisico_entre_buffer_e_iosb_e_recusado_sem_copia_ou_consumo() {
    let (mut b, mut d, r, _) = setup();
    let h = open(&mut b, &mut d, &r);
    let pa = b.traduzir(GuestVirt(0x12000), Acesso::Leitura).unwrap();
    b.mmu_mut()
        .mapear(GuestVirt(0x50000000), pa, 1, Perm::RW)
        .unwrap();
    let before = saved(&d);
    let old = word(&b, IO);
    assert!(call(&mut b, &mut d, &r, 219, &[h, 0, 0, 0, IO, 0x50000150, 4, 0]).is_err());
    assert_eq!(&saved(&d)[36..], &before[36..]);
    assert_eq!(word(&b, IO), old);
    let mut aliased = args();
    aliased[0] = 0x50000150;
    assert!(call(&mut b, &mut d, &r, 190, &aliased).is_err());
    assert_eq!(&saved(&d)[36..], &before[36..]);
}

#[test]
fn snapshot_recusa_colisao_com_mutant_e_create_variantes_nao_publicam_saida() {
    let (mut b, mut d, r, _) = setup();
    open(&mut b, &mut d, &r);
    assert_eq!(call(&mut b, &mut d, &r, 192, &[HANDLE, 0, 0]).unwrap(), 0);
    let mutant = word(&b, HANDLE);
    let source = saved(&d);
    let at = source.windows(4).rposition(|v| v == b"CDFS").unwrap();
    let row = at + 17 + d.executable_media().unwrap().len();
    let mut corrupt = source.clone();
    corrupt[row..row + 4].copy_from_slice(&mutant.to_le_bytes());
    assert!(
        Dispatcher::new()
            .load(&mut SnapshotReader::new(&corrupt).unwrap())
            .is_err()
    );
    for (index, value) in [
        (1, 0x40100080),
        (1, 0x100001),
        (4, OFFSET),
        (7, 2),
        (8, 0x40),
    ] {
        let mut variant = args();
        variant[index] = value;
        assert!(matches!(
            call(&mut b, &mut d, &r, 190, &variant),
            Err(Erro::VarianteNaoImplementada { .. })
        ));
        assert_eq!(&saved(&d)[36..], &source[36..]);
        assert_eq!(word(&b, HANDLE), mutant);
    }
}
