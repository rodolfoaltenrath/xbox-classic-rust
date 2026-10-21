#[cfg(test)]
mod system_time_verification {
    use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
    use xbox_core::time::CPU_HZ;
    use xbox_kernel::dispatch::{Dispatcher, SystemTimeReference};
    use xbox_mem::GuestVirt;
    use xbox_recomp::runtime::{Session, Exit, ESP};
    mod query { include!("kernel_system_time.rs"); }

    fn loaded() -> Session { Session::load(include_bytes!("kernel_system_time.xbe")).unwrap() }
    fn boundary(s: &mut Session) {
        assert_eq!(query::run(&mut s.state, &mut s.bus, 100).unwrap(), Exit::KernelCall {ordinal:128});
    }
    fn read(s: &Session, at: u32) -> u64 {
        let mut bytes=[0;8]; s.bus.ler_bytes(GuestVirt(at),&mut bytes).unwrap(); u64::from_le_bytes(bytes)
    }
    fn saved(s: &Session) -> Vec<u8> {
        let mut out=SnapshotWriter::new(); s.save(&mut out); out.finish()
    }
    // Traduções podem marcar Accessed/Dirty na MMU, inclusive em preflight.
    fn effects(s: &Session) -> (xbox_recomp::runtime::State, Vec<u8>, Vec<u8>) {
        let mut ram=vec![0;0xF00];s.bus.ler_bytes(GuestVirt(0x12100),&mut ram).unwrap();
        let mut out=SnapshotWriter::new();s.kernel.save(&mut out);
        (s.state.clone(),ram,out.finish())
    }
    fn finish(s: &mut Session) {
        s.atender_kernel().unwrap();
        assert_eq!(query::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::Returned);
    }

    #[test]
    fn stdcall_64bits_usa_clock_da_sessao_e_preserva_flags_sem_avancar_tempo_no_hle() {
        let mut s=loaded();let esp=s.state.regs[ESP];boundary(&mut s);
        s.state.clock.set(CPU_HZ);s.state.retired=987654;s.state.eflags=0xED7;
        let before=s.state.clone();let call=s.kernel_call().unwrap();
        s.atender_kernel().unwrap();
        let mut expected=before;expected.regs[0]=0;expected.regs[ESP]=esp;expected.eip=call.return_pc;
        assert_eq!(s.state,expected);
        assert_eq!(read(&s,0x12101),125_911_584_010_000_000);
        boundary(&mut s);s.state.clock.set(2*CPU_HZ);finish(&mut s);
        assert_eq!(read(&s,0x12110),125_911_584_020_000_000);
        assert_eq!(s.state.regs[ESP],esp+4);assert_eq!(s.kernel.atendidas(),2);
    }

    #[test]
    fn repartir_orcamento_e_consultar_fronteira_nao_altera_tempo_observado() {
        let mut a=loaded();let mut b=loaded();boundary(&mut a);
        loop {
            match query::run(&mut b.state,&mut b.bus,1).unwrap() {
                Exit::Budget=>{},Exit::KernelCall {ordinal:128}=>break,other=>panic!("{other:?}"),
            }
        }
        assert_eq!(a.state,b.state);
        for _ in 0..3 {
            assert_eq!(query::run(&mut b.state,&mut b.bus,0).unwrap(),Exit::Budget);
            assert_eq!(query::run(&mut b.state,&mut b.bus,1).unwrap(),Exit::KernelCall {ordinal:128});
        }
        a.atender_kernel().unwrap();b.atender_kernel().unwrap();
        assert!(saved(&a)==saved(&b), "particao de orcamento alterou snapshot");
    }

    #[test]
    fn snapshot_restaura_clock_epoca_ram_e_consulta_pendente_para_replay_identico() {
        let mut s=loaded();
        s.kernel=Dispatcher::with_system_time(SystemTimeReference::from_boot_ticks(116_444_736_000_000_000).unwrap());
        boundary(&mut s);s.state.clock.set(CPU_HZ);s.atender_kernel().unwrap();boundary(&mut s);
        s.state.clock.set(2*CPU_HZ);let checkpoint=saved(&s);finish(&mut s);let complete=saved(&s);
        assert_eq!(read(&s,0x12101),116_444_736_010_000_000);
        assert_eq!(read(&s,0x12110),116_444_736_020_000_000);
        let mut restored=loaded();let mut inp=SnapshotReader::new(&checkpoint).unwrap();
        Snapshot::load(&mut restored,&mut inp).unwrap();assert_eq!(inp.remaining(),0);
        assert!(saved(&restored)==checkpoint);finish(&mut restored);assert!(saved(&restored)==complete);
    }

    #[test]
    fn saida_parcial_e_overflow_recusam_sem_mudar_cpu_ram_e_permitem_retry() {
        let mut s=loaded();boundary(&mut s);let esp=s.state.regs[ESP];
        s.bus.escrever_u32(GuestVirt(esp+4),0x12FFC).unwrap();
        s.bus.escrever_u32(GuestVirt(0x12FFC),0xDEADBEEF).unwrap();let before=effects(&s);
        assert!(s.atender_kernel().is_err());assert!(effects(&s)==before, "falha alterou CPU, RAM ou kernel");
        s.bus.escrever_u32(GuestVirt(esp+4),0x12101).unwrap();
        s.kernel=Dispatcher::with_system_time(SystemTimeReference::from_boot_ticks(i64::MAX).unwrap());
        s.state.clock.set(CPU_HZ);let before=effects(&s);
        assert!(s.atender_kernel().is_err());assert!(effects(&s)==before, "falha alterou CPU, RAM ou kernel");
        s.kernel=Dispatcher::new();s.atender_kernel().unwrap();boundary(&mut s);finish(&mut s);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12FFC)).unwrap(),0xDEADBEEF);
    }

    #[test]
    fn retorno_validado_antes_da_saida_e_alias_da_pilha_usa_continuacao_capturada() {
        let mut s=loaded();boundary(&mut s);let esp=s.state.regs[ESP];let call=s.kernel_call().unwrap();
        s.bus.escrever_u32(GuestVirt(esp),0x50000000).unwrap();let before=effects(&s);
        assert!(s.atender_kernel().is_err());assert!(effects(&s)==before, "falha alterou CPU, RAM ou kernel");
        s.bus.escrever_u32(GuestVirt(esp),call.return_pc).unwrap();
        s.bus.escrever_u32(GuestVirt(esp+4),esp).unwrap();s.state.clock.set(CPU_HZ);
        s.atender_kernel().unwrap();
        assert_eq!(read(&s,esp),125_911_584_010_000_000);
        assert_eq!(s.state.eip,call.return_pc);assert_eq!(s.state.regs[ESP],call.return_esp);
        boundary(&mut s);finish(&mut s);
    }
}
