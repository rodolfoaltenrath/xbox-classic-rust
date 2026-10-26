#[cfg(test)]
mod gpu_instance_verification {
    use xbox_core::{Snapshot,SnapshotReader,SnapshotWriter};
    use xbox_mem::GuestVirt;
    use xbox_recomp::runtime::{Session,Exit,ESP};
    mod guest {include!("gpu_instance.rs");}
    fn loaded()->Session {Session::load(include_bytes!("gpu_instance.xbe")).unwrap()}
    fn boundary(s:&mut Session){assert_eq!(guest::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::KernelCall{ordinal:168});}
    fn saved(s:&Session)->Vec<u8>{let mut w=SnapshotWriter::new();s.save(&mut w);w.finish()}
    fn attend(s:&mut Session){
        let call=s.kernel_call().unwrap();let before=s.state.clone();s.atender_kernel().unwrap();
        let mut expected=before;expected.regs[0]=0x83FF0000;expected.regs[ESP]=call.return_esp;expected.eip=call.return_pc;
        assert_eq!(s.state,expected);
    }
    fn finish(s:&mut Session){attend(s);assert_eq!(guest::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::Returned);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x83FEB000)).unwrap(),0xCAFEBABE);assert_eq!(s.kernel.atendidas(),2);}
    #[test]
    fn stdcall_consulta_repetida_preserva_bytes_clock_flags_e_pilha(){
        let mut s=loaded();s.state.eflags=0xED7;let esp=s.state.regs[ESP];boundary(&mut s);let used=s.bus.frames().usados();
        attend(&mut s);assert_eq!(s.state.regs[ESP],esp);assert_eq!(s.bus.frames().usados(),used+32);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12100)).unwrap(),65536);boundary(&mut s);finish(&mut s);
        assert_eq!(s.bus.frames().usados(),used+32);assert_eq!(s.state.regs[ESP],esp+4);assert_eq!(s.state.eflags,0xED7);
    }
    #[test]
    fn snapshot_replay_e_migracao_krnl_v12_preservam_ou_provisionam_pool(){
        let mut s=loaded();boundary(&mut s);let mut old=saved(&s);super::boot_dma_verification::strip_file_media(&mut old);
        let at=old.windows(4).rposition(|v|v==b"KRNL").unwrap();old[at+4..at+8].copy_from_slice(&12u32.to_le_bytes());let at=old.windows(4).rposition(|v|v==b"GINS").unwrap();old.drain(at..at+9);
        let mut migrated=loaded();Snapshot::load(&mut migrated,&mut SnapshotReader::new(&old).unwrap()).unwrap();
        assert!(!migrated.kernel.gpu_instance_memory().provisioned());attend(&mut migrated);boundary(&mut migrated);finish(&mut migrated);
        attend(&mut s);boundary(&mut s);let checkpoint=saved(&s);finish(&mut s);let complete=saved(&s);
        assert!(saved(&migrated)==complete);let mut restored=loaded();Snapshot::load(&mut restored,&mut SnapshotReader::new(&checkpoint).unwrap()).unwrap();
        assert!(saved(&restored)==checkpoint);finish(&mut restored);assert!(saved(&restored)==complete);
    }
    #[test]
    fn retorno_invalido_recusa_antes_da_reserva_e_saida_sobre_retorno_usa_continuacao(){
        let mut s=loaded();boundary(&mut s);let esp=s.state.regs[ESP];let call=s.kernel_call().unwrap();let used=s.bus.frames().usados();
        s.bus.escrever_u32(GuestVirt(esp),0x50000000).unwrap();let before=s.state.clone();assert!(s.atender_kernel().is_err());assert_eq!(s.state,before);
        assert_eq!(s.bus.frames().usados(),used);assert!(!s.kernel.gpu_instance_memory().provisioned());
        s.bus.escrever_u32(GuestVirt(esp),call.return_pc).unwrap();s.bus.escrever_u32(GuestVirt(esp+8),esp).unwrap();
        attend(&mut s);assert_eq!(s.bus.ler_u32(GuestVirt(esp)).unwrap(),65536);assert_eq!(s.state.eip,call.return_pc);boundary(&mut s);finish(&mut s);
    }
}
