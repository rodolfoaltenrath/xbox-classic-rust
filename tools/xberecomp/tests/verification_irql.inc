#[cfg(test)]
mod irql_verification {
    use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
    use xbox_kernel::dispatch::{Dispatcher, Irql};
    use xbox_mem::GuestVirt;
    use xbox_recomp::runtime::{Session, Exit, ESP, INTERRUPT_FLAG};
    mod query { include!("kernel_irql.rs"); }

    fn loaded(level: Irql) -> Session {
        let mut s=Session::load(include_bytes!("kernel_irql.xbe")).unwrap();
        s.kernel=Dispatcher::with_irql(level);s.state.eflags=0xED7;s
    }
    fn boundary(s:&mut Session) {
        assert_eq!(query::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::KernelCall {ordinal:103});
    }
    fn saved(s:&Session)->Vec<u8> {
        let mut out=SnapshotWriter::new();s.save(&mut out);out.finish()
    }
    fn attend(s:&mut Session) {
        let call=s.kernel_call().unwrap();let before=s.state.clone();
        s.atender_kernel().unwrap();
        let mut expected=before;expected.regs[0]=u32::from(s.kernel.irql().level());
        expected.regs[ESP]=call.return_esp;expected.eip=call.return_pc;
        expected.returned=call.return_pc==xbox_recomp::runtime::RETURN_SENTINEL;
        assert_eq!(s.state,expected);
    }
    fn finish(s:&mut Session) {
        attend(s);boundary(s);attend(s);
        assert_eq!(query::run(&mut s.state,&mut s.bus,0).unwrap(),Exit::Returned);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12100)).unwrap(),u32::from(s.kernel.irql().level()));
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12104)).unwrap(),u32::from(s.kernel.irql().level()));
        assert_eq!(s.kernel.atendidas(),3);
    }

    #[test]
    fn consulta_stdcall_sem_argumentos_preserva_cpu_e_retorna_nivel_independente_de_cli_if() {
        for level in [Irql::Passive,Irql::Apc,Irql::Dispatch] {
            let mut s=loaded(level);let esp=s.state.regs[ESP];boundary(&mut s);
            assert_ne!(s.state.eflags&INTERRUPT_FLAG,0);attend(&mut s);
            assert_eq!(s.state.regs[ESP],esp);boundary(&mut s);
            assert_eq!(s.state.eflags&INTERRUPT_FLAG,0);assert_eq!(s.kernel.irql(),level);
            let before=s.state.clone();
            for _ in 0..3 { boundary(&mut s); }
            assert_eq!(s.state,before);finish(&mut s);
            assert_eq!(s.state.regs[ESP],esp+4);assert_eq!(s.kernel.irql(),level);
            assert_eq!(s.state.eflags,0xCD7);
        }
    }

    #[test]
    fn snapshot_restaura_irql_nao_passivo_separado_de_if_e_chamada_pendente() {
        let mut s=loaded(Irql::Dispatch);boundary(&mut s);attend(&mut s);boundary(&mut s);
        let checkpoint=saved(&s);finish(&mut s);let complete=saved(&s);
        let mut restored=loaded(Irql::Passive);let mut inp=SnapshotReader::new(&checkpoint).unwrap();
        Snapshot::load(&mut restored,&mut inp).unwrap();assert_eq!(inp.remaining(),0);
        assert!(saved(&restored)==checkpoint);assert_eq!(restored.kernel.irql(),Irql::Dispatch);
        assert_eq!(restored.state.eflags&INTERRUPT_FLAG,0);
        finish(&mut restored);assert!(saved(&restored)==complete);
    }

    #[test]
    fn retorno_invalido_nao_altera_cpu_irql_contadores_ou_ram_e_permite_retry() {
        let mut s=loaded(Irql::Apc);boundary(&mut s);let esp=s.state.regs[ESP];
        let call=s.kernel_call().unwrap();s.bus.escrever_u32(GuestVirt(esp),0x50000000).unwrap();
        let before=s.state.clone();assert!(s.atender_kernel().is_err());
        assert_eq!(s.state,before);assert_eq!(s.kernel.irql(),Irql::Apc);assert_eq!(s.kernel.atendidas(),0);
        assert_eq!(s.bus.ler_u32(GuestVirt(esp)).unwrap(),0x50000000);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12100)).unwrap(),0);
        s.bus.escrever_u32(GuestVirt(esp),call.return_pc).unwrap();attend(&mut s);boundary(&mut s);finish(&mut s);
    }
}
