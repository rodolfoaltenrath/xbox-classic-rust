
#[cfg(test)]
mod control_verification {
    use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
    use xbox_mem::GuestVirt;
    use xbox_recomp::{Error, runtime::{Session, State, Exit}};
    /* MODULES */

    #[test]
    fn cli_limpa_apenas_if_e_preserva_registradores() {
        let mut s=Session::load(include_bytes!("control_cli.xbe")).unwrap();
        let initial=s.state.clone();
        for flags in 0..4096 {
            s.state=initial.clone(); s.state.eflags=flags;
            s.state.regs[0]=0x12345678; s.state.regs[2]=0x87654321;
            assert_eq!(control_cli::run(&mut s.state,&mut s.bus,1).unwrap(),Exit::Returned);
            assert_eq!(s.state.eflags,flags & !0x200);
            assert_eq!((s.state.regs[0],s.state.regs[2]),(0x12345678,0x87654321));
            assert_eq!((s.state.retired,s.state.clock.tsc()),(2,2));
            assert_eq!(s.state.halted,None);
        }
    }

    #[test]
    fn hlt_para_no_ultimo_bloco_e_polls_nao_executam_a_continuacao() {
        for flags in [0xAD7,0x8D7] {
            let mut s=Session::load(include_bytes!("control_hlt.xbe")).unwrap();
            s.state.eflags=flags; s.state.regs[0]=0xBAD;
            let initial=s.state.clone();
            assert_eq!(control_hlt::run(&mut s.state,&mut s.bus,0).unwrap(),Exit::Budget);
            assert_eq!(s.state,initial);
            let expected=Exit::Halted{pc:0x11000,interrupts_enabled:flags & 0x200 != 0};
            assert_eq!(control_hlt::run(&mut s.state,&mut s.bus,1).unwrap(),expected);
            assert_eq!(s.state.eip,0x11001); assert_eq!(s.state.eflags,flags);
            assert_eq!((s.state.retired,s.state.clock.tsc()),(1,1));
            let halted=s.state.clone();
            for budget in [0,1,1000] {
                assert_eq!(control_hlt::run(&mut s.state,&mut s.bus,budget).unwrap(),expected);
                assert_eq!(s.state,halted);
                assert_eq!(s.bus.ler_u32(GuestVirt(0x12020)).unwrap(),0);
            }
        }
    }

    #[test]
    fn assert_cli_hlt_preserva_parada_em_snapshot_e_rejeita_truncamento() {
        let mut s=Session::load(include_bytes!("control_assert.xbe")).unwrap();
        s.state.eflags=0xAD7;
        let expected=Exit::Halted{pc:0x11001,interrupts_enabled:false};
        assert_eq!(control_assert::run(&mut s.state,&mut s.bus,2).unwrap(),expected);
        assert_eq!(s.state.eip,0x11002); assert_eq!(s.state.eflags,0x8D7);
        assert_eq!((s.state.retired,s.state.clock.tsc()),(2,2));
        let mut writer=SnapshotWriter::new(); s.state.save(&mut writer);
        let bytes=writer.finish(); let mut restored=State::new(0);
        restored.load(&mut SnapshotReader::new(&bytes).unwrap()).unwrap();
        assert_eq!(restored,s.state);
        assert_eq!(control_assert::run(&mut restored,&mut s.bus,100).unwrap(),expected);
        assert_eq!(restored,s.state);
        for end in 0..bytes.len() {
            if let Ok(mut reader)=SnapshotReader::new(&bytes[..end]) {
                assert!(State::new(0).load(&mut reader).is_err());
            }
        }
    }

    #[test]
    fn cli_hlt_verificam_limite_do_clock_antes_dos_efeitos() {
        let mut s=Session::load(include_bytes!("control_assert.xbe")).unwrap();
        s.state.eflags=0xAD7; s.state.clock.set(u64::MAX);
        let initial=s.state.clone();
        assert!(matches!(control_assert::run(&mut s.state,&mut s.bus,8),Err(Error::TimeExhausted{pc:0x11000})));
        assert_eq!(s.state,initial);
        s.state.clock=xbox_core::Clock::new(); s.state.clock.advance(u64::MAX-1);
        assert!(matches!(control_assert::run(&mut s.state,&mut s.bus,8),Err(Error::TimeExhausted{pc:0x11001})));
        assert_eq!(s.state.eflags,0x8D7); assert_eq!(s.state.halted,None);
        assert_eq!((s.state.retired,s.state.clock.tsc()),(1,u64::MAX));
    }
}
