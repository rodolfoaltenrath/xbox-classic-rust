#[cfg(test)]
mod boot_dma_verification {
    use xbox_core::{Snapshot,SnapshotReader,SnapshotWriter};
    use xbox_mem::{BOOT_DMA_ADDRESS,AlocadorDeFrames,GuestVirt,GuestPhys,Perm};
    use xbox_recomp::{Error,runtime::{Session,Exit}};
    mod guest {include!("boot_dma_guest.rs");}
    fn loaded()->Session{Session::load(include_bytes!("boot_dma_guest.xbe")).unwrap()}
    fn saved(s:&Session)->Vec<u8>{let mut w=SnapshotWriter::new();s.save(&mut w);w.finish()}
    pub(super) fn strip_gamma(bytes:&mut Vec<u8>){
        let at=bytes.windows(4).rposition(|v|v==b"GAMM").unwrap();
        assert_eq!(&bytes[at+4..at+8],&1u32.to_le_bytes());
        bytes.drain(at..at+xbox_mem::nv2a_gamma::SNAPSHOT_BYTES);
    }
    pub(super) fn strip_file_media(bytes:&mut Vec<u8>){
        let at=bytes.windows(4).rposition(|v|v==b"CDFS").unwrap();
        let version=u32::from_le_bytes(bytes[at+4..at+8].try_into().unwrap());assert!((1..=2).contains(&version));
        let count_at=match bytes[at+8] {0=>at+9,1=>at+13+u32::from_le_bytes(bytes[at+9..at+13].try_into().unwrap()) as usize,_=>panic!("presence")};
        let count=u32::from_le_bytes(bytes[count_at..count_at+4].try_into().unwrap()) as usize;
        // KRNL v15 (ADR-0090) grava SMBC logo depois de CDFS: abaixo de v14 nenhuma das duas existe.
        let mut end=count_at+4+count*16;
        if version>=2 {
            assert_eq!(&bytes[end..end+8],b"DIRE\x01\0\0\0");
            let dirs=u32::from_le_bytes(bytes[end+9..end+13].try_into().unwrap())as usize;end+=13;
            for _ in 0..dirs {let pattern=bytes[end+9];end+=10;if pattern==1{let n=u16::from_le_bytes(bytes[end..end+2].try_into().unwrap())as usize;end+=2+n;}else{assert_eq!(pattern,0);}}
        }
        let mut end=if bytes[end..].starts_with(b"SMBC"){end+SMBC_BYTES}else{end};
        if bytes[end..].starts_with(b"SEMA") {
            assert_eq!(&bytes[end+4..end+8],&1u32.to_le_bytes());
            let count=u32::from_le_bytes(bytes[end+8..end+12].try_into().unwrap())as usize;end+=12+count*12;
        }
        bytes.drain(at..end);
    }
    /// Seção SMBC v1: tag, versão, dois pares presença/valor e o contador u64.
    pub(super) const SMBC_BYTES:usize=4+4+2+2+8;
    pub(super) fn legacy_file_media(bytes:&[u8])->Vec<u8>{
        let mut result=bytes.to_vec();let at=result.windows(4).rposition(|v|v==b"CDFS").unwrap();
        let smbc=result.windows(4).rposition(|v|v==b"SMBC").map(|s|result[s..s+SMBC_BYTES].to_vec()).unwrap_or_default();
        let sema=result.windows(4).rposition(|v|v==b"SEMA").map(|s|{
            let n=u32::from_le_bytes(result[s+8..s+12].try_into().unwrap())as usize;result[s..s+12+n*12].to_vec()
        }).unwrap_or_else(||b"SEMA\x01\0\0\0\0\0\0\0".to_vec());
        strip_file_media(&mut result);let empty=[b'C',b'D',b'F',b'S',2,0,0,0,0,0,0,0,0,b'D',b'I',b'R',b'E',1,0,0,0,0,0,0,0,0];
        result.splice(at..at,empty.into_iter().chain(smbc).chain(sema));result
    }
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
        let mut bytes=saved(&s);strip_gamma(&mut bytes);let at=bytes.windows(4).position(|v|v==b"BUS0").unwrap();bytes[at+4..at+8].copy_from_slice(&5u32.to_le_bytes());
        let graph=bytes.windows(4).rposition(|v|v==b"PGRF").unwrap();let len=u64::from_le_bytes(bytes[graph+12..graph+20].try_into().unwrap()) as usize;bytes.drain(graph..graph+20+len);let at=bytes.windows(4).rposition(|v|v==b"PFFO").unwrap();let count=u32::from_le_bytes(bytes[at+12..at+16].try_into().unwrap()) as usize;let mut end=at+16+count*4;if u32::from_le_bytes(bytes[at+8..at+12].try_into().unwrap())==2{let len=u64::from_le_bytes(bytes[end..end+8].try_into().unwrap()) as usize;end+=8+len;}bytes.drain(at..end);
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
