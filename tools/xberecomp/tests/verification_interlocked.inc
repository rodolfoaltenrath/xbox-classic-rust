
#[cfg(test)]
mod interlocked_verification {
    use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
    use xbox_mem::{GuestVirt, Acesso, Perm};
    use xbox_recomp::runtime::{Session, Exit, ESP};
    mod exchange { include!("kernel_interlocked.rs"); }

    fn loaded() -> Session {
        let mut s=Session::load(include_bytes!("kernel_interlocked.xbe")).unwrap();
        s.bus.escrever_u32(GuestVirt(0x12100),0x11223344).unwrap();s
    }
    fn boundary(s:&mut Session) {
        assert_eq!(exchange::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::KernelCall {ordinal:54});
    }
    fn saved(s:&Session)->Vec<u8> {
        let mut out=SnapshotWriter::new();s.save(&mut out);out.finish()
    }
    fn finish(s:&mut Session) {
        s.atender_kernel().unwrap(); // segunda chamada já pendente.
        assert_eq!(s.state.regs[0],1);
        boundary(s);s.atender_kernel().unwrap(); // tail-call.
        assert_eq!(exchange::run(&mut s.state,&mut s.bus,0).unwrap(),Exit::Returned);
        assert_eq!(s.state.regs[0],0xAABBCCDD);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12100)).unwrap(),0xCAFEBABE);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12200)).unwrap(),0x11223344);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12204)).unwrap(),1);
    }

    #[test]
    fn fastcall_troca_retorna_valor_anterior_preserva_flags_e_tail_call() {
        let mut s=loaded();let esp=s.state.regs[ESP];boundary(&mut s);
        s.state.eflags=0xED7;let before=s.state.clone();let call=s.kernel_call().unwrap();
        s.atender_kernel().unwrap();
        let mut expected=before;expected.regs[0]=0x11223344;expected.regs[ESP]=esp;expected.eip=call.return_pc;
        assert_eq!(s.state,expected);assert_eq!(s.bus.ler_u32(GuestVirt(0x12100)).unwrap(),1);
        boundary(&mut s);finish(&mut s);
        assert_eq!(s.state.regs[ESP],esp+4);assert_eq!(s.state.eflags,0xED7);
        assert_eq!(s.kernel.atendidas(),3);
    }

    #[test]
    fn snapshot_preserva_troca_concluida_e_reexecuta_chamada_pendente() {
        let mut s=loaded();boundary(&mut s);s.atender_kernel().unwrap();boundary(&mut s);
        let checkpoint=saved(&s);finish(&mut s);let complete=saved(&s);
        let mut restored=loaded();let mut inp=SnapshotReader::new(&checkpoint).unwrap();
        Snapshot::load(&mut restored,&mut inp).unwrap();assert_eq!(inp.remaining(),0);
        assert_eq!(saved(&restored),checkpoint);finish(&mut restored);
        assert_eq!(saved(&restored),complete);
    }

    #[test]
    fn pagina_ro_recusa_mesmo_com_valor_igual_e_permite_retry_sem_efeito_parcial() {
        let mut s=loaded();boundary(&mut s);s.state.regs[2]=0x11223344;
        let frame=s.bus.traduzir(GuestVirt(0x12000),Acesso::Leitura).unwrap();
        s.bus.mapear(GuestVirt(0x12000),frame,1,Perm::RO).unwrap();let before=s.state.clone();
        assert!(s.atender_kernel().is_err());assert_eq!(s.state,before);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12100)).unwrap(),0x11223344);
        assert_eq!(s.kernel.atendidas(),0);
        s.bus.mapear(GuestVirt(0x12000),frame,1,Perm::RW).unwrap();s.state.regs[2]=1;
        s.atender_kernel().unwrap();boundary(&mut s);finish(&mut s);
    }

    #[test]
    fn target_sobre_retorno_usa_continuacao_capturada_antes_da_troca() {
        let mut s=loaded();boundary(&mut s);let esp=s.state.regs[ESP];let call=s.kernel_call().unwrap();
        s.state.regs[1]=esp;s.state.regs[2]=0x50000000;
        s.atender_kernel().unwrap();
        assert_eq!(s.state.regs[0],call.return_pc);
        assert_eq!(s.state.eip,call.return_pc);assert_eq!(s.state.regs[ESP],call.return_esp);
        assert_eq!(s.bus.ler_u32(GuestVirt(esp)).unwrap(),0x50000000);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12100)).unwrap(),0x11223344);
        boundary(&mut s);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12200)).unwrap(),call.return_pc);
    }

    #[test]
    fn retorno_invalido_recusa_antes_de_ler_trocar_ou_retornar_sucesso() {
        let mut s=loaded();boundary(&mut s);let esp=s.state.regs[ESP];let call=s.kernel_call().unwrap();
        s.bus.escrever_u32(GuestVirt(esp),0x50000000).unwrap();let before=s.state.clone();
        assert!(s.atender_kernel().is_err());assert_eq!(s.state,before);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12100)).unwrap(),0x11223344);
        assert_eq!(s.kernel.atendidas(),0);
        s.bus.escrever_u32(GuestVirt(esp),call.return_pc).unwrap();
        s.atender_kernel().unwrap();boundary(&mut s);finish(&mut s);
    }
}
