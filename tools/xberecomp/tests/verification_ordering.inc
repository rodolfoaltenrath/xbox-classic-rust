
#[cfg(test)]
mod ordering_verification {
    use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
    use xbox_mem::{GuestVirt, Perm, Acesso};
    use xbox_recomp::{Error, runtime::{Session, Exit}};
    /* MODULES */

    #[test]
    fn sfence_preserva_flags_registradores_e_nao_acessa_endereco_dos_registradores() {
        let mut s=Session::load(include_bytes!("sfence_direct.xbe")).unwrap();
        s.state.eflags=0xAD7; s.state.regs[0]=0xDEADBEEF; s.state.regs[2]=0x87654321;
        let regs=s.state.regs; let esp=regs[4];
        assert_eq!(sfence_direct::run(&mut s.state,&mut s.bus,1).unwrap(),Exit::Returned);
        for (index,value) in regs.iter().enumerate() {
            assert_eq!(s.state.regs[index],if index==4 { esp+4 } else { *value });
        }
        assert_eq!(s.state.eflags,0xAD7);
        assert_eq!((s.state.retired,s.state.clock.tsc()),(2,2));
        assert_eq!(s.bus.telemetria().total_de_acessos(),0);
    }

    #[test]
    fn stores_ordenados_sao_visiveis_por_alias_e_dma_e_snapshot_reproduz_continuacao() {
        let image=include_bytes!("sfence_stores.xbe");
        let mut s=Session::load(image).unwrap(); s.state.regs[0]=0x12345678; s.state.regs[1]=0xABCDEF01;
        let frame=s.bus.traduzir(GuestVirt(0x12000),Acesso::Leitura).unwrap();
        s.bus.mapear(GuestVirt(0x13000),frame,1,Perm::RW).unwrap();
        assert_eq!(sfence_stores::run(&mut s.state,&mut s.bus,1).unwrap(),Exit::Budget);
        assert_eq!(s.state.clock.tsc(),3); assert_eq!(s.bus.ler_u32(GuestVirt(0x13020)).unwrap(),0x12345678);
        assert_eq!(&s.bus.dma(frame,0x28).unwrap()[0x20..0x24],&0x12345678u32.to_le_bytes());
        assert_eq!(s.bus.ler_u32(GuestVirt(0x13024)).unwrap(),0);
        let mut writer=SnapshotWriter::new(); s.state.save(&mut writer); s.bus.save(&mut writer); let checkpoint=writer.finish();
        assert_eq!(sfence_stores::run(&mut s.state,&mut s.bus,1).unwrap(),Exit::Returned);
        let complete=s.state.clone(); assert_eq!(s.bus.ler_u32(GuestVirt(0x13024)).unwrap(),0xABCDEF01);
        let mut reader=SnapshotReader::new(&checkpoint).unwrap(); s.state.load(&mut reader).unwrap(); s.bus.load(&mut reader).unwrap();
        assert_eq!(sfence_stores::run(&mut s.state,&mut s.bus,20).unwrap(),Exit::Returned);
        assert_eq!(s.state,complete); assert_eq!(s.state.clock.tsc(),6);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x13024)).unwrap(),0xABCDEF01);
    }

    #[test]
    fn sfence_nao_muda_permissoes_nem_esconde_faltas_de_stores() {
        let mut s=Session::load(include_bytes!("sfence_stores.xbe")).unwrap();
        s.state.regs[0]=0x12345678; s.state.regs[1]=0xABCDEF01;
        let frame=s.bus.traduzir(GuestVirt(0x12000),Acesso::Leitura).unwrap();
        s.bus.mapear(GuestVirt(0x12000),frame,1,Perm::RO).unwrap();
        let before=s.state.clone(); assert!(sfence_stores::run(&mut s.state,&mut s.bus,1).is_err()); assert_eq!(s.state,before);
        s.bus.mapear(GuestVirt(0x12000),frame,1,Perm::RW).unwrap();
        sfence_stores::run(&mut s.state,&mut s.bus,1).unwrap();
        s.bus.mapear(GuestVirt(0x12000),frame,1,Perm::RO).unwrap();
        let before=s.state.clone(); assert!(sfence_stores::run(&mut s.state,&mut s.bus,1).is_err()); assert_eq!(s.state,before);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12020)).unwrap(),0x12345678);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12024)).unwrap(),0);
        assert_eq!(s.state.clock.tsc(),3);
    }

    #[test]
    fn sfence_respeita_orcamento_zero_e_limite_do_clock() {
        let mut s=Session::load(include_bytes!("sfence_direct.xbe")).unwrap();
        s.state.clock.advance(u64::MAX); let before=s.state.clone();
        assert_eq!(sfence_direct::run(&mut s.state,&mut s.bus,0).unwrap(),Exit::Budget); assert_eq!(s.state,before);
        assert!(matches!(sfence_direct::run(&mut s.state,&mut s.bus,1),Err(Error::TimeExhausted{pc:0x11000}))); assert_eq!(s.state,before);
        s.state.clock=xbox_core::Clock::new(); s.state.clock.advance(u64::MAX-1);
        assert!(matches!(sfence_direct::run(&mut s.state,&mut s.bus,1),Err(Error::TimeExhausted{pc:0x11003})));
        assert_eq!(s.state.regs,before.regs); assert_eq!(s.state.eflags,before.eflags);
        assert_eq!((s.state.retired,s.state.clock.tsc()),(1,u64::MAX));
    }
}
