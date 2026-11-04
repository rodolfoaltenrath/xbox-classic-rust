#[cfg(test)]
mod firmware_verification {
    use xbox_core::{Snapshot, SnapshotReader, SnapshotWriter};
    use xbox_mem::GuestVirt;
    use xbox_recomp::Error;
    use xbox_recomp::runtime::{Desfecho, Limites, Session};
    mod guest { include!("kernel_firmware.rs"); }

    const LIMITES: Limites = Limites { blocos_por_fatia: 1000, fatias: 100 };

    fn loaded() -> Session {
        Session::load(include_bytes!("kernel_firmware.xbe")).unwrap()
    }
    fn word(s: &Session, at: u32) -> u32 {
        s.bus.ler_u32(GuestVirt(at)).unwrap()
    }
    fn saved(s: &Session) -> Vec<u8> {
        let mut w = SnapshotWriter::new();
        s.save(&mut w);
        w.finish()
    }

    /// No Rust **gerado**: o título guarda um endereço de dados salvos, imprime, registra duas
    /// notificações de desligamento e volta ao firmware. A execução termina com esse desfecho,
    /// as notificações rodam da maior prioridade para a menor e nada depois da chamada executa.
    #[test]
    fn retorno_ao_firmware_chama_as_notificacoes_por_prioridade_e_encerra() {
        let mut s = loaded();
        let desfecho = s.conduzir(LIMITES, guest::run, |_| Ok(false)).unwrap();
        assert_eq!(desfecho, Desfecho::RetornoAoFirmware(1), "HalRebootRoutine");
        assert_eq!(word(&s, 0x12504), 1, "prioridade 5 roda primeiro");
        assert_eq!(word(&s, 0x12500), 2, "prioridade 1 roda depois");
        assert_eq!(word(&s, 0x12518), 0, "a chamada não retorna: o código seguinte não roda");
        assert_eq!(word(&s, 0x12510), 0x1234, "AvGetSavedDataAddress devolve o que o Set guardou");
        assert_eq!(s.kernel.video().saved_data_address(), 0x1234);
        assert_eq!(s.kernel.debug_output(), b"n=7");
        assert_eq!(word(&s, 0x12514), 0, "DbgPrint devolve STATUS_SUCCESS");
        assert_eq!(s.kernel.recusadas(), 0);
    }

    #[test]
    fn o_estado_depois_do_desfecho_e_salvavel_e_restauravel() {
        let mut s = loaded();
        s.conduzir(LIMITES, guest::run, |_| Ok(false)).unwrap();
        let bytes = saved(&s);
        let mut r = loaded();
        r.load(&mut SnapshotReader::new(&bytes).unwrap()).unwrap();
        assert!(saved(&r) == bytes, "ida e volta idêntica");
        assert_eq!(r.kernel.video().saved_data_address(), 0x1234);
        // O transcrito do DbgPrint é diagnóstico do host: não faz parte do snapshot.
        assert_eq!(r.kernel.debug_output(), b"");
    }

    #[test]
    fn quem_nao_conduz_recebe_erro_em_vez_de_um_retorno_que_nao_existe() {
        let mut s = loaded();
        let mut chamadas = 0;
        let erro = loop {
            assert!(chamadas < 100, "o programa devia chegar a HalReturnToFirmware");
            chamadas += 1;
            match guest::run(&mut s.state, &mut s.bus, 1000).unwrap() {
                xbox_recomp::runtime::Exit::KernelCall { .. } => {
                    if let Err(e) = s.atender_kernel() {
                        break e;
                    }
                }
                other => panic!("{other:?}"),
            }
        };
        assert!(matches!(erro, Error::FirmwareReturn { routine: 1 }), "{erro}");
    }
}
