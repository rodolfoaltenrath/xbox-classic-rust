
#[cfg(test)]
mod kernel_verification {
    use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
    use xbox_mem::{Acesso, GuestVirt, Perm};
    use xbox_recomp::runtime::{Session, Exit, State, ESP};
    mod abi { include!("kernel_abi.rs"); }
    mod tail { include!("kernel_tail.rs"); }
    mod fs_fault { include!("fs_fault.rs"); }

    #[test]
    fn abi_tls_fs_e_snapshot_pendente_funcionam_em_rust_nativo() {
        let mut session = Session::load(include_bytes!("kernel_abi.xbe")).unwrap();
        let initial_esp = session.state.regs[ESP];
        assert_eq!(abi::run(&mut session.state, &mut session.bus, 0).unwrap(), Exit::Budget);
        assert_eq!(abi::run(&mut session.state, &mut session.bus, 100).unwrap(), Exit::KernelCall { ordinal: 165 });
        let call = session.kernel_call().unwrap();
        assert_eq!(call.argument(&session.bus, 0).unwrap(), 0x1122);
        let before = session.state.clone();
        assert!(session.complete_kernel_call(8, 0).is_err());
        assert_eq!(session.state, before);
        session.complete_kernel_call(165, 0xA0).unwrap();
        assert_eq!(session.state.regs[ESP], initial_esp);
        assert_eq!(abi::run(&mut session.state, &mut session.bus, 100).unwrap(), Exit::KernelCall { ordinal: 8 });
        assert_eq!(session.bus.ler_u32(GuestVirt(0x12030)).unwrap(), 0xA0);
        assert_eq!(session.kernel_call().unwrap().argument(&session.bus, 0).unwrap(), 0x33);

        let mut writer = SnapshotWriter::new();
        session.state.save(&mut writer); session.bus.save(&mut writer);
        let snapshot = writer.finish();
        session.complete_kernel_call(8, 0xDEAD).unwrap();
        let mut restored = State::new(0);
        let mut reader = SnapshotReader::new(&snapshot).unwrap();
        restored.load(&mut reader).unwrap();
        session.bus.load(&mut reader).unwrap(); session.state = restored;
        assert_eq!(abi::run(&mut session.state, &mut session.bus, 1).unwrap(), Exit::KernelCall { ordinal: 8 });
        session.complete_kernel_call(8, 0xB0).unwrap();
        assert_eq!(session.state.regs[ESP], initial_esp - 4);
        assert_eq!(abi::run(&mut session.state, &mut session.bus, 100).unwrap(), Exit::KernelCall { ordinal: 21 });
        let call = session.kernel_call().unwrap();
        assert_eq!([call.argument(&session.bus, 0).unwrap(), call.argument(&session.bus, 1).unwrap(), call.argument(&session.bus, 2).unwrap()], [0x44, 0x55, 0x66]);
        session.complete_kernel_call(21, 0xC0).unwrap();
        assert_eq!(session.state.regs[ESP], initial_esp);
        assert_eq!(abi::run(&mut session.state, &mut session.bus, 100).unwrap(), Exit::Returned);
        assert_eq!(session.state.regs[0], 0xC0);
        assert_eq!(session.state.regs[3], 0x44332212);
        assert_eq!(session.state.regs[6], 0xC8); // LEA, without FS base
        assert_eq!(session.bus.ler_u32(GuestVirt(session.state.fs_base + 0x14)).unwrap(), 0x44332212);
        assert_eq!(session.bus.ler_u32(GuestVirt(0x12020)).unwrap(), 0x44332211);
    }

    #[test]
    fn tail_call_do_kernel_preserva_retorno_do_guest() {
        let mut s = Session::load(include_bytes!("kernel_tail.xbe")).unwrap();
        assert_eq!(tail::run(&mut s.state, &mut s.bus, 10).unwrap(), Exit::KernelCall { ordinal: 8 });
        assert_eq!(s.state.retired, 1);
        s.complete_kernel_call(8, 123).unwrap();
        assert_eq!(tail::run(&mut s.state, &mut s.bus, 0).unwrap(), Exit::Returned);
        assert_eq!(s.state.regs[0], 123);
    }

    #[test]
    fn kernel_slot_nao_importado_nao_altera_cpu_nem_pilha() {
        let mut s = Session::load(include_bytes!("kernel_tail.xbe")).unwrap();
        s.bus.escrever_u32(GuestVirt(0x12000), 0x7FFE0008).unwrap(); // ordinal 1, not imported
        let before = s.state.clone();
        let ret = s.bus.ler_u32(GuestVirt(s.state.regs[ESP])).unwrap();
        assert!(tail::run(&mut s.state, &mut s.bus, 10).is_err());
        assert_eq!(s.state, before);
        assert_eq!(s.bus.ler_u32(GuestVirt(s.state.regs[ESP])).unwrap(), ret);
    }

    #[test]
    fn kernel_entry_recusa_trampolim_cuja_protecao_foi_alterada() {
        let mut s = Session::load(include_bytes!("kernel_tail.xbe")).unwrap();
        s.state.eip = 0x7FFE0040;
        let frame = s.bus.traduzir(GuestVirt(0x7FFE0000), Acesso::Leitura).unwrap();
        s.bus.mapear(GuestVirt(0x7FFE0000), frame, 1, Perm::RW).unwrap();
        assert!(tail::run(&mut s.state, &mut s.bus, 1).is_err());
    }

    #[test]
    fn acesso_fs_com_base_invalida_falha_sem_efeito_parcial() {
        let mut s = Session::load(include_bytes!("fs_fault.xbe")).unwrap();
        s.state.fs_base = 0;
        let before = s.state.clone();
        assert!(fs_fault::run(&mut s.state, &mut s.bus, 100).is_err());
        assert_eq!(s.state, before);
    }
}
