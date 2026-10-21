#[cfg(test)]
mod thread_verification {
    use xbox_core::snapshot::{Snapshot,SnapshotReader,SnapshotWriter};
    use xbox_kernel::dispatch::ThreadPhase;
    use xbox_mem::GuestVirt;
    use xbox_recomp::runtime::{Session,Exit,ESP};
    mod boot { include!("thread_boot.rs"); }
    mod ret { include!("thread_ret.rs"); }
    fn loaded()->Session {
        let mut s=Session::load(include_bytes!("thread_boot.xbe")).unwrap();
        s.state.regs[3]=0xFACE; s.bus.escrever_u32(GuestVirt(0x12018),0xDEADBEEF).unwrap();s
    }
    fn boundary(s:&mut Session,ordinal:u32) {
        assert_eq!(boot::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::KernelCall{ordinal});
    }
    fn saved(s:&Session)->Vec<u8> { let mut out=SnapshotWriter::new();s.save(&mut out);out.finish() }
    fn start(s:&mut Session) {
        boundary(s,255);s.atender_kernel().unwrap();assert!(s.has_parked_parent());
        assert_eq!(s.kernel.current_thread(),2);assert_eq!(s.state.eip,0x11080);
    }
    fn finish(s:&mut Session) {
        s.atender_kernel().unwrap(); // KeGetCurrentThread.
        boundary(s,258);let before=s.state.clone();s.atender_kernel().unwrap();
        assert!(!s.has_parked_parent());assert_eq!(s.kernel.current_thread(),1);
        assert_eq!(s.state.clock,before.clock);assert_eq!(s.state.retired,before.retired);
        assert_eq!(s.state.regs[3],0xFACE);assert_eq!(s.bus.ler_u32(GuestVirt(0x12100)).unwrap(),0x12345678);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12104)).unwrap(),0x12120);
        assert_eq!(s.kernel.child_thread().unwrap().phase,ThreadPhase::Terminated(42));
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12018)).unwrap(),0xDEADBEEF);
        boundary(s,233);s.atender_kernel().unwrap();
        assert_eq!(boot::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::Returned);
        assert!(!s.complete_thread_return().unwrap());assert_eq!(s.state.regs[0],0);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12018)).unwrap(),0);
    }
    #[test]
    fn system_routine_tls_guest_start_context_termino_e_pai_preservam_relogio_global() {
        let mut s=loaded();let fs=s.state.fs_base;let esp=s.state.regs[ESP];start(&mut s);
        let child=s.kernel.child_thread().unwrap();assert_eq!(s.state.fs_base,child.tib());
        assert_eq!(s.bus.ler_u32(GuestVirt(child.tls())).unwrap(),0);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12014)).unwrap(),2);
        boundary(&mut s,104);finish(&mut s);assert_eq!(s.state.fs_base,fs);assert_eq!(s.state.regs[ESP],esp+4);
    }
    #[test]
    fn snapshot_da_filha_em_chamada_pendente_restaura_pai_e_resultado_integral() {
        let mut s=loaded();start(&mut s);boundary(&mut s,104);let checkpoint=saved(&s);
        finish(&mut s);let complete=saved(&s);let mut restored=loaded();
        let mut inp=SnapshotReader::new(&checkpoint).unwrap();Snapshot::load(&mut restored,&mut inp).unwrap();
        assert_eq!(inp.remaining(),0);assert!(saved(&restored)==checkpoint);
        finish(&mut restored);assert!(saved(&restored)==complete);
        let mut corrupt=checkpoint;let at=corrupt.windows(4).rposition(|v|v==b"TCXT").unwrap();
        corrupt[at+8+8*4..at+8+9*4].copy_from_slice(&0x50000000u32.to_le_bytes());
        assert!(Snapshot::load(&mut loaded(),&mut SnapshotReader::new(&corrupt).unwrap()).is_err());
    }
    #[test]
    fn criacao_recusada_preserva_cpu_frames_saidas_e_nao_estaciona_pai() {
        let mut s=loaded();boundary(&mut s,255);let esp=s.state.regs[ESP];let used=s.bus.frames().usados();
        s.bus.escrever_u32(GuestVirt(esp+32),1).unwrap(); // CreateSuspended.
        let before=s.state.clone();assert!(s.atender_kernel().is_err());assert_eq!(s.state,before);
        assert_eq!(s.bus.frames().usados(),used);assert!(!s.has_parked_parent());assert!(s.kernel.child_thread().is_none());
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12010)).unwrap(),0);
        s.bus.escrever_u32(GuestVirt(esp+32),0).unwrap();
        s.bus.escrever_u32(GuestVirt(esp),0x50000000).unwrap();assert!(s.atender_kernel().is_err());
        assert_eq!(s.state,before);assert_eq!(s.bus.frames().usados(),used);
    }
    #[test]
    fn saida_sobre_retorno_da_criacao_usa_continuacao_capturada_do_pai() {
        let mut s=loaded();boundary(&mut s,255);let call=s.kernel_call().unwrap();let esp=s.state.regs[ESP];
        s.bus.escrever_u32(GuestVirt(esp+4),esp).unwrap();s.atender_kernel().unwrap();
        assert_eq!(s.bus.ler_u32(GuestVirt(esp)).unwrap(),s.kernel.child_thread().unwrap().handle);
        boundary(&mut s,104);s.atender_kernel().unwrap();boundary(&mut s,258);s.atender_kernel().unwrap();
        assert_eq!(s.state.eip,call.return_pc);assert_eq!(s.state.regs[ESP],call.return_esp);
    }
    #[test]
    fn ret_do_system_routine_termina_apenas_filha_e_retorna_ao_pai() {
        let mut s=Session::load(include_bytes!("thread_ret.xbe")).unwrap();
        assert_eq!(ret::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::KernelCall{ordinal:255});s.atender_kernel().unwrap();
        assert_eq!(ret::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::Returned);
        assert!(s.complete_thread_return().unwrap());assert!(!s.state.returned);
        assert_eq!(s.kernel.child_thread().unwrap().phase,ThreadPhase::Terminated(9));
        assert_eq!(ret::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::KernelCall{ordinal:233});s.atender_kernel().unwrap();
        assert_eq!(ret::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::Returned);assert!(!s.complete_thread_return().unwrap());
    }
    #[test]
    fn aots_v1_krnl_v6_migra_sem_thread_e_pode_criar_filha() {
        let mut s=loaded();let mut old=saved(&s);let t=old.windows(4).rposition(|tag|tag==b"THRD").unwrap();old.truncate(t);
        for (tag,version) in [(b"AOTS",1u32),(b"KRNL",6),(b"KMUT",2)] {
            let at=old.windows(4).position(|v|v==tag).unwrap();old[at+4..at+8].copy_from_slice(&version.to_le_bytes());
        }
        Snapshot::load(&mut s,&mut SnapshotReader::new(&old).unwrap()).unwrap();start(&mut s);boundary(&mut s,104);finish(&mut s);
    }
}
