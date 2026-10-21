
#[cfg(test)]
mod msr_verification {
    use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
    use xbox_mem::{GuestVirt, Acesso, Perm};
    use xbox_recomp::{Error, runtime::{Session, Exit, CpuProfile}};
    /* MODULES */

    fn saved(s: &Session) -> Vec<u8> {
        let mut out=SnapshotWriter::new(); s.save(&mut out); out.finish()
    }

    #[test]
    fn rdmsr_poweron_usa_ecx_e_preserva_flags_e_registradores_fora_edx_eax() {
        let mut s=Session::load(include_bytes!("msr_direct.xbe")).unwrap();
        s.state.regs=[0xDEADBEEF,0x2A,0x12345678,0x87654321,s.state.regs[4],0xABCD,0xFF00,0x00FF];
        s.state.eflags=0xED7; s.state.clock.advance(999); s.state.retired=123;
        let before=s.state.clone();
        assert_eq!(msr_direct::run(&mut s.state,&mut s.bus,1).unwrap(),Exit::Budget);
        let mut expected=before;
        expected.regs[0]=0x01040000; expected.regs[2]=0;
        expected.eip=0x11004; expected.clock.advance(2); expected.retired+=2;
        assert_eq!(s.state,expected);
        assert_eq!(s.state.cpu_profile,CpuProfile::XboxRetailV1);
        assert_eq!(s.bus.telemetria().total_de_acessos(),0);
    }

    #[test]
    fn msr_tsc_e_rdtsc_compartilham_clock_de_64_bits_com_carry() {
        for start in [0u64,0xFFFF_FFFD,0xABCD_EF01_1234_5678] {
            let mut s=Session::load(include_bytes!("msr_tsc.xbe")).unwrap();
            s.state.clock.advance(start); s.state.eflags=0xED7; s.state.retired=9876;
            let esp=s.state.regs[4];
            assert_eq!(msr_tsc::run(&mut s.state,&mut s.bus,20).unwrap(),Exit::Returned);
            assert_eq!(s.state.regs[3],start as u32);
            assert_eq!(s.state.regs[6],(start>>32) as u32);
            assert_eq!(s.state.regs[0],(start+5) as u32);
            assert_eq!(s.state.regs[2],((start+5)>>32) as u32);
            assert_eq!(s.state.regs[1],0x10); assert_eq!(s.state.regs[4],esp+4);
            assert_eq!(s.state.eflags,0xED7);
            assert_eq!(s.bus.ler_u32(GuestVirt(0x12020)).unwrap(),(start+5) as u32);
            assert_eq!(s.bus.ler_u32(GuestVirt(0x12024)).unwrap(),((start+5)>>32) as u32);
            assert_eq!((s.state.clock.tsc(),s.state.retired),(start+9,9885));
        }
    }

    #[test]
    fn msr_sem_modelo_preserva_cpu_tempo_e_store_seguinte_e_permite_retry() {
        let mut s=Session::load(include_bytes!("msr_store.xbe")).unwrap();
        s.bus.escrever_u32(GuestVirt(0x12020),0xAABBCCDD).unwrap();
        s.state.clock.advance(100); s.state.eflags=0xED7;
        s.state.regs[0]=0x12345678; s.state.regs[2]=0x87654321;
        for index in [0,0x11,0x1B,0x8000002A,u32::MAX] {
            s.state.regs[1]=index; let before=s.state.clone();
            let error=msr_store::run(&mut s.state,&mut s.bus,10).unwrap_err();
            let message=error.to_string();
            assert!(matches!(error,Error::UnsupportedMsr {pc:0x11000,index:i} if i==index));
            assert!(message.contains("0x00011000") && message.contains(&format!("0x{index:08X}")),"{message}");
            assert_eq!(s.state,before);
            assert_eq!(s.bus.ler_u32(GuestVirt(0x12020)).unwrap(),0xAABBCCDD);
        }
        s.state.regs[1]=0x2A;
        assert_eq!(msr_store::run(&mut s.state,&mut s.bus,10).unwrap(),Exit::Returned);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12020)).unwrap(),0x01040000);
        assert_eq!((s.state.clock.tsc(),s.state.retired),(103,3));
    }

    #[test]
    fn orcamento_polls_e_limite_do_clock_nao_executam_msr() {
        let mut s=Session::load(include_bytes!("msr_direct.xbe")).unwrap();
        s.state.regs[1]=0x2A; s.state.clock.advance(u64::MAX);
        let before=s.state.clone();
        assert_eq!(msr_direct::run(&mut s.state,&mut s.bus,0).unwrap(),Exit::Budget);
        assert_eq!(s.state,before);
        assert!(matches!(msr_direct::run(&mut s.state,&mut s.bus,1),Err(Error::TimeExhausted{pc:0x11000})));
        assert_eq!(s.state,before);
        s.state.clock=xbox_core::Clock::new(); s.state.clock.advance(u64::MAX-1);
        assert!(matches!(msr_direct::run(&mut s.state,&mut s.bus,1),Err(Error::TimeExhausted{pc:0x11002})));
        assert_eq!(s.state.regs[0],0x01040000); assert_eq!(s.state.regs[2],0);
        assert_eq!((s.state.clock.tsc(),s.state.retired),(u64::MAX,1));
        s.state.regs[1]=u32::MAX;
        for reason in 0..3 {
            s.state.returned=reason==0;
            s.state.halted=if reason==1 {Some(0x11000)} else {None};
            s.state.breakpoint=if reason==2 {Some(0x11000)} else {None};
            let before=s.state.clone();
            for budget in [0,1,100] {
                assert_eq!(msr_direct::run(&mut s.state,&mut s.bus,budget).unwrap(),before.stopped_exit().unwrap());
                assert_eq!(s.state,before);
            }
        }
    }

    #[test]
    fn perfil_cpu_e_clock_sobrevivem_snapshot_integral_antes_do_rdmsr() {
        let image=include_bytes!("msr_tsc.xbe");
        let mut whole=Session::load(image).unwrap(); whole.state.clock.advance(0xFFFFFFFD);
        msr_tsc::run(&mut whole.state,&mut whole.bus,100).unwrap();
        let complete=saved(&whole);
        let mut split=Session::load(image).unwrap(); split.state.clock.advance(0xFFFFFFFD);
        assert_eq!(msr_tsc::run(&mut split.state,&mut split.bus,1).unwrap(),Exit::Budget);
        assert_eq!(split.state.eip,0x1100D);
        let checkpoint=saved(&split);
        let mut restored=Session::load(image).unwrap();
        let mut reader=SnapshotReader::new(&checkpoint).unwrap();
        Snapshot::load(&mut restored,&mut reader).unwrap(); assert_eq!(reader.remaining(),0);
        assert_eq!(saved(&restored),checkpoint);
        assert_eq!(restored.state.cpu_profile,CpuProfile::XboxRetailV1);
        assert_eq!(msr_tsc::run(&mut restored.state,&mut restored.bus,100).unwrap(),Exit::Returned);
        assert_eq!(saved(&restored),complete);
        assert_eq!(msr_tsc::run(&mut split.state,&mut split.bus,1).unwrap(),Exit::Returned);
        assert_eq!(saved(&split),complete);
    }

    #[test]
    fn falta_no_store_seguinte_preserva_msr_concluido_e_sua_cobranca() {
        let mut s=Session::load(include_bytes!("msr_store.xbe")).unwrap();
        s.state.regs[1]=0x2A; s.state.eflags=0xED7;
        let frame=s.bus.traduzir(GuestVirt(0x12000),Acesso::Leitura).unwrap();
        s.bus.mapear(GuestVirt(0x12000),frame,1,Perm::RO).unwrap();
        assert!(matches!(msr_store::run(&mut s.state,&mut s.bus,1),Err(Error::Memory(_))));
        assert_eq!(s.state.eip,0x11002);
        assert_eq!((s.state.regs[0],s.state.regs[2],s.state.eflags),(0x01040000,0,0xED7));
        assert_eq!((s.state.retired,s.state.clock.tsc()),(1,1));
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12020)).unwrap(),0);
    }
}
