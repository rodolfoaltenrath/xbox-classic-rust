
#[cfg(test)]
mod timing_verification {
    use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
    use xbox_mem::{GuestVirt, Perm, Acesso};
    use xbox_recomp::{Error, runtime::{Session, Exit}};
    /* MODULES */
    fn saved(s: &Session) -> Vec<u8> {
        let mut writer=SnapshotWriter::new(); s.state.save(&mut writer); s.bus.save(&mut writer); writer.finish()
    }
    fn restore(s: &mut Session, bytes: &[u8]) {
        let mut reader=SnapshotReader::new(bytes).unwrap(); s.state.load(&mut reader).unwrap(); s.bus.load(&mut reader).unwrap();
    }
    #[test]
    fn rdtsc_le_clock_edx_eax_preserva_flags_e_outros_registradores() {
        let mut s=Session::load(include_bytes!("tsc_pair.xbe")).unwrap();
        let start=0xFFFFFFFDu64; s.state.clock.advance(start);
        s.state.eflags=0xED7; s.state.retired=9999;
        s.state.regs[5]=0xABCDEF01; s.state.regs[6]=0x12345678; s.state.regs[7]=0x87654321;
        assert_eq!(tsc_pair::run(&mut s.state,&mut s.bus,8).unwrap(),Exit::Returned);
        assert_eq!(s.state.regs[1],start as u32); assert_eq!(s.state.regs[3],0);
        assert_eq!(s.state.regs[0],1); assert_eq!(s.state.regs[2],1);
        assert_eq!(&s.state.regs[5..],&[0xABCDEF01,0x12345678,0x87654321]);
        assert_eq!(s.state.eflags,0xED7); assert_eq!(s.state.retired,10005);
        assert_eq!(s.state.clock.tsc(),start+6);
    }
    #[test]
    fn fatias_orcamento_snapshot_e_replay_nao_alteram_o_tempo_guest() {
        let image=include_bytes!("tsc_pair.xbe");
        let mut whole=Session::load(image).unwrap(); whole.state.clock.advance(1234567890123);
        tsc_pair::run(&mut whole.state,&mut whole.bus,8).unwrap();
        let mut split=Session::load(image).unwrap(); split.state.clock.advance(1234567890123);
        let before=split.state.clone(); assert_eq!(tsc_pair::run(&mut split.state,&mut split.bus,0).unwrap(),Exit::Budget); assert_eq!(split.state,before);
        assert_eq!(tsc_pair::run(&mut split.state,&mut split.bus,1).unwrap(),Exit::Budget);
        let checkpoint=saved(&split);
        tsc_pair::run(&mut split.state,&mut split.bus,1).unwrap(); assert_eq!(whole.state,split.state);
        restore(&mut split,&checkpoint); tsc_pair::run(&mut split.state,&mut split.bus,8).unwrap();
        assert_eq!(whole.state,split.state); let before=split.state.clone();
        assert_eq!(tsc_pair::run(&mut split.state,&mut split.bus,0).unwrap(),Exit::Returned); assert_eq!(split.state,before);
    }
    #[test]
    fn rep_parcial_avanca_por_elemento_sem_confundir_com_retired() {
        let mut s=Session::load(include_bytes!("tsc_rep.xbe")).unwrap();
        s.state.regs[1]=5; s.state.regs[6]=0x12010; s.state.regs[7]=0x12020;
        s.bus.escrever_bytes(GuestVirt(0x12010),b"abcde").unwrap();
        assert_eq!(tsc_rep::run(&mut s.state,&mut s.bus,2).unwrap(),Exit::Budget);
        assert_eq!(s.state.clock.tsc(),2); assert_eq!(s.state.retired,0); assert_eq!(s.state.regs[1],3);
        let checkpoint=saved(&s);
        assert_eq!(tsc_rep::run(&mut s.state,&mut s.bus,20).unwrap(),Exit::Returned);
        assert_eq!(s.state.regs[0],5); assert_eq!(s.state.clock.tsc(),7); assert_eq!(s.state.retired,3);
        let complete=s.state.clone(); let mut copy=[0;5]; s.bus.ler_bytes(GuestVirt(0x12020),&mut copy).unwrap(); assert_eq!(&copy,b"abcde");
        restore(&mut s,&checkpoint); tsc_rep::run(&mut s.state,&mut s.bus,20).unwrap(); assert_eq!(s.state,complete);
        let mut empty=Session::load(include_bytes!("tsc_rep.xbe")).unwrap();
        tsc_rep::run(&mut empty.state,&mut empty.bus,8).unwrap();
        assert_eq!(empty.state.regs[0],1); assert_eq!(empty.state.clock.tsc(),3);
    }
    #[test]
    fn faltas_e_limite_do_clock_nao_cobram_tempo_nem_alteram_cpu() {
        let mut s=Session::load(include_bytes!("tsc_fault.xbe")).unwrap();
        s.state.clock.advance(100); let before=s.state.clone();
        let frame=s.bus.traduzir(GuestVirt(0x12000),Acesso::Leitura).unwrap();
        s.bus.mapear(GuestVirt(0x12000),frame,1,Perm::RO).unwrap();
        assert!(tsc_fault::run(&mut s.state,&mut s.bus,8).is_err()); assert_eq!(s.state,before);
        s.bus.mapear(GuestVirt(0x12000),frame,1,Perm::RW).unwrap();
        tsc_fault::run(&mut s.state,&mut s.bus,8).unwrap(); assert_eq!(s.state.regs[0],101); assert_eq!(s.state.clock.tsc(),103);
        let mut limit=Session::load(include_bytes!("tsc_pair.xbe")).unwrap(); limit.state.clock.advance(u64::MAX);
        let before=limit.state.clone();
        assert!(matches!(tsc_pair::run(&mut limit.state,&mut limit.bus,8),Err(Error::TimeExhausted{pc:0x11000}))); assert_eq!(limit.state,before);
        assert_eq!(tsc_pair::run(&mut limit.state,&mut limit.bus,0).unwrap(),Exit::Budget);
    }
    #[test]
    fn fronteira_hle_nao_cria_um_segundo_relogio_nem_tempo_por_poll() {
        let mut s=Session::load(include_bytes!("tsc_kernel.xbe")).unwrap();
        assert_eq!(tsc_kernel::run(&mut s.state,&mut s.bus,20).unwrap(),Exit::KernelCall{ordinal:289});
        assert_eq!(s.state.clock.tsc(),3); let before=s.state.clone();
        for _ in 0..3 { assert_eq!(tsc_kernel::run(&mut s.state,&mut s.bus,1).unwrap(),Exit::KernelCall{ordinal:289}); }
        assert_eq!(s.state,before);
        s.atender_kernel().unwrap(); assert_eq!(s.state.clock.tsc(),3);
        tsc_kernel::run(&mut s.state,&mut s.bus,20).unwrap(); assert_eq!(s.state.regs[0],3); assert_eq!(s.state.clock.tsc(),5);
    }
}
