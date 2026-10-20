
#[cfg(test)]
mod breakpoint_verification {
    use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
    use xbox_mem::GuestVirt;
    use xbox_recomp::{Error, runtime::{Session, State, Exit}};
    /* MODULES */

    #[test]
    fn int3_gera_bp_independente_de_if_e_nao_executa_o_store_seguinte() {
        for flags in [0xAD7,0x8D7] {
            let mut s=Session::load(include_bytes!("bp_direct.xbe")).unwrap();
            s.state.eflags=flags; s.state.regs[0]=0x12345678;
            let initial=s.state.clone();
            assert_eq!(bp_direct::run(&mut s.state,&mut s.bus,0).unwrap(),Exit::Budget);
            assert_eq!(s.state,initial);
            assert_eq!(bp_direct::run(&mut s.state,&mut s.bus,1).unwrap(),Exit::Breakpoint{pc:0x11000});
            let mut expected=initial;
            expected.breakpoint=Some(0x11000); expected.eip=0x11001; expected.retired=1; expected.clock.advance(1);
            assert_eq!(s.state,expected);
            assert_eq!(s.bus.ler_u32(GuestVirt(0x12020)).unwrap(),0);
        }
    }

    #[test]
    fn breakpoint_pendente_sobrevive_snapshot_truncamentos_e_polls() {
        let mut s=Session::load(include_bytes!("bp_direct.xbe")).unwrap();
        s.state.clock.advance(u64::MAX-1);
        let expected=Exit::Breakpoint{pc:0x11000};
        assert_eq!(bp_direct::run(&mut s.state,&mut s.bus,1).unwrap(),expected);
        let mut writer=SnapshotWriter::new(); s.state.save(&mut writer);
        let bytes=writer.finish(); let mut restored=State::new(0);
        restored.load(&mut SnapshotReader::new(&bytes).unwrap()).unwrap();
        assert_eq!(restored,s.state);
        for budget in [0,1,1000] {
            assert_eq!(bp_direct::run(&mut restored,&mut s.bus,budget).unwrap(),expected);
            assert_eq!(restored,s.state); assert_eq!(restored.clock.tsc(),u64::MAX);
        }
        for end in 0..bytes.len() {
            if let Ok(mut reader)=SnapshotReader::new(&bytes[..end]) {
                assert!(State::new(0).load(&mut reader).is_err());
            }
        }
    }

    #[test]
    fn branch_sobre_breakpoint_retorna_e_fatias_preservam_o_resultado() {
        for flags in [2,0x42] {
            let mut whole=Session::load(include_bytes!("bp_branch.xbe")).unwrap(); whole.state.eflags=flags;
            let mut split=Session::load(include_bytes!("bp_branch.xbe")).unwrap(); split.state.eflags=flags;
            let expected=if flags & 0x40 != 0 { Exit::Returned } else { Exit::Breakpoint{pc:0x11002} };
            assert_eq!(bp_branch::run(&mut whole.state,&mut whole.bus,8).unwrap(),expected);
            assert_eq!(bp_branch::run(&mut split.state,&mut split.bus,1).unwrap(),Exit::Budget);
            assert_eq!(bp_branch::run(&mut split.state,&mut split.bus,1).unwrap(),expected);
            assert_eq!(split.state,whole.state);
        }
    }

    #[test]
    fn esgotamento_antes_de_int3_nao_cria_excecao_e_preserva_store_anterior() {
        let mut s=Session::load(include_bytes!("bp_store.xbe")).unwrap();
        s.state.clock.advance(u64::MAX-1); s.state.regs[0]=0x12345678;
        assert!(matches!(bp_store::run(&mut s.state,&mut s.bus,1),Err(Error::TimeExhausted{pc:0x11005})));
        assert_eq!(s.state.eip,0x11005); assert_eq!(s.state.breakpoint,None);
        assert_eq!((s.state.retired,s.state.clock.tsc()),(1,u64::MAX));
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12020)).unwrap(),0x12345678);
    }
}
