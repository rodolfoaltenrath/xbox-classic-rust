use xbox_core::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_mem::{Arena, Bus, GuestPhys, GuestVirt, Perm, PfifoProfile};
mod support;
fn write(b: &mut Bus, off: u32, value: u32) {
    b.escrever_u32(GuestVirt(0xFD000000 + off), value).unwrap();
}
fn read(b: &Bus, off: u32) -> u32 {
    b.ler_u32(GuestVirt(0xFD000000 + off)).unwrap()
}
fn modern() -> Bus {
    let mut b = Bus::com_arena(Arena::com_tamanho(0x10000).unwrap()).unwrap();
    for (va, pages) in [(0xFD002000, 2), (0xFD400000, 1), (0xFD710000, 3)] {
        b.mapear(GuestVirt(va), GuestPhys(va), pages, Perm::RW)
            .unwrap();
    }
    for (off, v) in [
        (0x7111C0, 0xB002),
        (0x7111C4, 0xFFFF),
        (0x7111C8, 3),
        (0x712000, 0x97),
        (0x710068, 13),
        (0x71006C, 0x80011200),
        (0x710030, 6),
        (0x710034, 0x8000111C),
        (0x2210, 0x03000100),
        (0x322C, 0x111C),
        (0x3204, 0x100),
        (0x2504, 1),
        (0x3200, 1),
        (0x3250, 1),
        (0x3220, 1),
        (0x400720, 1),
    ] {
        write(&mut b, off, v);
    }
    b
}
// Os contratos puros do PFIFO continuam verificando o perfil BUS0 v8, em que
// métodos (inclusive NOP não zero) eram só telemetria. O acoplamento novo e o
// bloqueio por trap são exercitados separadamente abaixo no perfil moderno.
fn setup() -> Bus {
    let mut b = modern();
    let mut bytes = support::without_pgraph(&saved(&b)).to_vec();
    bytes[16..20].copy_from_slice(&8u32.to_le_bytes());
    b.load(&mut SnapshotReader::new(&bytes).unwrap()).unwrap();
    b
}
fn data(b: &mut Bus, base: u32, words: &[u32]) {
    let bytes: Vec<_> = words.iter().flat_map(|v| v.to_le_bytes()).collect();
    b.carregar_em_fisico(GuestPhys(base), &bytes).unwrap();
}
fn program(b: &mut Bus, words: &[u32]) {
    data(b, 0x1000, words);
    write(b, 0x3244, 0x1000);
    write(b, 0x3240, 0x1000 + words.len() as u32 * 4);
}
fn saved(b: &Bus) -> Vec<u8> {
    let mut w = SnapshotWriter::new();
    b.save(&mut w);
    w.finish()
}
#[test]
fn bootstrap_jump_lido_da_ram_atinge_put_sem_atalho_por_escrita() {
    for word in [0x1001, 0x20001000] {
        let mut b = setup();
        data(&mut b, 0, &[word]);
        write(&mut b, 0x3240, 0x1000);
        assert_eq!(read(&b, 0x3244), 0);
        assert_eq!(read(&b, 0x3214), 0);
        let report = b.service_gpu(1).unwrap();
        assert_eq!(report.words, 1);
        assert!(report.drained);
        assert_eq!(read(&b, 0x3244), 0x1000);
        assert_eq!(read(&b, 0x3214), 16);
        assert_eq!(b.nv2a_pfifo().methods(), []);
    }
}
#[test]
fn metodos_inc_nao_inc_binding_e_contexto_dma_sao_observados() {
    let mut b = setup();
    program(
        &mut b,
        &[
            1 << 18,
            13,
            (2 << 18) | 0x100,
            10,
            11,
            (1 << 30) | (3 << 18) | 0x1818,
            1,
            2,
            3,
            (1 << 18) | 0x180,
            6,
        ],
    );
    assert!(b.service_gpu(100).unwrap().drained);
    let records = b.nv2a_pfifo().methods();
    let at = |method| records.iter().find(|r| r.method == method).unwrap();
    assert_eq!((at(0).class, at(0).first), (0x97, 13));
    assert_eq!(at(0x100).first, 10);
    assert_eq!(at(0x104).first, 11);
    assert_eq!(
        (at(0x1818).count, at(0x1818).first, at(0x1818).last),
        (3, 1, 3)
    );
    assert_eq!((at(0x180).first, at(0x180).first_resolved), (6, 0x111C0));
}
#[test]
fn pacote_incompleto_aguarda_put_e_conserva_metodo_subcanal_e_contagem() {
    let mut b = setup();
    program(&mut b, &[1 << 18, 13, (2 << 18) | 0x100, 10]);
    assert!(!b.service_gpu(100).unwrap().drained);
    assert_eq!(read(&b, 0x3244), 0x1010);
    assert_eq!(read(&b, 0x3214), 0);
    assert_eq!(b.service_gpu(100).unwrap().words, 0);
    data(&mut b, 0x1010, &[11]);
    write(&mut b, 0x3240, 0x1014);
    assert!(b.service_gpu(1).unwrap().drained);
    let r = b
        .nv2a_pfifo()
        .methods()
        .iter()
        .find(|r| r.method == 0x104)
        .unwrap();
    assert_eq!(r.first, 11);
}
#[test]
fn call_return_e_jump_respeitam_orcamento_e_endereco_de_retorno() {
    let mut b = setup();
    let mut words = [0; 14];
    words[0] = 0x1022;
    words[1] = 0x1031;
    words[8] = 1 << 18;
    words[9] = 13;
    words[10] = 0x20000;
    words[12] = (1 << 18) | 0x100;
    words[13] = 7;
    program(&mut b, &words);
    b.service_gpu(1).unwrap();
    assert_eq!(read(&b, 0x3244), 0x1020);
    assert_eq!(read(&b, 0x324C), 0x1005);
    b.service_gpu(3).unwrap();
    assert_eq!(read(&b, 0x3244), 0x1004);
    assert_eq!(read(&b, 0x324C), 0);
    assert!(b.service_gpu(3).unwrap().drained);
    assert_eq!(
        b.nv2a_pfifo()
            .methods()
            .iter()
            .find(|r| r.method == 0x100)
            .unwrap()
            .first,
        7
    );
}
#[test]
fn descritor_invalido_limite_e_ramht_recusam_sem_consumir_palavra_invalida() {
    for (off, value) in [
        (0x7111C0, 0xB003),
        (0x7111C4, 0x1002),
        (0x7111C8, 0xFFFFF003),
    ] {
        let mut b = setup();
        program(&mut b, &[1 << 18, 13]);
        write(&mut b, off, value);
        let err = b.service_gpu(100).unwrap_err();
        assert_eq!(err.get, 0x1000);
        assert_eq!(read(&b, 0x3244), 0x1000);
        assert_eq!(b.nv2a_pfifo().methods(), []);
    }
    let mut b = setup();
    program(&mut b, &[1 << 18, 13]);
    write(&mut b, 0x71006C, 0);
    assert_eq!(b.service_gpu(100).unwrap_err().get, 0x1004);
    write(&mut b, 0x71006C, 0x80011200);
    assert!(b.service_gpu(100).unwrap().drained);
    assert_eq!(b.nv2a_pfifo().methods()[0].class, 0x97);
}
#[test]
fn return_sem_call_call_aninhado_e_header_reservado_recusam_explicitamente() {
    for word in [0x20000, 0xDEADBEEF] {
        let mut b = setup();
        program(&mut b, &[word]);
        assert!(b.service_gpu(1).is_err());
        assert_eq!(read(&b, 0x3244), 0x1000);
    }
    let mut b = setup();
    program(&mut b, &[0x1002]);
    assert_eq!(b.service_gpu(2).unwrap_err().get, 0x1000);
    assert_eq!(read(&b, 0x324C), 0x1005);
}
#[test]
fn bus_v7_pffo_v1_migra_para_controles_sem_consumidor_novo() {
    let mut b = setup();
    program(&mut b, &[1 << 18, 13]);
    let mut old = saved(&b);
    let at = support::without_pfifo(&old).len();
    let payload = at + 16 + 37 * 4;
    old.truncate(payload);
    old[16..20].copy_from_slice(&7u32.to_le_bytes());
    old[at + 4..at + 8].copy_from_slice(&1u32.to_le_bytes());
    old[at + 8..at + 12].copy_from_slice(&1u32.to_le_bytes());
    let mut restored = setup();
    restored
        .load(&mut SnapshotReader::new(&old).unwrap())
        .unwrap();
    assert_eq!(restored.nv2a_pfifo().profile(), PfifoProfile::RegistersV1);
    assert_eq!(restored.service_gpu(100).unwrap().words, 0);
    assert_eq!(read(&restored, 0x3244), 0x1000);
    assert_eq!(read(&restored, 0x3214), 0);
    let checkpoint = saved(&restored);
    b.load(&mut SnapshotReader::new(&checkpoint).unwrap())
        .unwrap();
    assert_eq!(saved(&b), checkpoint);
}

#[test]
fn snapshot_no_meio_do_pacote_e_dos_parametros_reproduz_get_e_lista_de_metodos() {
    let mut b = setup();
    program(
        &mut b,
        &[
            1 << 18,
            13,
            (3 << 18) | 0x100,
            10,
            11,
            12,
            (1 << 30) | (2 << 18) | 0x1818,
            3,
            4,
        ],
    );
    assert_eq!(b.service_gpu(3).unwrap().words, 3);
    let pending_header = saved(&b);
    b.service_gpu(2).unwrap();
    let pending_data = saved(&b);
    assert!(b.service_gpu(100).unwrap().drained);
    let expected_get = read(&b, 0x3244);
    assert_eq!(expected_get, read(&b, 0x3240));
    let expected_methods = b.nv2a_pfifo().methods().to_vec();
    let complete = saved(&b);
    assert_eq!(expected_methods.iter().map(|r| r.count).sum::<u64>(), 6);
    for checkpoint in [pending_header, pending_data] {
        let mut restored = setup();
        restored
            .load(&mut SnapshotReader::new(&checkpoint).unwrap())
            .unwrap();
        assert_eq!(saved(&restored), checkpoint);
        assert!(restored.service_gpu(100).unwrap().drained);
        assert_eq!(read(&restored, 0x3244), expected_get);
        assert_eq!(restored.nv2a_pfifo().methods(), expected_methods);
        assert_eq!(saved(&restored), complete);
    }
}
#[test]
fn snapshot_com_call_ativo_e_objeto_ligado_reproduz_return_e_parametros() {
    let mut b = setup();
    let mut words = [0; 14];
    words[0] = 0x1022;
    words[1] = 0x1031;
    words[8] = 1 << 18;
    words[9] = 13;
    words[10] = 0x20000;
    words[12] = (1 << 18) | 0x100;
    words[13] = 7;
    program(&mut b, &words);
    b.service_gpu(3).unwrap();
    assert_eq!(read(&b, 0x324C), 0x1005);
    let checkpoint = saved(&b);
    assert!(b.service_gpu(100).unwrap().drained);
    let complete = saved(&b);
    let mut restored = setup();
    restored
        .load(&mut SnapshotReader::new(&checkpoint).unwrap())
        .unwrap();
    assert_eq!(read(&restored, 0x324C), 0x1005);
    assert!(restored.service_gpu(100).unwrap().drained);
    assert_eq!(read(&restored, 0x3244), 0x1038);
    assert_eq!(read(&restored, 0x324C), 0);
    assert_eq!(restored.nv2a_pfifo().methods(), b.nv2a_pfifo().methods());
    assert_eq!(saved(&restored), complete);
}
#[test]
fn snapshot_recusa_binding_registro_e_versao_de_comandos_sem_publicar_pfifo() {
    let mut b = setup();
    program(&mut b, &[1 << 18, 13, (1 << 18) | 0x100, 7]);
    b.service_gpu(3).unwrap();
    let checkpoint = saved(&b);
    let at = checkpoint.windows(4).rposition(|v| v == b"PCMD").unwrap();
    // PCMD v1: binding 0 presente (13 bytes), sete ausentes; contagem em +28.
    for (off, value) in [
        (4, 0u32),
        (4, 2),
        (13, 0x100000),
        (17, 0x1000),
        (28, 4097),
        (32, 0x1000),
    ] {
        let mut bad = checkpoint.clone();
        bad[at + off..at + off + 4].copy_from_slice(&value.to_le_bytes());
        let mut dest = setup();
        let before = dest.nv2a_pfifo().clone();
        assert!(dest.load(&mut SnapshotReader::new(&bad).unwrap()).is_err());
        assert_eq!(dest.nv2a_pfifo(), &before);
    }
    let mut bad = checkpoint.clone();
    bad[at + 8] = 2;
    assert!(
        setup()
            .load(&mut SnapshotReader::new(&bad).unwrap())
            .is_err()
    );
    let mut bad = checkpoint;
    bad[at + 39..at + 47].copy_from_slice(&0u64.to_le_bytes());
    assert!(
        setup()
            .load(&mut SnapshotReader::new(&bad).unwrap())
            .is_err()
    );
}

fn clear_trap() -> Bus {
    let mut b = modern();
    for base in [0xFD000000, 0xFD600000] {
        b.mapear(GuestVirt(base), GuestPhys(base), 1, Perm::RW)
            .unwrap();
    }
    for (off, value) in [
        (0x710038, 7),
        (0x71003C, 0x8000111D),
        (0x7111D0, 3),
        (0x7111D4, 0xFFF),
        (0x7111D8, 0x2000),
    ] {
        write(&mut b, off, value);
    }
    let mut words = vec![1 << 18, 13];
    for (method, v) in [
        (0x194, 7),
        (0x200, 4 << 16),
        (0x204, 3 << 16),
        (0x208, 0x128),
        (0x20C, 16),
        (0x1D90, 0xFF102030),
        (0x1D98, 3 << 16),
        (0x1D9C, 2 << 16),
        (0x100, 0x12345678),
        (0x1D94, 0xF0),
    ] {
        words.extend([(1 << 18) | method, v]);
    }
    program(&mut b, &words);
    b
}
fn ack_and_clear(b: &mut Bus) {
    write(b, 0x7111D8, 0x3000);
    write(b, 0x400100, 1 << 20);
    assert!(b.service_gpu(100).unwrap().drained);
    assert_eq!(b.dma(GuestPhys(0x2000), 48).unwrap(), [0; 48]);
    assert_eq!(
        b.dma(GuestPhys(0x3000), 48).unwrap(),
        [0xFF102030u32.to_le_bytes(); 12].concat()
    );
}
#[test]
fn pgraph_trap_suspende_dma_ate_ack_e_clear_usa_descritor_reescrito_pelo_guest() {
    let mut b = clear_trap();
    let step = b.service_gpu(100).unwrap();
    assert!(!step.drained);
    assert_eq!(read(&b, 0x3244), 0x1050);
    let get = read(&b, 0x3244);
    assert_eq!(b.service_gpu(100).unwrap().words, 0);
    assert_eq!(read(&b, 0x3244), get);
    assert_eq!(read(&b, 0x400708), 0x12345678);
    assert!(!b.gpu_irq_line_asserted());
    write(&mut b, 0x400140, 1 << 20);
    assert_eq!(read(&b, 0x100), 1 << 12);
    write(&mut b, 0x140, 1);
    assert!(b.gpu_irq_line_asserted());
    b.nv2a_irq_mut().raise_vblank();
    write(&mut b, 0x600140, 1);
    assert_eq!(read(&b, 0x100), (1 << 12) | (1 << 24));
    let checkpoint = saved(&b);
    ack_and_clear(&mut b);
    assert_eq!(read(&b, 0x100), 1 << 24);
    write(&mut b, 0x600100, 1);
    assert!(!b.gpu_irq_line_asserted());
    let mut r = modern();
    r.load(&mut SnapshotReader::new(&checkpoint).unwrap())
        .unwrap();
    assert_eq!(saved(&r), checkpoint);
    ack_and_clear(&mut r);
    write(&mut r, 0x600100, 1);
    assert_eq!(saved(&r), saved(&b));
}
#[test]
fn bus_v8_preserva_consumo_sem_pgraph_e_scanout_valida_store_antes_de_publicar() {
    let mut b = clear_trap();
    let mut old = support::without_pgraph(&saved(&b)).to_vec();
    old[16..20].copy_from_slice(&8u32.to_le_bytes());
    b.load(&mut SnapshotReader::new(&old).unwrap()).unwrap();
    assert_eq!(
        b.nv2a_pgraph().profile(),
        xbox_mem::PgraphProfile::LegacyUnmodeled
    );
    assert!(b.service_gpu(100).unwrap().drained);
    assert_eq!(b.dma(GuestPhys(0x2000), 48).unwrap(), [0; 48]);
    assert!(!b.gpu_irq_line_asserted());
    let bytes = saved(&b);
    let mut r = modern();
    r.load(&mut SnapshotReader::new(&bytes).unwrap()).unwrap();
    assert_eq!(saved(&r), bytes);
    let mut m = clear_trap();
    write(&mut m, 0x600800, 0x3000);
    let before = m.nv2a_pgraph().clone();
    for value in [0x3001, 0x10000, u32::MAX] {
        assert!(m.escrever_u32(GuestVirt(0xFD600800), value).is_err());
        assert_eq!(m.nv2a_pgraph(), &before);
    }
    assert!(m.escrever_u16(GuestVirt(0xFD400720), 1).is_err());
    assert_eq!(m.nv2a_pgraph(), &before);
}
