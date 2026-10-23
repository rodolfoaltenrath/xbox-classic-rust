#[cfg(test)]
mod pfb_verification {
    use xbox_core::{Snapshot,SnapshotReader,SnapshotWriter};
    use xbox_mem::{GuestVirt,PfbProfile};
    use xbox_recomp::{Error,runtime::{Session,Exit}};
    mod guest { include!("pfb_guest.rs"); }
    fn loaded()->Session { Session::load(include_bytes!("pfb_guest.xbe")).unwrap() }
    fn saved(s:&Session)->Vec<u8> {let mut out=SnapshotWriter::new();s.save(&mut out);out.finish()}
    fn pending(s:&mut Session) {
        assert_eq!(guest::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::KernelCall{ordinal:1});
    }
    fn finish(s:&mut Session) {
        s.atender_kernel().unwrap();assert_eq!(guest::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::Returned);
    }
    #[test]
    fn guest_le_perfil_de_memoria_e_programa_cfg0_sem_mudar_flags_ou_adicionar_tempo() {
        let mut s=loaded();s.state.eflags=0xED7;pending(&mut s);finish(&mut s);
        let expected=if s.bus.ram()==128<<20 {0x03070103} else {0x03070003};
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12100)).unwrap(),expected);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12104)).unwrap(),0x11448000);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12108)).unwrap(),0xDEADBEEF);
        assert_eq!(s.state.eflags,0xED7);assert_eq!(s.state.retired,10);
        assert_eq!(s.state.clock.cycles(),10);assert_eq!(s.bus.telemetria().total_de_acessos(),0);
    }
    #[test]
    fn snapshot_replay_e_bus_v3_preservam_respectivamente_perfil_e_ausencia_de_modelo() {
        let mut s=loaded();pending(&mut s);let checkpoint=saved(&s);finish(&mut s);let complete=saved(&s);
        let mut restored=loaded();Snapshot::load(&mut restored,&mut SnapshotReader::new(&checkpoint).unwrap()).unwrap();
        assert!(saved(&restored)==checkpoint);finish(&mut restored);assert!(saved(&restored)==complete);
        let mut old=checkpoint;let at=old.windows(4).position(|v|v==b"PRAM").unwrap();old.drain(at..at+20+xbox_mem::nv2a_pramin::PRAMIN_SIZE);let at=old.windows(4).position(|v|v==b"BUS0").unwrap();
        old[at+4..at+8].copy_from_slice(&3u32.to_le_bytes());let at=old.windows(4).position(|v|v==b"PFBM").unwrap();old.drain(at..at+20);
        Snapshot::load(&mut restored,&mut SnapshotReader::new(&old).unwrap()).unwrap();
        assert_eq!(restored.bus.nv2a_pfb().profile(),PfbProfile::LegacyUnmodeled);
        assert_eq!(restored.bus.nv2a_pramdac().coefficient(),Some(0x00011C01));finish(&mut restored);
        assert_eq!(restored.bus.ler_u32(GuestVirt(0x12108)).unwrap(),0);
        assert_eq!(restored.bus.telemetria().total_de_acessos(),2);
        let migrated=saved(&restored);Snapshot::load(&mut s,&mut SnapshotReader::new(&migrated).unwrap()).unwrap();assert!(saved(&s)==migrated);
    }
    #[test]
    fn store_desalinhado_preserva_cpu_clock_e_pfb_e_permite_retentar() {
        let mut s=loaded();pending(&mut s);s.atender_kernel().unwrap();
        assert_eq!(guest::run(&mut s.state,&mut s.bus,1).unwrap(),Exit::Budget);
        s.state.regs[2]=0xFD100201;let before=s.state.clone();let pfb=s.bus.nv2a_pfb().clone();
        assert!(matches!(guest::run(&mut s.state,&mut s.bus,1),Err(Error::Memory(_))));
        assert_eq!(s.state,before);assert_eq!(s.bus.nv2a_pfb(),&pfb);
        s.state.regs[2]=0xFD100200;
        assert_eq!(guest::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::Returned);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12108)).unwrap(),0xDEADBEEF);
    }
}
