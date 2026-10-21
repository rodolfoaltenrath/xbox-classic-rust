
#[cfg(test)]
mod critical_verification {
    use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
    use xbox_mem::GuestVirt;
    use xbox_recomp::runtime::{Session, Exit, ESP};
    mod critical { include!("kernel_critical.rs"); }

    fn loaded() -> Session {
        Session::load(include_bytes!("kernel_critical.xbe")).unwrap()
    }

    fn boundary(s: &mut Session, ordinal: u32) {
        assert_eq!(critical::run(&mut s.state, &mut s.bus, 100).unwrap(), Exit::KernelCall { ordinal });
    }

    fn state(s: &Session) -> [u32; 3] {
        [16,20,24].map(|offset| s.bus.ler_u32(GuestVirt(0x12100+offset)).unwrap())
    }

    fn saved(s: &Session) -> Vec<u8> {
        let mut out = SnapshotWriter::new(); s.save(&mut out); out.finish()
    }

    fn finish(s: &mut Session) {
        s.atender_kernel().unwrap(); // TryEnter recursivo, já pendente.
        assert_eq!(s.state.regs[0], 1);
        assert_eq!(state(s), [1,2,1]);
        for (ordinal, expected) in [(294,[0,1,1]), (294,[u32::MAX,0,0]), (306,[0,1,1]), (294,[u32::MAX,0,0])] {
            boundary(s, ordinal); s.atender_kernel().unwrap();
            assert_eq!(state(s), expected);
        }
        assert_eq!(critical::run(&mut s.state, &mut s.bus, 100).unwrap(), Exit::Returned);
    }

    #[test]
    fn secoes_criticas_passam_pela_abi_e_restauram_recursao_e_chamada_pendente() {
        let mut s = loaded(); let initial_esp = s.state.regs[ESP];
        boundary(&mut s,291); s.atender_kernel().unwrap();
        assert_eq!(s.state.regs[ESP],initial_esp);
        assert_eq!(state(&s),[u32::MAX,0,0]);
        boundary(&mut s,277); s.atender_kernel().unwrap();
        assert_eq!(state(&s),[0,1,1]);
        boundary(&mut s,306);
        let checkpoint=saved(&s);
        finish(&mut s);
        assert_eq!(s.state.regs[ESP], initial_esp+4);
        assert_eq!(s.kernel.atendidas(),7);
        let returns: Vec<u32>=(0..7).map(|i| s.bus.ler_u32(GuestVirt(0x12200+i*4)).unwrap()).collect();
        assert_eq!(returns,[0,0,1,0,0,1,0]);
        let complete=saved(&s);
        let mut restored=loaded();
        let mut reader=SnapshotReader::new(&checkpoint).unwrap();
        Snapshot::load(&mut restored,&mut reader).unwrap();
        assert_eq!(reader.remaining(),0);
        assert_eq!(saved(&restored),checkpoint);
        finish(&mut restored);
        assert_eq!(saved(&restored),complete);
    }

    #[test]
    fn estrutura_parcial_preserva_cpu_e_permite_corrigir_a_chamada() {
        let mut s=loaded(); boundary(&mut s,291);
        let esp=s.state.regs[ESP];
        s.bus.escrever_u32(GuestVirt(esp+4),0x12FF0).unwrap();
        s.bus.escrever_bytes(GuestVirt(0x12FF0),&[0xAA;16]).unwrap();
        let before=s.state.clone();
        assert!(s.atender_kernel().is_err());
        assert_eq!(s.state,before);
        assert_eq!(s.kernel.atendidas(),0);
        let mut guard=[0;16]; s.bus.ler_bytes(GuestVirt(0x12FF0),&mut guard).unwrap();
        assert_eq!(guard,[0xAA;16]);
        s.bus.escrever_u32(GuestVirt(esp+4),0x12100).unwrap();
        s.atender_kernel().unwrap(); boundary(&mut s,277);
    }

    #[test]
    fn contencao_recusa_enter_e_try_false_retoma_sem_adquirir() {
        let mut s=loaded(); boundary(&mut s,291); s.atender_kernel().unwrap();
        boundary(&mut s,277);
        for (offset,value) in [(16,0),(20,1),(24,2)] {
            s.bus.escrever_u32(GuestVirt(0x12100+offset),value).unwrap();
        }
        let before=s.state.clone();
        let error=s.atender_kernel().unwrap_err().to_string();
        assert!(error.contains("scheduler"),"{error}");
        assert_eq!(s.state,before);
        assert_eq!(state(&s),[0,1,2]);
        // Retry da mesma entrada após o proprietário externo liberar.
        for (offset,value) in [(16,u32::MAX),(20,0),(24,0)] {
            s.bus.escrever_u32(GuestVirt(0x12100+offset),value).unwrap();
        }
        s.atender_kernel().unwrap(); boundary(&mut s,306);
        s.bus.escrever_u32(GuestVirt(0x12118),2).unwrap();
        s.atender_kernel().unwrap();
        assert_eq!(s.state.regs[0],0);
        assert_eq!(state(&s),[0,1,2]);
        boundary(&mut s,294);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12208)).unwrap(),0);
    }
}
