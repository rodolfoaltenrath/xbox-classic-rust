
#[cfg(test)]
mod nvram_verification {
    use xbox_core::snapshot::{Snapshot,SnapshotReader,SnapshotWriter};
    use xbox_kernel::dispatch::{Dispatcher,EepromState,EepromSource};
    use xbox_mem::GuestVirt;
    use xbox_recomp::runtime::{Session,Exit,ESP};
    mod nvram { include!("kernel_nvram.rs"); }

    fn session() -> Session {
        let mut s=Session::load(include_bytes!("kernel_nvram.xbe")).unwrap();
        s.kernel=Dispatcher::with_eeprom(EepromState::supplied(std::array::from_fn(|i|i as u8)));
        s.bus.escrever_bytes(GuestVirt(0x12100),&[0xBB;8]).unwrap();
        s.bus.escrever_bytes(GuestVirt(0x121FF),&[0xAA;258]).unwrap();
        s
    }
    fn boundary(s:&mut Session) {
        assert_eq!(nvram::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::KernelCall{ordinal:24});
    }
    fn bytes(s:&Session) -> Vec<u8> {
        let mut out=SnapshotWriter::new(); s.save(&mut out); out.finish()
    }
    fn finish(s:&mut Session) {
        s.atender_kernel().unwrap();
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12100)).unwrap(),3);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12104)).unwrap(),256);
        let mut image=[0;256]; s.bus.ler_bytes(GuestVirt(0x12200),&mut image).unwrap();
        assert_eq!(image,std::array::from_fn(|i|i as u8));
        for ordinal in [335,336,337] {
            assert_eq!(nvram::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::KernelCall{ordinal});
            s.atender_kernel().unwrap();
        }
        assert_eq!(nvram::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::Returned);
    }
    #[test]
    fn eeprom_gerada_atravessa_abi_sha_e_snapshot_com_origem_e_bytes_preservados() {
        let mut s=session(); let esp=s.state.regs[ESP]; boundary(&mut s);
        let checkpoint=bytes(&s); finish(&mut s); let complete=bytes(&s);
        assert_eq!(s.state.regs[ESP],esp+4);
        assert_eq!(s.kernel.atendidas(),4);
        assert_eq!(s.bus.ler_u8(GuestVirt(0x121FF)).unwrap(),0xAA);
        assert_eq!(s.bus.ler_u8(GuestVirt(0x12300)).unwrap(),0xAA);
        // SHA1 de bytes 00..FF, calculado separadamente por .NET ComputeHash.
        let mut hash=[0;20]; s.bus.ler_bytes(GuestVirt(0x12500),&mut hash).unwrap();
        assert_eq!(hash,[0x49,0x16,0xD6,0xBD,0xB7,0xF7,0x8E,0x68,0x03,0x69,0x8C,0xAB,0x32,0xD1,0x58,0x6E,0xA4,0x57,0xDF,0xC8]);
        let mut restored=Session::load(include_bytes!("kernel_nvram.xbe")).unwrap();
        assert_eq!(restored.kernel.eeprom().source(),EepromSource::SyntheticV1);
        Snapshot::load(&mut restored,&mut SnapshotReader::new(&checkpoint).unwrap()).unwrap();
        assert_eq!(restored.kernel.eeprom().source(),EepromSource::Supplied);
        finish(&mut restored); assert_eq!(bytes(&restored),complete);
    }
    #[test]
    fn status_de_buffer_curto_e_saida_invalida_retornam_ao_guest_sem_copia_parcial() {
        for (index,value,status,length) in [(3,255,0xC0000023,256),(1,0,0xC0000005,0xBBBBBBBB)] {
            let mut s=session(); let esp=s.state.regs[ESP]; boundary(&mut s);
            s.bus.escrever_u32(GuestVirt(s.state.regs[ESP]+4+index*4),value).unwrap();
            s.atender_kernel().unwrap();
            assert_eq!(s.state.regs[0],status); assert_eq!(s.state.regs[ESP],esp);
            let mut output=[0;256]; s.bus.ler_bytes(GuestVirt(0x12200),&mut output).unwrap();
            assert_eq!(output,[0xAA;256]);
            assert_eq!(s.bus.ler_u32(GuestVirt(0x12100)).unwrap(),0xBBBBBBBB);
            assert_eq!(s.bus.ler_u32(GuestVirt(0x12104)).unwrap(),length);
        }
    }
    #[test]
    fn indice_nao_modelado_preserva_cpu_e_permite_corrigir_a_chamada_pendente() {
        let mut s=session(); boundary(&mut s);
        s.bus.escrever_u32(GuestVirt(s.state.regs[ESP]+4),7).unwrap();
        let before=s.state.clone();
        let error=s.atender_kernel().unwrap_err().to_string();
        assert!(error.contains("ExQueryNonVolatileSetting"));
        assert_eq!(s.state,before); assert_eq!(s.kernel.recusadas(),1);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12200)).unwrap(),0xAAAAAAAA);
        s.bus.escrever_u32(GuestVirt(s.state.regs[ESP]+4),0xFFFF).unwrap();
        finish(&mut s);
    }
}
