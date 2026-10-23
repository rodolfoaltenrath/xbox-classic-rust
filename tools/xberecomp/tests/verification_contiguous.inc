#[cfg(test)]
mod contiguous_verification {
    use xbox_core::{Snapshot,SnapshotReader,SnapshotWriter};
    use xbox_mem::{Acesso,GuestPhys,GuestVirt};
    use xbox_recomp::runtime::{Session,Exit,ESP};
    mod guest {include!("kernel_contiguous.rs");}
    fn loaded()->Session {Session::load(include_bytes!("kernel_contiguous.xbe")).unwrap()}
    fn boundary(s:&mut Session) {assert_eq!(guest::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::KernelCall{ordinal:166});}
    fn handle(s:&mut Session,address:u32) {
        let call=s.kernel_call().unwrap();let mut expected=s.state.clone();s.atender_kernel().unwrap();
        expected.eip=call.return_pc;expected.regs[ESP]=call.return_esp;expected.regs[0]=address;assert_eq!(s.state,expected);
    }
    fn saved(s:&Session)->Vec<u8> {let mut out=SnapshotWriter::new();s.save(&mut out);out.finish()}
    fn finish(s:&mut Session) {
        handle(s,0x80040000);assert_eq!(guest::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::Returned);
        assert_eq!(s.state.regs[0],0x80040000);assert_eq!(s.bus.ler_u32(GuestVirt(0x12100)).unwrap(),0x80020000);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12104)).unwrap(),0x20000);
        assert_eq!(s.bus.dma(GuestPhys(0x20FFE),4).unwrap(),&0xCAFEBABEu32.to_le_bytes());
        assert_eq!(s.kernel.contiguous_memory().allocations().len(),2);
    }
    #[test]
    fn memoria_fisica_linear_dma_e_stdcall_preservam_cpu_flags_relogio() {
        let mut s=loaded();let esp=s.state.regs[ESP];s.state.eflags=0xED7;
        boundary(&mut s);let used=s.bus.frames().usados();handle(&mut s,0x80020000);
        assert_eq!(s.bus.frames().usados(),used+3);assert!(s.bus.mmu().consultar(GuestVirt(0x20000)).is_none());
        assert!(s.bus.traduzir(GuestVirt(0x80020000),Acesso::Execucao).is_err());
        boundary(&mut s);let flags=s.state.eflags;finish(&mut s);
        assert_eq!(s.state.regs[ESP],esp+4);assert_eq!(s.state.eflags,flags);assert_eq!(s.bus.frames().usados(),used+4);
    }
    #[test]
    fn snapshot_preserva_frames_dados_e_segunda_alocacao_e_migra_sem_recursos() {
        let mut s=loaded();let initial=saved(&s);boundary(&mut s);handle(&mut s,0x80020000);boundary(&mut s);
        let checkpoint=saved(&s);finish(&mut s);let complete=saved(&s);let mut restored=loaded();
        Snapshot::load(&mut restored,&mut SnapshotReader::new(&checkpoint).unwrap()).unwrap();assert!(saved(&restored)==checkpoint);
        finish(&mut restored);assert!(saved(&restored)==complete);
        let at=initial.windows(4).rposition(|v|v==b"CMEM").unwrap();let mut old=initial[..at].to_vec();old.push(0);
        let k=old.windows(4).position(|v|v==b"KRNL").unwrap();old[k+4..k+8].copy_from_slice(&9u32.to_le_bytes());let aots=old.windows(4).position(|v|v==b"AOTS").unwrap();old[aots+4..aots+8].copy_from_slice(&2u32.to_le_bytes());
        Snapshot::load(&mut restored,&mut SnapshotReader::new(&old).unwrap()).unwrap();assert!(restored.kernel.contiguous_memory().allocations().is_empty());
        boundary(&mut restored);handle(&mut restored,0x80020000);boundary(&mut restored);finish(&mut restored);assert_eq!(restored.scheduling_profile(),xbox_recomp::runtime::SchedulingProfile::LegacyBootstrap);assert!(saved(&restored)==super::legacy_scheduler_snapshot(&complete));
        // Remover só os registros mantendo o Bus não pode esconder a janela possuída.
        let mut orphan=checkpoint;let at=orphan.windows(4).rposition(|v|v==b"CMEM").unwrap();
        orphan.drain(at+12..at+28);orphan[at+8..at+12].copy_from_slice(&0u32.to_le_bytes());
        assert!(Snapshot::load(&mut loaded(),&mut SnapshotReader::new(&orphan).unwrap()).is_err());
    }
    #[test]
    fn recusas_retorno_invalido_e_oom_preservam_recursos_e_entrada_pendente() {
        let mut s=loaded();boundary(&mut s);let esp=s.state.regs[ESP];let used=s.bus.frames().usados();
        s.bus.escrever_u32(GuestVirt(esp+20),0x40).unwrap();let before=s.state.clone();assert!(s.atender_kernel().is_err());assert_eq!(s.state,before);
        assert_eq!(s.bus.frames().usados(),used);assert!(s.kernel.contiguous_memory().allocations().is_empty());
        s.bus.escrever_u32(GuestVirt(esp+20),0x404).unwrap();let call=s.kernel_call().unwrap();
        s.bus.escrever_u32(GuestVirt(esp),0x50000000).unwrap();assert!(s.atender_kernel().is_err());assert_eq!(s.state,before);
        s.bus.escrever_u32(GuestVirt(esp),call.return_pc).unwrap();s.bus.escrever_u32(GuestVirt(esp+4),128*1024*1024).unwrap();
        handle(&mut s,0);assert_eq!(s.bus.frames().usados(),used);assert!(s.kernel.contiguous_memory().allocations().is_empty());
    }
}
