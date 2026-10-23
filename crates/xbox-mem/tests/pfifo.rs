use xbox_core::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_mem::{Arena, Bus, GuestPhys, GuestVirt, Perm, PfifoProfile};
mod support;
fn bus() -> Bus {
    let mut b = Bus::com_arena(Arena::com_tamanho(0x4000).unwrap()).unwrap();
    b.mapear(GuestVirt(0xFD002000), GuestPhys(0xFD002000), 2, Perm::RW)
        .unwrap();
    b.mapear(GuestVirt(0xFD800000), GuestPhys(0xFD800000), 17, Perm::RW)
        .unwrap();
    b.mapear(GuestVirt(0x40000000), GuestPhys(0xFD003000), 1, Perm::RW)
        .unwrap();
    b
}
fn write(b: &mut Bus, off: u32, v: u32) {
    b.escrever_u32(GuestVirt(0xFD000000 + off), v).unwrap();
}
fn read(b: &Bus, off: u32) -> u32 {
    b.ler_u32(GuestVirt(0xFD000000 + off)).unwrap()
}
fn channel(b: &mut Bus) {
    write(b, 0x2504, 1);
    write(b, 0x3204, 0x100);
}
fn saved(b: &Bus) -> Vec<u8> {
    let mut w = SnapshotWriter::new();
    b.save(&mut w);
    w.finish()
}
fn load(b: &mut Bus, bytes: &[u8]) {
    let mut r = SnapshotReader::new(bytes).unwrap();
    b.load(&mut r).unwrap();
    assert_eq!(r.remaining(), 0);
}
#[test]
fn controles_e_aliases_user_compartilham_estado_sem_avancar_dma() {
    let mut b = bus();
    channel(&mut b);
    write(&mut b, 0x322C, 0x111C);
    write(&mut b, 0x3244, 4);
    write(&mut b, 0x800040, 0x18C000);
    assert_eq!(read(&b, 0x3240), 0x18C000);
    assert_eq!(read(&b, 0x800040), 0x18C000);
    assert_eq!(read(&b, 0x800044), 4);
    assert_eq!(read(&b, 0x3244), 4);
    b.escrever_u32(GuestVirt(0x40000244), 8).unwrap();
    assert_eq!(read(&b, 0x800044), 8);
    let mut value = [0; 4];
    b.ler_fisico(GuestPhys(0xFD00322C), &mut value).unwrap();
    assert_eq!(u32::from_le_bytes(value), 0x111C);
    b.escrever_fisico(GuestPhys(0xFD800040), &0x20u32.to_le_bytes())
        .unwrap();
    assert_eq!(read(&b, 0x3240), 0x20);
    assert_eq!(read(&b, 0x3244), 8);
    assert_eq!(b.telemetria().total_de_acessos(), 0);
}
#[test]
fn read_modify_write_e_subpalavras_sao_little_endian_e_intr_e_w1c() {
    let mut b = bus();
    write(&mut b, 0x3204, 1);
    let push = read(&b, 0x3204);
    write(&mut b, 0x3204, push | 0x100);
    assert_eq!(read(&b, 0x3204), 0x101);
    write(&mut b, 0x3224, 0x12345678);
    assert_eq!(b.ler_u8(GuestVirt(0xFD003225)).unwrap(), 0x56);
    assert_eq!(b.ler_u16(GuestVirt(0xFD003226)).unwrap(), 0x1234);
    write(&mut b, 0x2100, u32::MAX);
    assert_eq!(read(&b, 0x2100), 0);
    assert!(!b.nv2a_irq().line_asserted());
}
#[test]
fn canal_pio_nao_selecionado_e_canais_user_fora_do_recorte_recusam() {
    let mut b = bus();
    for (mode, push, addr) in [
        (0, 0, 0xFD800040),
        (1, 0, 0xFD800040),
        (1, 0x101, 0xFD800040),
        (1, 0x100, 0xFD810040),
    ] {
        write(&mut b, 0x2504, mode);
        write(&mut b, 0x3204, push);
        let before = b.nv2a_pfifo().clone();
        assert!(b.escrever_u32(GuestVirt(addr), 123).is_err());
        assert!(b.ler_u32(GuestVirt(addr)).is_err());
        assert_eq!(b.nv2a_pfifo(), &before);
    }
    channel(&mut b);
    write(&mut b, 0x800040, 16);
    assert_eq!(read(&b, 0x3240), 16);
}
#[test]
fn largura_invalida_e_store_composto_nao_alteram_controle_ou_saida() {
    let mut b = bus();
    channel(&mut b);
    write(&mut b, 0x3240, 0x10);
    let before = b.nv2a_pfifo().clone();
    for (addr, size) in [
        (0xFD003240, 1),
        (0xFD003240, 2),
        (0xFD003241, 4),
        (0xFD003240, 8),
        (0xFD00323F, 4),
    ] {
        assert!(
            b.escrever_bytes(GuestVirt(addr), &[0xFF; 8][..size])
                .is_err()
        );
        assert_eq!(b.nv2a_pfifo(), &before);
    }
    let mut out = [0xAA; 8];
    assert!(b.ler_bytes(GuestVirt(0xFD003240), &mut out).is_err());
    assert_eq!(out, [0xAA; 8]);
    assert!(b.escrever_fisico(GuestPhys(0xFD003240), &[1; 8]).is_err());
    assert_eq!(b.nv2a_pfifo(), &before);
}
#[test]
fn controles_fora_do_inventario_e_estados_empty_continuam_na_telemetria() {
    let mut b = bus();
    for off in [0x2080, 0x32A0] {
        assert_eq!(read(&b, off), 0);
    }
    assert_eq!(b.telemetria().total_de_acessos(), 2);
    write(&mut b, 0x3240, 4);
    assert_eq!(read(&b, 0x3244), 0);
    assert_eq!(read(&b, 0x3214), 0);
}
#[test]
fn snapshot_restabelece_perfil_controles_aliases_e_valida_payload() {
    let mut b = bus();
    channel(&mut b);
    b.reserve_boot_dma_page().unwrap();
    write(&mut b, 0x3240, 16);
    write(&mut b, 0x3244, 4);
    let checkpoint = saved(&b);
    let mut dest = bus();
    load(&mut dest, &checkpoint);
    assert_eq!(saved(&dest), checkpoint);
    assert_eq!(read(&dest, 0x800044), 4);
    let at = support::without_pfifo(&checkpoint).len();
    let fifo = dest.nv2a_pfifo().clone();
    for (off, v) in [
        (4, 0u32),
        (4, 3),
        (8, 3),
        (12, 0),
        (12, 38),
        (16 + 3 * 4, 1),
    ] {
        let mut bad = checkpoint.clone();
        bad[at + off..at + off + 4].copy_from_slice(&v.to_le_bytes());
        assert!(dest.load(&mut SnapshotReader::new(&bad).unwrap()).is_err());
        assert_eq!(dest.nv2a_pfifo(), &fifo);
    }
    for end in [at, at + 8, at + 12, at + 16, checkpoint.len() - 1] {
        assert!(
            dest.load(&mut SnapshotReader::new(&checkpoint[..end]).unwrap())
                .is_err()
        );
        assert_eq!(dest.nv2a_pfifo(), &fifo);
    }
}
#[test]
fn bus_v6_migra_sem_pfifo_preservando_boot_e_replay_com_zero_declarado() {
    let mut b = bus();
    b.reserve_boot_dma_page().unwrap();
    channel(&mut b);
    write(&mut b, 0x800040, 16);
    let current = saved(&b);
    let mut old = support::without_pfifo(&current).to_vec();
    old[16..20].copy_from_slice(&6u32.to_le_bytes());
    let mut dest = bus();
    write(&mut dest, 0x3240, 123);
    load(&mut dest, &old);
    assert_eq!(dest.nv2a_pfifo().profile(), PfifoProfile::LegacyUnmodeled);
    assert!(dest.boot_dma_page().reserved());
    dest.escrever_u32(GuestVirt(0xFD800040), 16).unwrap();
    assert_eq!(read(&dest, 0x800040), 0);
    assert_eq!(read(&dest, 0x800044), 0);
    assert_eq!(dest.telemetria().total_de_acessos(), 3);
    let checkpoint = saved(&dest);
    load(&mut b, &checkpoint);
    assert_eq!(saved(&b), checkpoint);
    assert_eq!(b.nv2a_pfifo().profile(), PfifoProfile::LegacyUnmodeled);
}
