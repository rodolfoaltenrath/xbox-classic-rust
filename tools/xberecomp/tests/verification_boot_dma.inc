#[cfg(test)]
mod boot_dma_verification {
    use xbox_core::{Snapshot,SnapshotReader,SnapshotWriter};
    use xbox_mem::{BOOT_DMA_ADDRESS,AlocadorDeFrames,GuestVirt,GuestPhys,Perm};
    use xbox_recomp::{Error,runtime::{Session,Exit}};
    mod guest {include!("boot_dma_guest.rs");}
    fn loaded()->Session{Session::load(include_bytes!("boot_dma_guest.xbe")).unwrap()}
    fn saved(s:&Session)->Vec<u8>{let mut w=SnapshotWriter::new();s.save(&mut w);w.finish()}
    fn pending(s:&mut Session){assert_eq!(guest::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::KernelCall{ordinal:1});}
    fn finish(s:&mut Session){s.atender_kernel().unwrap();assert_eq!(guest::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::Returned);}
    // Fixture do layout físico anterior: remove a reserva e desloca os frames
    // de imagem/startup de volta. Não basta remover BTPG mantendo a MMU moderna.
    pub(super) fn legacy_snapshot(checkpoint:&[u8],image:&[u8])->Vec<u8>{
        let mut s=Session::load(image).unwrap();Snapshot::load(&mut s,&mut SnapshotReader::new(checkpoint).unwrap()).unwrap();
        assert!(s.kernel.contiguous_memory().allocations().is_empty());assert!(s.kernel.virtual_memory().allocations().is_empty());
        assert!(s.bus.boot_dma_page().reserved());let old_frames=s.bus.frames().clone();
        let mut ram=s.bus.arena().bytes()[4096..].to_vec();ram.resize(s.bus.ram(),0);s.bus.carregar_em_fisico(GuestPhys(0),&ram).unwrap();
        let mappings:Vec<_>=s.bus.mmu().mapeamentos().collect();
        for(va,pa,perm)in mappings{
            if va==BOOT_DMA_ADDRESS {assert_eq!(pa,GuestPhys(0));s.bus.mmu_mut().desmapear(va,1);}
            else if (pa.0 as usize)<s.bus.ram(){assert!(pa.0>=4096);s.bus.mapear(va,GuestPhys(pa.0-4096),1,perm).unwrap();}
        }
        let mut frames=AlocadorDeFrames::novo(s.bus.ram());
        for frame in 1..old_frames.total(){if old_frames.em_uso(frame){frames.reservar(GuestPhys((frame-1)*4096),1).unwrap();}}
        *s.bus.frames_mut()=frames;
        let mut bytes=saved(&s);let at=bytes.windows(4).position(|v|v==b"BUS0").unwrap();bytes[at+4..at+8].copy_from_slice(&5u32.to_le_bytes());
        let at=bytes.windows(4).position(|v|v==b"BTPG").unwrap();bytes.drain(at..at+9);bytes
    }
    #[test]
    fn guest_escreve_boot_dma_com_wbinvd_sem_modificar_headers_flags_ou_relogio(){
        let mut s=loaded();s.state.eflags=0xED7;let used=s.bus.frames().usados();let headers=s.bus.dma(GuestPhys(4096),4096).unwrap().to_vec();
        pending(&mut s);finish(&mut s);assert_eq!(s.bus.ler_u32(GuestVirt(0x12100)).unwrap(),0x12345001);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12104)).unwrap(),u32::MAX);assert_eq!(s.bus.dma(GuestPhys(0),4).unwrap(),&u32::MAX.to_le_bytes());
        assert_eq!(s.bus.dma(GuestPhys(4096),4096).unwrap(),headers);assert_eq!(s.bus.mmu().consultar(GuestVirt(0x10000)),Some((GuestPhys(4096),Perm::RO)));
        assert_eq!(s.bus.frames().usados(),used);assert_eq!(s.state.eflags,0xED7);assert_eq!(s.state.retired,10);assert_eq!(s.state.clock.cycles(),10);
    }
    #[test]
    fn snapshot_replay_preserva_boot_dma_e_continuacao(){
        let mut s=loaded();pending(&mut s);let checkpoint=saved(&s);finish(&mut s);let complete=saved(&s);
        let mut restored=loaded();Snapshot::load(&mut restored,&mut SnapshotReader::new(&checkpoint).unwrap()).unwrap();
        assert!(saved(&restored)==checkpoint);finish(&mut restored);assert!(saved(&restored)==complete);
    }
    #[test]
    fn bus_v5_reproduz_layout_anterior_e_falta_dma_sem_criar_alias_dos_headers(){
        let s=loaded();let old=legacy_snapshot(&saved(&s),include_bytes!("boot_dma_guest.xbe"));let mut dest=loaded();
        Snapshot::load(&mut dest,&mut SnapshotReader::new(&old).unwrap()).unwrap();assert!(!dest.bus.boot_dma_page().reserved());
        assert_eq!(dest.bus.mmu().consultar(GuestVirt(0x10000)),Some((GuestPhys(0),Perm::RO)));
        assert!(dest.bus.mmu().consultar(BOOT_DMA_ADDRESS).is_none());assert_eq!(dest.bus.dma(GuestPhys(0),4).unwrap(),b"XBEH");
        assert_eq!(guest::run(&mut dest.state,&mut dest.bus,1).unwrap(),Exit::Budget);let cpu=dest.state.clone();
        assert!(matches!(guest::run(&mut dest.state,&mut dest.bus,1),Err(Error::Memory(_))));assert_eq!(dest.state,cpu);
        let checkpoint=saved(&dest);Snapshot::load(&mut dest,&mut SnapshotReader::new(&checkpoint).unwrap()).unwrap();assert!(saved(&dest)==checkpoint);
    }
}
