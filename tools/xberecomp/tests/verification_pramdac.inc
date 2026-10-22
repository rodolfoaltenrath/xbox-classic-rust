#[cfg(test)]
mod pramdac_verification {
    use xbox_core::{Snapshot,SnapshotReader,SnapshotWriter};
    use xbox_mem::{GuestVirt,PramdacProfile};
    use xbox_recomp::{Error,runtime::{Session,Exit}};
    mod guest { include!("pramdac_guest.rs"); }
    fn loaded()->Session { Session::load(include_bytes!("pramdac_guest.xbe")).unwrap() }
    fn saved(s:&Session)->Vec<u8> {let mut out=SnapshotWriter::new();s.save(&mut out);out.finish()}
    fn pending(s:&mut Session) {
        assert_eq!(guest::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::KernelCall{ordinal:1});
    }
    fn finish(s:&mut Session) {
        s.atender_kernel().unwrap();assert_eq!(guest::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::Returned);
    }
    #[test]
    fn leitura_programacao_e_frequencia_usam_mmio_guest_sem_cobrar_tempo_extra() {
        let mut s=loaded();s.state.eflags=0xED7;pending(&mut s);finish(&mut s);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12100)).unwrap(),0x00011C01);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12104)).unwrap(),0x00010902);
        assert_eq!(s.bus.nv2a_pramdac().core_clock_hz(),Some(37499998));
        assert_eq!(s.state.eflags,0xED7);assert_eq!(s.state.retired,8);
        assert_eq!(s.state.clock.cycles(),8);assert_eq!(s.bus.telemetria().total_de_acessos(),0);
    }
    #[test]
    fn snapshot_integral_reproduz_programacao_e_bus_v2_migra_para_zero_declarado() {
        let mut s=loaded();pending(&mut s);let checkpoint=saved(&s);finish(&mut s);let complete=saved(&s);
        let mut restored=loaded();Snapshot::load(&mut restored,&mut SnapshotReader::new(&checkpoint).unwrap()).unwrap();
        assert!(saved(&restored)==checkpoint);finish(&mut restored);assert!(saved(&restored)==complete);
        let mut old=checkpoint;let at=old.windows(4).position(|v|v==b"BUS0").unwrap();
        old[at+4..at+8].copy_from_slice(&2u32.to_le_bytes());let at=old.windows(4).position(|v|v==b"NPLL").unwrap();old.drain(at..at+16);
        Snapshot::load(&mut restored,&mut SnapshotReader::new(&old).unwrap()).unwrap();
        assert_eq!(restored.bus.nv2a_pramdac().profile(),PramdacProfile::LegacyUnmodeled);finish(&mut restored);
        assert_eq!(restored.bus.ler_u32(GuestVirt(0x12104)).unwrap(),0);
        assert_eq!(restored.bus.telemetria().total_de_acessos(),2);
        let migrated=saved(&restored);Snapshot::load(&mut s,&mut SnapshotReader::new(&migrated).unwrap()).unwrap();assert!(saved(&s)==migrated);
    }
    #[test]
    fn store_desalinhado_recusa_sem_alterar_cpu_clock_ou_coeficiente_e_permite_reparo() {
        let mut s=loaded();pending(&mut s);s.atender_kernel().unwrap();
        assert_eq!(guest::run(&mut s.state,&mut s.bus,1).unwrap(),Exit::Budget);
        s.state.regs[2]=0xFD680501;let before=s.state.clone();let clock=s.bus.nv2a_pramdac().clone();
        assert!(matches!(guest::run(&mut s.state,&mut s.bus,1),Err(Error::Memory(_))));
        assert_eq!(s.state,before);assert_eq!(s.bus.nv2a_pramdac(),&clock);
        s.state.regs[2]=0xFD680500;
        assert_eq!(guest::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::Returned);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12104)).unwrap(),0x00010902);
    }
}
