#[cfg(test)]
mod links_verification {
    use xbox_core::{Snapshot,SnapshotReader,SnapshotWriter};
    use xbox_mem::GuestVirt;
    use xbox_kernel::dispatch::STATUS_OBJECT_NAME_NOT_FOUND;
    use xbox_recomp::runtime::{Session,Exit,ESP};
    mod guest { include!("kernel_links.rs"); }
    fn loaded()->Session {
        let mut s=Session::load(include_bytes!("kernel_links.xbe")).unwrap();
        for (at,buffer,bytes) in [(0x12100u32,0x12200u32,b"\\??\\D:".as_slice()),(0x12108,0x12240,b"\\Device\\CdRom0\\")] {
            s.bus.escrever_u16(GuestVirt(at),bytes.len() as u16).unwrap();
            s.bus.escrever_u16(GuestVirt(at+2),bytes.len() as u16).unwrap();
            s.bus.escrever_u32(GuestVirt(at+4),buffer).unwrap();s.bus.escrever_bytes(GuestVirt(buffer),bytes).unwrap();
        }
        s.bus.escrever_u32(GuestVirt(0x12114),0x12100).unwrap();
        s.bus.escrever_u32(GuestVirt(0x12118),0x40).unwrap();s
    }
    fn boundary(s:&mut Session,ordinal:u32) {assert_eq!(guest::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::KernelCall{ordinal});}
    fn saved(s:&Session)->Vec<u8> {let mut out=SnapshotWriter::new();s.save(&mut out);out.finish()}
    fn handle(s:&mut Session) {
        let call=s.kernel_call().unwrap();let mut expected=s.state.clone();let clock=s.state.clock;
        s.atender_kernel().unwrap();expected.eip=call.return_pc;expected.regs[ESP]=call.return_esp;expected.regs[0]=s.state.regs[0];
        assert_eq!(s.state,expected);assert_eq!(s.state.clock,clock);
    }
    fn mount(s:&mut Session) {boundary(s,203);handle(s);assert_eq!(s.state.regs[0],STATUS_OBJECT_NAME_NOT_FOUND);boundary(s,67);handle(s);}
    fn finish(s:&mut Session) {
        handle(s);assert_eq!(s.state.regs[0],0);
        assert_eq!(guest::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::Returned);
        assert_eq!(s.state.regs[0],123);assert_eq!(s.kernel.drive_d_target(),Some("\\Device\\CdRom0\\"));
    }
    #[test]
    fn fluxo_nxdk_consulta_cria_abre_fecha_preserva_abi_flags_e_destino() {
        let mut s=loaded();let esp=s.state.regs[ESP];s.state.eflags=0xED7;mount(&mut s);
        boundary(&mut s,203);handle(&mut s);assert_eq!(s.bus.ler_u32(GuestVirt(0x12120)).unwrap(),4);
        boundary(&mut s,187);finish(&mut s);assert_eq!(s.kernel.atendidas(),4);assert_eq!(s.state.eflags,0xED7);
        assert_eq!(s.state.regs[ESP],esp+4);assert_eq!(s.bus.ler_u32(GuestVirt(0x12180)).unwrap(),STATUS_OBJECT_NAME_NOT_FOUND);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12184)).unwrap(),0);
    }
    #[test]
    fn snapshot_com_handle_aberto_reproduz_close_e_legado_migra_namespace_vazio() {
        let mut s=loaded();let initial=saved(&s);mount(&mut s);boundary(&mut s,203);handle(&mut s);boundary(&mut s,187);
        let checkpoint=saved(&s);finish(&mut s);let complete=saved(&s);let mut restored=loaded();
        Snapshot::load(&mut restored,&mut SnapshotReader::new(&checkpoint).unwrap()).unwrap();
        assert!(saved(&restored)==checkpoint);finish(&mut restored);assert!(saved(&restored)==complete);
        let at=initial.windows(4).rposition(|tag|tag==b"SYML").unwrap();let mut old=initial[..at].to_vec();old.push(0); // AOTS v2: sem pai.
        let at=old.windows(4).position(|tag|tag==b"KRNL").unwrap();old[at+4..at+8].copy_from_slice(&7u32.to_le_bytes());let aots=old.windows(4).position(|v|v==b"AOTS").unwrap();old[aots+4..aots+8].copy_from_slice(&2u32.to_le_bytes());
        Snapshot::load(&mut restored,&mut SnapshotReader::new(&old).unwrap()).unwrap();assert!(restored.kernel.drive_d_target().is_none());
        mount(&mut restored);boundary(&mut restored,203);handle(&mut restored);boundary(&mut restored,187);finish(&mut restored);
        assert_eq!(restored.scheduling_profile(),xbox_recomp::runtime::SchedulingProfile::LegacyBootstrap);assert!(restored.kernel.executable_media().is_none());assert!(saved(&restored)==super::boot_dma_verification::legacy_file_media(&super::legacy_scheduler_snapshot(&complete)));
    }
    #[test]
    fn recusa_e_retorno_invalido_preservam_cpu_saida_e_handle_sobre_retorno_e_capturado() {
        let mut s=loaded();mount(&mut s);boundary(&mut s,203);let esp=s.state.regs[ESP];let call=s.kernel_call().unwrap();
        s.bus.escrever_u32(GuestVirt(0x12118),0).unwrap();let before=s.state.clone();assert!(s.atender_kernel().is_err());assert_eq!(s.state,before);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12120)).unwrap(),0);
        s.bus.escrever_u32(GuestVirt(0x12118),0x40).unwrap();s.bus.escrever_u32(GuestVirt(esp),0x50000000).unwrap();
        assert!(s.atender_kernel().is_err());assert_eq!(s.state,before);
        s.bus.escrever_u32(GuestVirt(esp),call.return_pc).unwrap();s.bus.escrever_u32(GuestVirt(esp+4),esp).unwrap();handle(&mut s);
        assert_eq!(s.state.eip,call.return_pc);assert_eq!(s.bus.ler_u32(GuestVirt(esp)).unwrap(),4);
    }
}
