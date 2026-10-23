#[cfg(test)]
mod pfifo_verification {
    use xbox_core::{Snapshot,SnapshotReader,SnapshotWriter};
    use xbox_mem::{GuestVirt,PfifoProfile};
    use xbox_recomp::{Error,runtime::{Session,Exit}};
    mod guest {include!("pfifo_guest.rs");}
    fn loaded()->Session {Session::load(include_bytes!("pfifo_guest.xbe")).unwrap()}
    fn saved(s:&Session)->Vec<u8>{let mut w=SnapshotWriter::new();s.save(&mut w);w.finish()}
    fn pending(s:&mut Session){assert_eq!(guest::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::KernelCall{ordinal:1});}
    fn finish(s:&mut Session){s.atender_kernel().unwrap();assert_eq!(guest::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::Returned);}
    fn outputs(s:&Session,values:[u32;3]){for(i,v)in values.into_iter().enumerate(){assert_eq!(s.bus.ler_u32(GuestVirt(0x12100+i as u32*4)).unwrap(),v);}}
    #[test]
    fn x86_programa_controles_e_aliases_sem_adiantar_dma_ou_mudar_flags_e_clock(){
        let mut s=loaded();s.state.eflags=0xED7;pending(&mut s);finish(&mut s);outputs(&s,[0x18C000,4,0x111C]);
        assert_eq!(s.state.eflags,0xED7);assert_eq!(s.state.retired,15);assert_eq!(s.state.clock.cycles(),15);assert_eq!(s.bus.telemetria().total_de_acessos(),0);
    }
    #[test]
    fn snapshot_replay_e_bus_v6_conservam_controles_ou_zero_legado(){
        let mut s=loaded();let initial=saved(&s);pending(&mut s);let checkpoint=saved(&s);finish(&mut s);let complete=saved(&s);
        let mut restored=loaded();restored.load(&mut SnapshotReader::new(&checkpoint).unwrap()).unwrap();assert!(saved(&restored)==checkpoint);finish(&mut restored);assert!(saved(&restored)==complete);
        let mut old=initial;let at=old.windows(4).rposition(|v|v==b"PFFO").unwrap();let count=u32::from_le_bytes(old[at+12..at+16].try_into().unwrap()) as usize;old.drain(at..at+16+count*4);
        let at=old.windows(4).position(|v|v==b"BUS0").unwrap();old[at+4..at+8].copy_from_slice(&6u32.to_le_bytes());
        restored.load(&mut SnapshotReader::new(&old).unwrap()).unwrap();assert!(restored.bus.boot_dma_page().reserved());assert_eq!(restored.bus.nv2a_pfifo().profile(),PfifoProfile::LegacyUnmodeled);
        pending(&mut restored);finish(&mut restored);outputs(&restored,[0,0,0]);let migrated=saved(&restored);s.load(&mut SnapshotReader::new(&migrated).unwrap()).unwrap();assert!(saved(&s)==migrated);
    }
    #[test]
    fn store_desalinhado_recusa_sem_publicar_controle_ou_avancar_cpu(){
        let mut s=loaded();assert_eq!(guest::run(&mut s.state,&mut s.bus,2).unwrap(),Exit::Budget);s.state.regs[2]=0xFD002505;
        let cpu=s.state.clone();let pfifo=s.bus.nv2a_pfifo().clone();assert!(matches!(guest::run(&mut s.state,&mut s.bus,1),Err(Error::Memory(_))));assert_eq!(s.state,cpu);assert_eq!(s.bus.nv2a_pfifo(),&pfifo);
        s.state.regs[2]=0xFD002504;pending(&mut s);finish(&mut s);outputs(&s,[0x18C000,4,0x111C]);
    }
}
