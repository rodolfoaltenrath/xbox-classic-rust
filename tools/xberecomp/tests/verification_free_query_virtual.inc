#[cfg(test)]
mod free_query_virtual_verification {
    use xbox_core::{Snapshot,SnapshotReader,SnapshotWriter};
    use xbox_mem::GuestVirt;
    use xbox_recomp::runtime::{Session,Exit,ESP};
    mod guest {include!("kernel_free_query_virtual.rs");}
    fn loaded()->Session {let mut s=Session::load(include_bytes!("kernel_free_query_virtual.xbe")).unwrap();s.bus.escrever_u32(GuestVirt(0x12104),4097).unwrap();s}
    fn saved(s:&impl Snapshot)->Vec<u8> {let mut w=SnapshotWriter::new();s.save(&mut w);w.finish()}
    fn boundary(s:&mut Session,ordinal:u32) {assert_eq!(guest::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::KernelCall{ordinal});}
    fn handle(s:&mut Session) {
        let call=s.kernel_call().unwrap();let mut expected=s.state.clone();expected.eip=call.return_pc;
        expected.regs[ESP]=call.return_esp;expected.regs[0]=0;s.atender_kernel().unwrap();assert_eq!(s.state,expected);
    }
    fn pending_free()->Session {
        let mut s=loaded();boundary(&mut s,184);handle(&mut s);boundary(&mut s,217);s.state.eflags=0xED7;handle(&mut s);
        let base=s.state.regs[3];let expected=[base,base,4,8192,0x1000,4,0x20000];
        for(i,v)in expected.into_iter().enumerate(){assert_eq!(s.bus.ler_u32(GuestVirt(0x12200+i as u32*4)).unwrap(),v);}
        boundary(&mut s,199);s
    }
    fn finish(s:&mut Session) {
        let base=s.state.regs[3];handle(s);assert!(s.bus.mmu().consultar(GuestVirt(base)).is_none());
        assert!(s.kernel.virtual_memory().allocations().is_empty());assert_eq!(s.bus.ler_u32(GuestVirt(0x12104)).unwrap(),8192);
        assert_eq!(guest::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::Returned);
    }
    #[test]
    fn xbox_query_and_release_abi_and_full_snapshot_replay() {
        let mut s=pending_free();let checkpoint=saved(&s);finish(&mut s);let complete=saved(&s);
        let mut restored=loaded();restored.load(&mut SnapshotReader::new(&checkpoint).unwrap()).unwrap();
        assert!(saved(&restored)==checkpoint);finish(&mut restored);assert!(saved(&restored)==complete);
    }
    #[test]
    fn release_self_output_refuses_without_cpu_or_resource_mutation_and_resumes() {
        let mut s=pending_free();let base=s.state.regs[3];let esp=s.state.regs[ESP];
        s.bus.escrever_u32(GuestVirt(base),base).unwrap();s.bus.escrever_u32(GuestVirt(base+4),0).unwrap();
        s.bus.escrever_u32(GuestVirt(esp+4),base).unwrap();s.bus.escrever_u32(GuestVirt(esp+8),base+4).unwrap();
        let cpu=s.state.clone();let frames=saved(s.bus.frames());let regions=saved(s.kernel.virtual_memory());
        assert!(s.atender_kernel().is_err());assert_eq!(s.state,cpu);assert_eq!(saved(s.bus.frames()),frames);assert_eq!(saved(s.kernel.virtual_memory()),regions);
        s.bus.escrever_u32(GuestVirt(esp+4),0x12100).unwrap();s.bus.escrever_u32(GuestVirt(esp+8),0x12104).unwrap();finish(&mut s);
    }
}
