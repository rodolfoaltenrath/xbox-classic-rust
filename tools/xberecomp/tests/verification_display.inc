#[cfg(test)]
mod display_verification {
    use xbox_core::{Snapshot,SnapshotReader,SnapshotWriter};
    use xbox_mem::{GuestPhys,GuestVirt};
    use xbox_recomp::runtime::{Session,Exit,ESP};
    mod guest {include!("kernel_display.rs");}
    fn loaded()->Session {Session::load(include_bytes!("kernel_display.xbe")).unwrap()}
    fn boundary(s:&mut Session,ordinal:u32) {assert_eq!(guest::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::KernelCall{ordinal});}
    fn handle(s:&mut Session,eax:u32) {
        let call=s.kernel_call().unwrap();let mut expected=s.state.clone();s.atender_kernel().unwrap();
        expected.regs[0]=eax;expected.regs[ESP]=call.return_esp;expected.eip=call.return_pc;assert_eq!(s.state,expected);
    }
    fn saved(s:&Session)->Vec<u8> {let mut out=SnapshotWriter::new();s.save(&mut out);out.finish()}
    fn pending(s:&mut Session) {boundary(s,166);handle(s,0x80020000);boundary(s,3);}
    fn pixels(s:&Session,enabled:bool) {
        let view=s.kernel.framebuffer(&s.bus).unwrap().unwrap();assert_eq!(view.mode().framebuffer(),GuestPhys(0x20000));
        assert_eq!((view.mode().width(),view.mode().height(),view.mode().pitch()),(640,480,2560));
        assert_eq!(&view.row(0).unwrap()[..4],&[0xCC,0x33,0,0xFF]);assert_eq!(&view.row(1).unwrap()[..4],&[0,0x55,0xAA,0xFF]);
        assert_eq!(view.row(0).unwrap().len(),2560);assert!(view.row(480).is_none());assert_eq!(view.enabled(),enabled);
    }
    fn finish(s:&mut Session) {
        handle(s,0);pixels(s,true);assert_eq!(guest::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::Returned);
    }
    #[test]
    fn modo_stdcall_tem_efeito_framebuffer_usa_pixels_guest_e_blanking_e_separado() {
        let mut s=loaded();let esp=s.state.regs[ESP];pending(&mut s);let clock=s.state.clock;
        handle(&mut s,0);assert_eq!(s.state.clock,clock);pixels(&s,false);boundary(&mut s,2);finish(&mut s);
        assert_eq!(s.state.regs[ESP],esp+4);assert_eq!(s.kernel.atendidas(),3);
    }
    #[test]
    fn snapshot_modo_e_dados_reproduzem_habilitacao_e_avst_v1_migra_sem_modo() {
        let mut s=loaded();pending(&mut s);let legacy_base=saved(&s);handle(&mut s,0);boundary(&mut s,2);
        let checkpoint=saved(&s);finish(&mut s);let complete=saved(&s);let mut restored=loaded();
        Snapshot::load(&mut restored,&mut SnapshotReader::new(&checkpoint).unwrap()).unwrap();assert!(saved(&restored)==checkpoint);
        pixels(&restored,false);finish(&mut restored);assert!(saved(&restored)==complete);
        let mut old=legacy_base;let av=old.windows(4).rposition(|v|v==b"AVST").unwrap();
        old[av+4..av+8].copy_from_slice(&1u32.to_le_bytes());old.remove(av+13); // v1 não tinha indicador de modo.
        Snapshot::load(&mut restored,&mut SnapshotReader::new(&old).unwrap()).unwrap();assert!(restored.kernel.video().mode().is_none());
        handle(&mut restored,0);pixels(&restored,false);boundary(&mut restored,2);finish(&mut restored);assert!(saved(&restored)==complete);
    }
    #[test]
    fn etapa_pa_e_retorno_invalidos_recusam_sem_publicar_modo_ou_alterar_cpu() {
        let mut s=loaded();pending(&mut s);let esp=s.state.regs[ESP];let call=s.kernel_call().unwrap();let before=s.state.clone();
        s.bus.escrever_u32(GuestVirt(esp+8),1).unwrap();assert!(s.atender_kernel().is_err());assert_eq!(s.state,before);assert!(s.kernel.video().mode().is_none());
        s.bus.escrever_u32(GuestVirt(esp+8),0).unwrap();s.bus.escrever_u32(GuestVirt(esp+24),0x80020000).unwrap();
        assert!(s.atender_kernel().is_err());assert_eq!(s.state,before);assert!(s.kernel.video().mode().is_none());
        s.bus.escrever_u32(GuestVirt(esp+24),0x20000).unwrap();s.bus.escrever_u32(GuestVirt(esp),0x50000000).unwrap();
        assert!(s.atender_kernel().is_err());assert_eq!(s.state,before);assert!(s.kernel.video().mode().is_none());
        s.bus.escrever_u32(GuestVirt(esp),call.return_pc).unwrap();handle(&mut s,0);pixels(&s,false);
    }
}
