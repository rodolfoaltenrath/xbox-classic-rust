use xbox_core::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_formats::fixtures::xbe_com_imports;
use xbox_kernel::{dispatch::Dispatcher, imports, loader::Image};
use xbox_mem::{Acesso, BOOT_DMA_ADDRESS, GuestPhys, GuestVirt, Perm};
fn setup() -> (xbox_mem::Bus, Dispatcher, [u32; 8]) {
    let xbe = xbe_com_imports(&[0xC3], &[166]).unwrap();
    let s = Image::prepare_aot(&xbe).unwrap();
    let mut regs = [0; 8];
    regs[4] = s.esp - 32;
    (
        s.image.bus,
        Dispatcher::for_image(0x10000, 0x3000).unwrap(),
        regs,
    )
}
fn saved(d: &Dispatcher) -> Vec<u8> {
    let mut w = SnapshotWriter::new();
    d.save(&mut w);
    w.finish()
}
#[test]
fn headers_e_codigo_ficam_fora_da_pagina_dma_e_alocador_nao_a_reutiliza() {
    let (mut b, mut d, r) = setup();
    assert!(b.boot_dma_page().reserved());
    assert_eq!(
        b.mmu().consultar(GuestVirt(0x10000)),
        Some((GuestPhys(4096), Perm::RO))
    );
    let headers = b.dma(GuestPhys(4096), 4096).unwrap().to_vec();
    b.escrever_u32(BOOT_DMA_ADDRESS, 0x12345001).unwrap();
    assert_eq!(b.dma(GuestPhys(4096), 4096).unwrap(), headers);
    assert_eq!(
        b.ler_u32(GuestVirt(0x10000)).unwrap(),
        u32::from_le_bytes(*b"XBEH")
    );
    assert!(b.traduzir(BOOT_DMA_ADDRESS, Acesso::Execucao).is_err());
    for (i, v) in [0x11000, 4096, 0, 4095, 4096, 4].into_iter().enumerate() {
        b.escrever_u32(GuestVirt(r[4] + i as u32 * 4), v).unwrap();
    }
    assert_eq!(
        d.atender(&mut b, imports::trampoline(166), &r).unwrap().eax,
        0
    );
    assert_eq!(d.contiguous_memory().allocations(), []);
    d.restore_memory_context(None, &b).unwrap();
}
#[test]
fn snapshot_e_atendimento_recusam_cmem_que_reivindica_frame_do_boot() {
    let (mut b, mut d, r) = setup();
    for (i, v) in [0x11000, 4096, 0x20000, 0x20FFF, 4096, 4]
        .into_iter()
        .enumerate()
    {
        b.escrever_u32(GuestVirt(r[4] + i as u32 * 4), v).unwrap();
    }
    assert_eq!(
        d.atender(&mut b, imports::trampoline(166), &r).unwrap().eax,
        0x80020000
    );
    let mut bad = saved(&d);
    let at = bad.windows(4).position(|x| x == b"CMEM").unwrap();
    bad[at + 12..at + 16].copy_from_slice(&0u32.to_le_bytes());
    let mut restored = Dispatcher::new();
    restored
        .load(&mut SnapshotReader::new(&bad).unwrap())
        .unwrap();
    assert!(restored.restore_memory_context(None, &b).is_err());
    assert!(
        restored
            .atender(&mut b, imports::trampoline(166), &r)
            .is_err()
    );
}
