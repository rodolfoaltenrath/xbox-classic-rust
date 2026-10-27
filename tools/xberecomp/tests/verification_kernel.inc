
#[cfg(test)]
mod kernel_verification {
    use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
    use xbox_mem::{Acesso, GuestVirt, Perm};
    use xbox_recomp::runtime::{Session, Exit, State, ESP};
    mod abi { include!("kernel_abi.rs"); }
    mod tail { include!("kernel_tail.rs"); }
    mod fs_fault { include!("fs_fault.rs"); }
    mod dispatch { include!("kernel_dispatch.rs"); }
    mod mutant { include!("kernel_mutant.rs"); }

    #[test]
    fn abi_tls_fs_e_snapshot_pendente_funcionam_em_rust_nativo() {
        let mut session = Session::load(include_bytes!("kernel_abi.xbe")).unwrap();
        let initial_esp = session.state.regs[ESP];
        assert_eq!(abi::run(&mut session.state, &mut session.bus, 0).unwrap(), Exit::Budget);
        assert_eq!(abi::run(&mut session.state, &mut session.bus, 100).unwrap(), Exit::KernelCall { ordinal: 181 });
        let call = session.kernel_call().unwrap();
        assert_eq!(call.argument(&session.bus, 0).unwrap(), 0x1122);
        let before = session.state.clone();
        assert!(session.complete_kernel_call(8, 0).is_err());
        assert_eq!(session.state, before);
        session.complete_kernel_call(181, 0xA0).unwrap();
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

    /// Caminho completo no Rust **gerado**: sair na fronteira, o dispatcher executar o
    /// serviço, o efeito aparecer na memória guest e a execução retomar até o fim.
    #[test]
    fn dispatcher_atende_servico_implementado_e_a_execucao_retoma() {
        let mut s = Session::load(include_bytes!("kernel_dispatch.xbe")).unwrap();
        assert_eq!(
            dispatch::run(&mut s.state, &mut s.bus, 100).unwrap(),
            Exit::KernelCall { ordinal: 289 }
        );
        s.atender_kernel().unwrap();
        assert_eq!(s.kernel.atendidas(), 1);
        assert_eq!(s.kernel.recusadas(), 0);

        // Efeito do serviço: ANSI_STRING preenchida apontando para a origem.
        assert_eq!(s.bus.ler_u16(GuestVirt(0x12010)).unwrap(), 14);
        assert_eq!(s.bus.ler_u16(GuestVirt(0x12012)).unwrap(), 15);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12014)).unwrap(), 0x12020);

        // Stdcall limpa os 8 bytes de argumento e a execução segue até o RET.
        assert_eq!(
            dispatch::run(&mut s.state, &mut s.bus, 100).unwrap(),
            Exit::Returned
        );
    }

    /// Serviço faltante é relatado com nome, ordinal e chamador, e a CPU fica intacta:
    /// é isso que permite implementar o serviço e retomar do mesmo ponto.
    #[test]
    fn dispatcher_recusa_servico_faltante_sem_tocar_a_cpu() {
        let mut s = Session::load(include_bytes!("kernel_abi.xbe")).unwrap();
        assert_eq!(
            abi::run(&mut s.state, &mut s.bus, 100).unwrap(),
            Exit::KernelCall { ordinal: 181 }
        );
        let antes = s.state.clone();
        let erro = s.atender_kernel().unwrap_err().to_string();
        assert!(erro.contains("MmQueryStatistics"), "{erro}");
        assert!(erro.contains("181"), "{erro}");
        assert!(erro.contains("0x00001122"), "argumento no relato: {erro}");
        assert_eq!(s.state, antes, "CPU alterada por uma recusa");
        assert_eq!(s.kernel.recusadas(), 1);
        assert_eq!(s.kernel.atendidas(), 0);
    }

    #[test]
    fn mutexes_e_continuacao_sobrevivem_ao_snapshot_integral_da_sessao() {
        let image = include_bytes!("kernel_mutant.xbe");
        let mut s = Session::load(image).unwrap();
        let initial_esp = s.state.regs[ESP];
        assert_eq!(mutant::run(&mut s.state, &mut s.bus, 100).unwrap(), Exit::KernelCall { ordinal: 192 });
        s.atender_kernel().unwrap();
        assert_eq!(s.state.regs[0], 0);
        assert_eq!(s.state.regs[ESP], initial_esp);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12010)).unwrap(), 4);
        assert_eq!(s.kernel.mutant(4).unwrap().owner, None);
        assert_eq!(mutant::run(&mut s.state, &mut s.bus, 100).unwrap(), Exit::KernelCall { ordinal: 192 });
        let checkpoint = session_bytes(&s);
        s.atender_kernel().unwrap();
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12014)).unwrap(), 8);
        assert_eq!(s.kernel.mutant(8).unwrap().owner, Some(1));
        assert_eq!(mutant::run(&mut s.state, &mut s.bus, 100).unwrap(), Exit::Returned);
        let complete = session_bytes(&s);

        let mut restored = Session::load(image).unwrap();
        let mut reader = SnapshotReader::new(&checkpoint).unwrap();
        Snapshot::load(&mut restored, &mut reader).unwrap();
        assert_eq!(reader.remaining(), 0);
        assert_eq!(session_bytes(&restored), checkpoint);
        assert!(restored.kernel.mutant(8).is_none());
        restored.atender_kernel().unwrap();
        assert_eq!(mutant::run(&mut restored.state, &mut restored.bus, 100).unwrap(), Exit::Returned);
        assert_eq!(session_bytes(&restored), complete);
    }

    fn session_bytes(s: &Session) -> Vec<u8> {
        let mut out = SnapshotWriter::new(); s.save(&mut out); out.finish()
    }

    #[test]
    fn retorno_invalido_e_recusado_antes_de_criar_mutex_ou_escrever_saida() {
        let mut s = Session::load(include_bytes!("kernel_mutant.xbe")).unwrap();
        mutant::run(&mut s.state, &mut s.bus, 100).unwrap();
        s.bus.escrever_u32(GuestVirt(s.state.regs[ESP]), 0x50000000).unwrap();
        let before = s.state.clone();
        assert!(s.atender_kernel().is_err());
        assert_eq!(s.state, before);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12010)).unwrap(), 0);
        assert!(s.kernel.mutant(4).is_none());
        assert_eq!(s.kernel.atendidas(), 0);
    }

    #[test]
    fn saida_sobre_retorno_usa_continuacao_capturada_antes_do_servico() {
        let mut s = Session::load(include_bytes!("kernel_mutant.xbe")).unwrap();
        mutant::run(&mut s.state, &mut s.bus, 100).unwrap();
        let call = s.kernel_call().unwrap();
        let esp = s.state.regs[ESP];
        s.bus.escrever_u32(GuestVirt(esp + 4), esp).unwrap();
        s.atender_kernel().unwrap();
        assert_eq!(s.state.eip, call.return_pc);
        assert_eq!(s.state.regs[ESP], call.return_esp);
        assert_eq!(s.bus.ler_u32(GuestVirt(esp)).unwrap(), 4);
        assert!(s.kernel.mutant(4).is_some());
        assert_eq!(mutant::run(&mut s.state, &mut s.bus, 100).unwrap(), Exit::KernelCall { ordinal: 192 });
    }

    #[test]
    fn erro_ntstatus_retoma_guest_sem_objeto_fantasma() {
        let mut s = Session::load(include_bytes!("kernel_mutant.xbe")).unwrap();
        mutant::run(&mut s.state, &mut s.bus, 100).unwrap();
        s.bus.escrever_u32(GuestVirt(s.state.regs[ESP] + 4), 0).unwrap();
        s.atender_kernel().unwrap();
        assert_eq!(s.state.regs[0], 0xC0000005);
        assert!(s.kernel.mutant(4).is_none());
        assert_eq!(mutant::run(&mut s.state, &mut s.bus, 100).unwrap(), Exit::KernelCall { ordinal: 192 });
        s.atender_kernel().unwrap();
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12014)).unwrap(), 4);
        assert_eq!(mutant::run(&mut s.state, &mut s.bus, 100).unwrap(), Exit::Returned);
    }
}
