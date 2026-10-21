
#[cfg(test)]
mod nop_verification {
    use xbox_core::snapshot::{Snapshot,SnapshotReader,SnapshotWriter};
    use xbox_mem::{GuestVirt,GuestPhys,Perm};
    use xbox_recomp::{Error,runtime::{Session,Exit}};
    /* MODULES */

    #[test]
    fn nops_de_um_a_nove_bytes_preservam_flags_registradores_e_cobram_uma_instrucao() {
        let mut s=Session::load(include_bytes!("nop_shapes.xbe")).unwrap();
        let initial=s.state.clone();
        for flags in 0..4096 {
            s.state=initial.clone(); s.state.eflags=flags;
            for reg in [0,1,2,3,5,6,7] { s.state.regs[reg]=0x50000000+reg as u32; }
            let regs=s.state.regs;
            assert_eq!(nop_shapes::run(&mut s.state,&mut s.bus,0).unwrap(),Exit::Budget);
            assert_eq!(s.state.retired,0);
            assert_eq!(nop_shapes::run(&mut s.state,&mut s.bus,1).unwrap(),Exit::Budget);
            assert_eq!(s.state.regs,regs); assert_eq!(s.state.eflags,flags);
            assert_eq!((s.state.retired,s.state.clock.tsc()),(10,10)); // 9 NOPs + JMP
        }
        assert_eq!(nop_shapes::run(&mut s.state,&mut s.bus,1).unwrap(),Exit::Returned);
    }

    #[test]
    fn operandos_nop_nao_leem_ram_mmio_fs_ou_endereco_de_dezesseis_bits() {
        let mut s=Session::load(include_bytes!("nop_addresses.xbe")).unwrap();
        s.bus.mapear(GuestVirt(0x50000000),GuestPhys(0xFD000000),1,Perm::RW).unwrap();
        let initial=s.state.clone();
        for address in [0,0x50000000,u32::MAX] {
            s.state=initial.clone(); s.state.fs_base=0;
            s.state.regs[0]=address; s.state.regs[1]=u32::MAX;
            s.state.regs[3]=0xFFFF; s.state.regs[6]=1;
            let regs=s.state.regs;
            assert_eq!(nop_addresses::run(&mut s.state,&mut s.bus,1).unwrap(),Exit::Budget);
            assert_eq!(s.state.regs,regs);
            assert_eq!(s.state.retired,4);
        }
        // Uma base FS sem página também não deve ser traduzida para NOP.
        s.state=initial; s.state.fs_base=u32::MAX;
        assert_eq!(nop_addresses::run(&mut s.state,&mut s.bus,1).unwrap(),Exit::Budget);
        assert!(s.bus.telemetria().registros().is_empty());
    }

    #[test]
    fn snapshot_entre_nops_e_ret_e_limite_do_clock_preservam_estado() {
        let mut s=Session::load(include_bytes!("nop_shapes.xbe")).unwrap();
        nop_shapes::run(&mut s.state,&mut s.bus,1).unwrap();
        let mut out=SnapshotWriter::new(); s.save(&mut out); let saved=out.finish();
        assert_eq!(nop_shapes::run(&mut s.state,&mut s.bus,1).unwrap(),Exit::Returned);
        let complete=s.state.clone();
        Snapshot::load(&mut s,&mut SnapshotReader::new(&saved).unwrap()).unwrap();
        assert_eq!(nop_shapes::run(&mut s.state,&mut s.bus,1).unwrap(),Exit::Returned);
        assert_eq!(s.state,complete);
        let mut limit=Session::load(include_bytes!("nop_shapes.xbe")).unwrap();
        limit.state.clock.advance(u64::MAX); let before=limit.state.clone();
        assert!(matches!(nop_shapes::run(&mut limit.state,&mut limit.bus,1),Err(Error::TimeExhausted{pc:0x11000})));
        assert_eq!(limit.state,before);
    }
}
