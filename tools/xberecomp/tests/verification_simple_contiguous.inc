#[cfg(test)]
mod simple_contiguous_verification {
    use xbox_core::{Snapshot, SnapshotReader, SnapshotWriter};
    use xbox_mem::GuestVirt;
    use xbox_recomp::runtime::{Session, Exit, ESP};
    mod guest { include!("kernel_simple_contiguous.rs"); }
    fn loaded() -> Session {Session::load(include_bytes!("kernel_simple_contiguous.xbe")).unwrap()}
    fn saved(s: &impl Snapshot) -> Vec<u8> {let mut w=SnapshotWriter::new();s.save(&mut w);w.finish()}
    fn boundary(s: &mut Session, ordinal: u32) {
        assert_eq!(guest::run(&mut s.state,&mut s.bus,100).unwrap(), Exit::KernelCall{ordinal});
    }
    fn handle(s: &mut Session, result: u32) {
        let call=s.kernel_call().unwrap(); let mut expected=s.state.clone();
        expected.eip=call.return_pc;expected.regs[ESP]=call.return_esp;expected.regs[0]=result;
        s.atender_kernel().unwrap(); assert_eq!(s.state,expected);
    }
    fn finish(s: &mut Session, base: u32) {
        handle(s,8192); boundary(s,171);handle(s,0);
        assert_eq!(guest::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::Returned);
        assert_eq!(s.state.regs[2],8192);assert!(s.bus.mmu().consultar(GuestVirt(base)).is_none());
        assert!(s.kernel.contiguous_memory().allocations().is_empty());
    }
    #[test]
    fn real_allocation_query_free_abi_and_full_session_replay() {
        let mut s=loaded(); let stack=s.state.regs[ESP]; boundary(&mut s,165);
        s.state.eflags=0xED7;let call=s.kernel_call().unwrap();assert_eq!(call.return_esp,stack);
        s.atender_kernel().unwrap();let base=s.state.regs[0];assert_ne!(base,0);assert_eq!(s.state.eflags,0xED7);
        boundary(&mut s,180);assert_eq!(s.bus.ler_u32(GuestVirt(base)).unwrap(),0xCAFEBABE);
        let checkpoint=saved(&s);finish(&mut s,base);let complete=saved(&s);
        let mut restored=loaded();restored.load(&mut SnapshotReader::new(&checkpoint).unwrap()).unwrap();
        assert!(saved(&restored)==checkpoint);finish(&mut restored,base);assert!(saved(&restored)==complete);
    }
    #[test]
    fn query_interior_refuses_without_changing_cpu_or_resource_then_resumes() {
        let mut s=loaded();boundary(&mut s,165);s.atender_kernel().unwrap();let base=s.state.regs[0];boundary(&mut s,180);
        let argument=s.state.regs[ESP]+4;s.bus.escrever_u32(GuestVirt(argument),base+4096).unwrap();
        let cpu=s.state.clone();let frames=saved(s.bus.frames());let cmem=saved(s.kernel.contiguous_memory());
        assert!(s.atender_kernel().is_err());assert_eq!(s.state,cpu);assert_eq!(saved(s.bus.frames()),frames);
        assert_eq!(saved(s.kernel.contiguous_memory()),cmem);
        s.bus.escrever_u32(GuestVirt(argument),base).unwrap();finish(&mut s,base);
    }
}
