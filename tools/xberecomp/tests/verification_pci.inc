
#[cfg(test)]
mod pci_verification {
    use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
    use xbox_mem::GuestVirt;
    use xbox_recomp::runtime::{Session, Exit, ESP};
    mod pci { include!("kernel_pci.rs"); }

    fn loaded() -> Session {
        let mut s=Session::load(include_bytes!("kernel_pci.xbe")).unwrap();
        s.bus.escrever_bytes(GuestVirt(0x120FF),&[0xAA;6]).unwrap();
        s.bus.escrever_u32(GuestVirt(0x12110),0xAAAAAAAA).unwrap(); s
    }

    fn boundary(s: &mut Session) {
        assert_eq!(pci::run(&mut s.state,&mut s.bus,1000).unwrap(),Exit::KernelCall {ordinal:46});
    }

    fn saved(s: &Session) -> Vec<u8> {
        let mut out=SnapshotWriter::new(); s.save(&mut out); out.finish()
    }

    fn finish(s: &mut Session) {
        s.atender_kernel().unwrap();
        assert_eq!(pci::run(&mut s.state,&mut s.bus,1000).unwrap(),Exit::Returned);
    }

    #[test]
    fn leitura_pci_limpa_stdcall_preserva_cpu_e_guest_calcula_frequencia() {
        let mut s=loaded(); let esp=s.state.regs[ESP]; boundary(&mut s);
        s.state.eflags=0xED7; let before=s.state.clone(); let call=s.kernel_call().unwrap();
        s.atender_kernel().unwrap();
        let mut expected=before; expected.regs[0]=0; expected.regs[ESP]=esp; expected.eip=call.return_pc;
        assert_eq!(s.state,expected);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12100)).unwrap(),0x01230801);
        assert_eq!(s.bus.ler_u8(GuestVirt(0x120FF)).unwrap(),0xAA);
        assert_eq!(s.bus.ler_u8(GuestVirt(0x12104)).unwrap(),0xAA);
        boundary(&mut s);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12200)).unwrap(),733333333);
        finish(&mut s);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12110)).unwrap(),0xAAAA2308);
        assert_eq!(s.state.regs[ESP],esp+4); assert_eq!(s.kernel.atendidas(),2);
    }

    #[test]
    fn perfil_pci_e_chamada_pendente_sobrevivem_snapshot_integral() {
        let mut s=loaded(); boundary(&mut s); s.atender_kernel().unwrap(); boundary(&mut s);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12200)).unwrap(),733333333);
        let checkpoint=saved(&s); finish(&mut s); let complete=saved(&s);
        let mut restored=loaded(); let mut inp=SnapshotReader::new(&checkpoint).unwrap();
        Snapshot::load(&mut restored,&mut inp).unwrap(); assert_eq!(inp.remaining(),0);
        assert_eq!(saved(&restored),checkpoint); finish(&mut restored);
        assert_eq!(saved(&restored),complete);
    }

    #[test]
    fn saida_parcial_recusa_sem_modificar_cpu_e_permite_retry() {
        let mut s=loaded(); boundary(&mut s); let esp=s.state.regs[ESP];
        s.bus.escrever_u32(GuestVirt(esp+16),0x12FFE).unwrap();
        s.bus.escrever_u16(GuestVirt(0x12FFE),0xBEEF).unwrap(); let before=s.state.clone();
        assert!(s.atender_kernel().is_err()); assert_eq!(s.state,before);
        assert_eq!(s.bus.ler_u16(GuestVirt(0x12FFE)).unwrap(),0xBEEF); assert_eq!(s.kernel.atendidas(),0);
        s.bus.escrever_u32(GuestVirt(esp+16),0x12100).unwrap();
        s.atender_kernel().unwrap(); boundary(&mut s);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12200)).unwrap(),733333333);
    }

    #[test]
    fn variantes_de_escrita_e_registro_desconhecido_nao_fingem_sucesso() {
        let mut s=loaded(); boundary(&mut s); let esp=s.state.regs[ESP];
        s.bus.escrever_u32(GuestVirt(esp+24),1).unwrap(); let before=s.state.clone();
        let error=s.atender_kernel().unwrap_err().to_string();
        assert!(error.contains("HalReadWritePCISpace") && error.contains("escrita"),"{error}");
        assert_eq!(s.state,before);
        s.bus.escrever_u32(GuestVirt(esp+24),0).unwrap();
        s.bus.escrever_u32(GuestVirt(esp+12),0x6B).unwrap();
        assert!(s.atender_kernel().is_err()); assert_eq!(s.state,before);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12100)).unwrap(),0xAAAAAAAA);
        assert_eq!((s.kernel.atendidas(),s.kernel.recusadas()),(0,2));
        s.bus.escrever_u32(GuestVirt(esp+12),0x6C).unwrap();
        s.atender_kernel().unwrap(); boundary(&mut s); finish(&mut s);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12200)).unwrap(),733333333);
    }
}
