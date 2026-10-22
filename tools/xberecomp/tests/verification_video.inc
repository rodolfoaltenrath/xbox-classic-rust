#[cfg(test)]
mod video_verification {
    use xbox_core::{Snapshot,SnapshotReader,SnapshotWriter};
    use xbox_kernel::dispatch::{Dispatcher,EepromState};
    use xbox_mem::GuestVirt;
    use xbox_recomp::runtime::{Session,Exit,ESP};
    mod guest {include!("kernel_video.rs");}
    fn loaded()->Session {Session::load(include_bytes!("kernel_video.xbe")).unwrap()}
    fn boundary(s:&mut Session,ordinal:u32) {assert_eq!(guest::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::KernelCall{ordinal});}
    fn handle(s:&mut Session) {
        let call=s.kernel_call().unwrap();let mut expected=s.state.clone();s.atender_kernel().unwrap();
        expected.eip=call.return_pc;expected.regs[ESP]=call.return_esp;expected.regs[0]=0;
        assert_eq!(s.state,expected);
    }
    fn saved(s:&Session)->Vec<u8> {let mut out=SnapshotWriter::new();s.save(&mut out);out.finish()}
    fn eeprom(standard:u32,flags:u32)->EepromState {
        let mut bytes=*EepromState::default().bytes();
        bytes[0x58..0x5C].copy_from_slice(&standard.to_le_bytes());
        bytes[0x94..0x98].copy_from_slice(&flags.to_le_bytes());EepromState::supplied(bytes)
    }
    fn finish(s:&mut Session) {
        handle(s);assert!(s.kernel.video().enabled());boundary(s,1);handle(s);
        assert_eq!(guest::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::Returned);
        assert_eq!(s.state.regs[0],0x00480101);assert_eq!(s.bus.ler_u32(GuestVirt(0x12108)).unwrap(),0);
    }
    #[test]
    fn consulta_settings_blanking_saved_data_e_abi_flags_clock_no_rust_gerado() {
        let mut s=loaded();let esp=s.state.regs[ESP];s.state.eflags=0xED7;
        boundary(&mut s,2);handle(&mut s);assert_eq!(s.bus.ler_u32(GuestVirt(0x12100)).unwrap(),0x00480101);
        boundary(&mut s,2);handle(&mut s);assert!(!s.kernel.video().enabled());
        boundary(&mut s,2);finish(&mut s);assert_eq!(s.state.eflags,0xED7);assert_eq!(s.state.regs[ESP],esp+4);
        assert_eq!(s.kernel.atendidas(),4);
    }
    #[test]
    fn snapshot_reproduz_estado_encoder_e_query_usa_eeprom_restaurada() {
        let mut s=loaded();boundary(&mut s,2);handle(&mut s);boundary(&mut s,2);handle(&mut s);boundary(&mut s,2);
        let checkpoint=saved(&s);finish(&mut s);let complete=saved(&s);let mut restored=loaded();
        Snapshot::load(&mut restored,&mut SnapshotReader::new(&checkpoint).unwrap()).unwrap();assert!(saved(&restored)==checkpoint);
        finish(&mut restored);assert!(saved(&restored)==complete);
        // Migração KRNL v8 reinicia o encoder e mantém a EEPROM guardada.
        let mut source=loaded();source.kernel=Dispatcher::with_eeprom(eeprom(0x00800300,0x80000));
        let initial=saved(&source);let at=initial.windows(4).rposition(|v|v==b"AVST").unwrap();let mut old=initial[..at].to_vec();old.push(0);
        let at=old.windows(4).position(|v|v==b"KRNL").unwrap();old[at+4..at+8].copy_from_slice(&8u32.to_le_bytes());let aots=old.windows(4).position(|v|v==b"AOTS").unwrap();old[aots+4..aots+8].copy_from_slice(&2u32.to_le_bytes());
        Snapshot::load(&mut restored,&mut SnapshotReader::new(&old).unwrap()).unwrap();assert!(!restored.kernel.video().enabled());
        boundary(&mut restored,2);handle(&mut restored);assert_eq!(restored.bus.ler_u32(GuestVirt(0x12100)).unwrap(),0x00880301);
        let mut s=loaded();s.kernel=Dispatcher::with_eeprom(eeprom(0x00400200,0));
        boundary(&mut s,2);handle(&mut s);assert_eq!(s.bus.ler_u32(GuestVirt(0x12100)).unwrap(),0x00400201);
    }
    #[test]
    fn recusas_preservam_cpu_ram_encoder_e_saida_sobre_retorno_usa_continuacao_capturada() {
        let mut s=loaded();boundary(&mut s,2);let esp=s.state.regs[ESP];let call=s.kernel_call().unwrap();
        s.bus.escrever_u32(GuestVirt(esp+8),11).unwrap();let before=s.state.clone();assert!(s.atender_kernel().is_err());assert_eq!(s.state,before);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12100)).unwrap(),0);assert!(!s.kernel.video().enabled());
        s.bus.escrever_u32(GuestVirt(esp+8),6).unwrap();s.bus.escrever_u32(GuestVirt(esp),0x50000000).unwrap();
        assert!(s.atender_kernel().is_err());assert_eq!(s.state,before);
        s.bus.escrever_u32(GuestVirt(esp),call.return_pc).unwrap();s.bus.escrever_u32(GuestVirt(esp+16),esp).unwrap();handle(&mut s);
        assert_eq!(s.state.eip,call.return_pc);assert_eq!(s.bus.ler_u32(GuestVirt(esp)).unwrap(),0x00480101);
    }
}
