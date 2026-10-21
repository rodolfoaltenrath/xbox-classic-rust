#[cfg(test)]
mod encoder_filters_verification {
    use xbox_core::{Snapshot,SnapshotReader,SnapshotWriter};
    use xbox_mem::GuestVirt;
    use xbox_recomp::runtime::{Session,Exit,ESP};
    mod guest {include!("kernel_encoder_filters.rs");}
    fn loaded()->Session {Session::load(include_bytes!("kernel_encoder_filters.xbe")).unwrap()}
    fn boundary(s:&mut Session) {assert_eq!(guest::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::KernelCall{ordinal:2});}
    fn handle(s:&mut Session) {
        let call=s.kernel_call().unwrap();let mut expected=s.state.clone();s.atender_kernel().unwrap();
        expected.regs[0]=0;expected.regs[ESP]=call.return_esp;expected.eip=call.return_pc;assert_eq!(s.state,expected);
    }
    fn filters(s:&Session,flicker:Option<u8>,soften:Option<bool>) {
        assert_eq!(s.kernel.video().flicker_filter(),flicker);assert_eq!(s.kernel.video().soften_filter(),soften);
        assert!(!s.kernel.video().enabled());assert!(s.kernel.video().mode().is_none());
    }
    fn saved(s:&Session)->Vec<u8> {let mut out=SnapshotWriter::new();s.save(&mut out);out.finish()}
    fn finish(s:&mut Session) {
        handle(s);filters(s,Some(5),Some(false));boundary(s);handle(s);filters(s,Some(7),Some(false));
        boundary(s);handle(s);filters(s,Some(7),Some(true));
        assert_eq!(guest::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::Returned);
    }
    #[test]
    fn filtros_no_rust_emitido_preservam_cpu_flags_clock_e_stdcall() {
        let mut s=loaded();let esp=s.state.regs[ESP];s.state.eflags=0xED7;filters(&s,None,None);
        boundary(&mut s);handle(&mut s);filters(&s,Some(5),None);boundary(&mut s);finish(&mut s);
        assert_eq!(s.state.eflags,0xED7);assert_eq!(s.state.regs[ESP],esp+4);assert_eq!(s.kernel.atendidas(),4);
    }
    #[test]
    fn snapshot_entre_filtros_reproduz_pedido_pendente_e_resultado_integral() {
        let mut s=loaded();boundary(&mut s);handle(&mut s);boundary(&mut s);let checkpoint=saved(&s);
        finish(&mut s);let complete=saved(&s);let mut restored=loaded();
        Snapshot::load(&mut restored,&mut SnapshotReader::new(&checkpoint).unwrap()).unwrap();
        assert!(saved(&restored)==checkpoint);filters(&restored,Some(5),None);finish(&mut restored);assert!(saved(&restored)==complete);
    }
    #[test]
    fn recusas_de_parametro_result_e_retorno_nao_publicam_filtros_e_permitem_reparo() {
        let mut s=loaded();boundary(&mut s);let esp=s.state.regs[ESP];let call=s.kernel_call().unwrap();let cpu=s.state.clone();
        for (offset,value) in [(4,0), (12,8), (12,u32::MAX), (16,0x12100), (16,0x60000000), (0,0x50000000)] {
            let address=GuestVirt(esp+offset);let original=s.bus.ler_u32(address).unwrap();s.bus.escrever_u32(address,value).unwrap();
            assert!(s.atender_kernel().is_err());assert_eq!(s.state,cpu);filters(&s,None,None);
            assert_eq!(s.bus.ler_u32(GuestVirt(0x12100)).unwrap(),0);s.bus.escrever_u32(address,original).unwrap();
        }
        assert_eq!(s.kernel_call().unwrap().return_pc,call.return_pc);handle(&mut s);filters(&s,Some(5),None);
        boundary(&mut s);let esp=s.state.regs[ESP];let cpu=s.state.clone();s.bus.escrever_u32(GuestVirt(esp+12),2).unwrap();
        assert!(s.atender_kernel().is_err());assert_eq!(s.state,cpu);filters(&s,Some(5),None);
        s.bus.escrever_u32(GuestVirt(esp+12),0).unwrap();finish(&mut s);
    }
}
