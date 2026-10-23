#[cfg(test)]
mod pramin_verification {
    use xbox_core::{Snapshot,SnapshotReader,SnapshotWriter};
    use xbox_mem::{GuestVirt,PraminProfile};
    use xbox_recomp::{Error,runtime::{Session,Exit}};
    mod guest { include!("pramin_guest.rs"); }
    fn loaded()->Session {Session::load(include_bytes!("pramin_guest.xbe")).unwrap()}
    fn saved(s:&Session)->Vec<u8>{let mut out=SnapshotWriter::new();s.save(&mut out);out.finish()}
    fn pending(s:&mut Session){assert_eq!(guest::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::KernelCall{ordinal:1});}
    fn finish(s:&mut Session){s.atender_kernel().unwrap();assert_eq!(guest::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::Returned);}
    #[test]
    fn guest_programa_e_le_instancia_sem_ciclos_extras_ou_flags_alteradas(){
        let mut s=loaded();s.state.eflags=0xED7;let used=s.bus.frames().usados();pending(&mut s);finish(&mut s);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12100)).unwrap(),0xCAFEBABE);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12104)).unwrap(),0xDEADBEEF);
        assert_eq!(s.bus.nv2a_pramin().read(0x7110A0),Some(0xDEADBEEF));
        assert_eq!(s.state.eflags,0xED7);assert_eq!(s.state.retired,9);assert_eq!(s.state.clock.cycles(),9);
        assert_eq!(s.bus.frames().usados(),used);assert_eq!(s.bus.telemetria().total_de_acessos(),0);
    }
    #[test]
    fn snapshot_replay_e_bus_v4_migra_sem_herdar_instancias(){
        let mut s=loaded();pending(&mut s);let checkpoint=saved(&s);finish(&mut s);let complete=saved(&s);
        let mut restored=loaded();Snapshot::load(&mut restored,&mut SnapshotReader::new(&checkpoint).unwrap()).unwrap();
        assert!(saved(&restored)==checkpoint);finish(&mut restored);assert!(saved(&restored)==complete);
        let mut old=checkpoint;let at=old.windows(4).position(|v|v==b"BUS0").unwrap();old[at+4..at+8].copy_from_slice(&4u32.to_le_bytes());
        let at=old.windows(4).position(|v|v==b"PRAM").unwrap();old.drain(at..at+20+xbox_mem::nv2a_pramin::PRAMIN_SIZE);
        Snapshot::load(&mut restored,&mut SnapshotReader::new(&old).unwrap()).unwrap();
        assert_eq!(restored.bus.nv2a_pramin().profile(),PraminProfile::LegacyUnmodeled);finish(&mut restored);
        assert_eq!(restored.bus.ler_u32(GuestVirt(0x12100)).unwrap(),0);assert_eq!(restored.bus.ler_u32(GuestVirt(0x12104)).unwrap(),0);
        assert_eq!(restored.bus.telemetria().total_de_acessos(),3);
        let migrated=saved(&restored);Snapshot::load(&mut s,&mut SnapshotReader::new(&migrated).unwrap()).unwrap();assert!(saved(&s)==migrated);
    }
    #[test]
    fn store_desalinhado_preserva_cpu_clock_e_instancias_e_pode_ser_retentado(){
        let mut s=loaded();pending(&mut s);s.atender_kernel().unwrap();
        assert_eq!(guest::run(&mut s.state,&mut s.bus,2).unwrap(),Exit::Budget);
        s.state.regs[2]=0xFD7110A1;let before=s.state.clone();let memory=s.bus.nv2a_pramin().clone();
        assert!(matches!(guest::run(&mut s.state,&mut s.bus,1),Err(Error::Memory(_))));
        assert_eq!(s.state,before);assert_eq!(s.bus.nv2a_pramin(),&memory);
        s.state.regs[2]=0xFD7110A0;assert_eq!(guest::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::Returned);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12104)).unwrap(),0xDEADBEEF);
    }
}
