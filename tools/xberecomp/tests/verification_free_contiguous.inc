#[cfg(test)]
mod free_contiguous_verification {
    use xbox_core::{Snapshot,SnapshotReader,SnapshotWriter};
    use xbox_mem::{Acesso,GuestPhys,GuestVirt};
    use xbox_recomp::runtime::{Session,Exit,ESP};
    mod guest {include!("kernel_free_contiguous.rs");}
    fn loaded()->Session {Session::load(include_bytes!("kernel_free_contiguous.xbe")).unwrap()}
    fn saved(s:&impl Snapshot)->Vec<u8> {let mut w=SnapshotWriter::new();s.save(&mut w);w.finish()}
    fn boundary(s:&mut Session,ordinal:u32) {assert_eq!(guest::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::KernelCall{ordinal});}
    fn handle(s:&mut Session,eax:u32) {
        let call=s.kernel_call().unwrap();let mut expected=s.state.clone();s.atender_kernel().unwrap();
        expected.eip=call.return_pc;expected.regs[ESP]=call.return_esp;expected.regs[0]=eax;assert_eq!(s.state,expected);
    }
    fn pending_free()->Session {let mut s=loaded();boundary(&mut s,166);handle(&mut s,0x80020000);boundary(&mut s,171);s}
    fn freed(s:&Session) {
        assert!(s.kernel.contiguous_memory().allocations().is_empty());
        for pa in [0x20000,0x21000] {
            assert!(!s.bus.frames().em_uso(pa/4096));assert!(s.bus.mmu().consultar(GuestVirt(0x80000000+pa)).is_none());
        }
        assert_eq!(s.bus.dma(GuestPhys(0x20000),4).unwrap(),&0xCAFEBABEu32.to_le_bytes());
        assert!(s.bus.boot_dma_page().reserved());s.bus.boot_dma_page().validate(&s.bus).unwrap();
    }
    fn finish(s:&mut Session) {
        boundary(s,166);handle(s,0x80020000);
        assert!(s.bus.dma(GuestPhys(0x20000),8192).unwrap().iter().all(|&v|v==0));
        assert_eq!(guest::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::Returned);assert_eq!(s.state.regs[2],0);
    }
    #[test]
    fn stdcall_free_desmapeia_devolve_frames_e_reuso_zera_sem_mudar_flags_ou_clock() {
        let mut s=pending_free();let used=s.bus.frames().usados();s.state.eflags=0xED7;
        handle(&mut s,0);assert_eq!(s.bus.frames().usados(),used-2);freed(&s);
        assert!(s.bus.traduzir(GuestVirt(0x80020000),Acesso::Leitura).is_err());finish(&mut s);assert_eq!(s.state.eflags,0xED7);
    }
    #[test]
    fn snapshot_antes_e_depois_de_free_reproduz_reuso_integral() {
        let mut s=pending_free();let before=saved(&s);handle(&mut s,0);freed(&s);let after=saved(&s);finish(&mut s);let complete=saved(&s);
        for (checkpoint,needs_free) in [(before,true),(after,false)] {
            let mut restored=loaded();restored.load(&mut SnapshotReader::new(&checkpoint).unwrap()).unwrap();assert!(saved(&restored)==checkpoint);
            if needs_free {handle(&mut restored,0);}freed(&restored);finish(&mut restored);assert!(saved(&restored)==complete);
        }
    }
    #[test]
    fn retorno_invalido_e_base_interior_preservam_alocacao_e_cpu_pendente() {
        let mut s=pending_free();let call=s.kernel_call().unwrap();let esp=s.state.regs[ESP];
        for invalid_return in [true,false] {
            s.bus.escrever_u32(GuestVirt(esp),if invalid_return {0x50000000}else{call.return_pc}).unwrap();
            s.bus.escrever_u32(GuestVirt(esp+4),if invalid_return {0x80020000}else{0x80021000}).unwrap();
            let cpu=s.state.clone();let frames=saved(s.bus.frames());let mmu=saved(s.bus.mmu());let cmem=saved(s.kernel.contiguous_memory());
            assert!(s.atender_kernel().is_err());assert_eq!(s.state,cpu);assert_eq!(saved(s.bus.frames()),frames);assert_eq!(saved(s.bus.mmu()),mmu);assert_eq!(saved(s.kernel.contiguous_memory()),cmem);
            assert_eq!(s.bus.ler_u32(GuestVirt(0x80020000)).unwrap(),0xCAFEBABE);
        }
        s.bus.escrever_u32(GuestVirt(esp+4),0x80020000).unwrap();handle(&mut s,0);freed(&s);finish(&mut s);
    }
}
