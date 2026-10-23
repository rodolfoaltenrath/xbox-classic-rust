#[cfg(test)]
mod interrupt_verification {
    use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
    use xbox_core::time::VBLANK_NTSC_MHZ;
    use xbox_core::Clock;
    use xbox_kernel::dispatch::Erro;
    use xbox_mem::GuestVirt;
    use xbox_recomp::runtime::{Desfecho, Limites, Session};
    use xbox_recomp::Error;
    mod ok { include!("interrupt_ok.rs"); }
    mod sem_fonte { include!("interrupt_sem_fonte.rs"); }
    mod presa { include!("interrupt_presa.rs"); }

    const LIMITES: Limites = Limites { blocos_por_fatia: 10_000, fatias: 200 };

    fn u32_at(s: &Session, at: u32) -> u32 {
        s.bus.ler_u32(GuestVirt(at)).unwrap()
    }

    fn rodar_ok() -> Session {
        let mut s = Session::load(include_bytes!("interrupt_ok.xbe")).unwrap();
        let desfecho = s.conduzir(LIMITES, ok::run, |_| Ok(false)).unwrap();
        assert_eq!(desfecho, Desfecho::Retornou);
        s
    }

    /// O ciclo inteiro no Rust **gerado**: o guest instala ISR e DPC, habilita o vblank e
    /// espera um evento que só o DPC pulsa. A espera bloqueia, o tempo corre até o vblank,
    /// o ISR roda, enfileira o DPC, o DPC roda e pulsa, e a espera é satisfeita.
    #[test]
    fn vblank_entrega_isr_depois_dpc_e_o_pulso_libera_a_espera() {
        let s = rodar_ok();
        assert_eq!(u32_at(&s, 0x12B28), 1, "KeConnectInterrupt devolve TRUE");
        assert_eq!(u32_at(&s, 0x12B20), 0, "NtWaitForSingleObject: STATUS_WAIT_0");
        assert_eq!(u32_at(&s, 0x12A00), 1, "ISR rodou uma vez");
        assert_eq!(u32_at(&s, 0x12A04), 1, "DPC rodou uma vez");
        assert_eq!(u32_at(&s, 0x12A18), 1, "o ISR veio primeiro");
        assert_eq!(u32_at(&s, 0x12A1C), 2, "o DPC veio depois");
    }

    #[test]
    fn isr_e_dpc_recebem_os_argumentos_do_contrato() {
        let s = rodar_ok();
        assert_eq!(u32_at(&s, 0x12A10), 0x12800, "ISR: Interrupt* é a KINTERRUPT");
        assert_eq!(u32_at(&s, 0x12A08), 0x15, "ISR: ServiceContext");
        assert_eq!(u32_at(&s, 0x12A14), 0x12900, "DPC: Dpc* é a KDPC");
        assert_eq!(u32_at(&s, 0x12A0C), 0xD1, "DPC: DeferredContext");
    }

    #[test]
    fn kernel_preenche_kinterrupt_e_kdpc_nos_layouts_do_nxdk() {
        let s = rodar_ok();
        assert_eq!(u32_at(&s, 0x12B24), 0x33, "vetor = 0x30 + nível 3");
        assert_eq!(s.bus.ler_u8(GuestVirt(0x12B00)).unwrap(), 24, "KIRQL = 27 - 3");
        assert_eq!(u32_at(&s, 0x12808), 3, "BusInterruptLevel");
        assert_eq!(u32_at(&s, 0x1280C), 24, "Irql");
        assert_eq!(s.bus.ler_u8(GuestVirt(0x12810)).unwrap(), 1, "Connected");
        assert_eq!(s.bus.ler_u8(GuestVirt(0x12811)).unwrap(), 1, "ShareVector");
        assert_eq!(s.bus.ler_u8(GuestVirt(0x12812)).unwrap(), 0, "Mode: LevelSensitive");
        assert_eq!(u32_at(&s, 0x12804), 0x15, "ServiceContext");
        assert_eq!(s.bus.ler_u16(GuestVirt(0x12900)).unwrap(), 19, "KDPC.Type = DpcObject");
        assert_eq!(s.bus.ler_u8(GuestVirt(0x12902)).unwrap(), 0, "o DPC já saiu da fila");
        assert_eq!(u32_at(&s, 0x12910), 0xD1, "DeferredContext");
    }

    #[test]
    fn o_tempo_correu_ate_o_primeiro_vblank_e_a_linha_foi_limpa() {
        let s = rodar_ok();
        let periodo = Clock::frame_cycles(VBLANK_NTSC_MHZ);
        let ciclos = s.state.clock.cycles();
        assert!(ciclos >= periodo, "esperou menos de um quadro: {ciclos} < {periodo}");
        assert!(ciclos < 2 * periodo, "passou de um quadro inteiro: {ciclos}");
        assert_eq!(s.vblanks(), 1);
        assert!(!s.bus.nv2a_irq().line_asserted(), "o DPC limpou o vblank");
        // O DPC religou o INTA: a próxima pendência chegaria à CPU.
        assert_eq!(u32_at(&s, 0xFD00_0140), 1);
    }

    #[test]
    fn contexto_interrompido_volta_inalterado() {
        let s = rodar_ok();
        // O main retornou ao sentinela com a pilha onde começou: nenhum quadro de
        // ISR/DPC vazou. ESP inicial = topo da pilha menos o endereço de retorno.
        let inicial = Session::load(include_bytes!("interrupt_ok.xbe")).unwrap();
        assert_eq!(s.state.regs[4], inicial.state.regs[4] + 4);
    }

    #[test]
    fn sem_vblank_habilitado_a_espera_continua_sendo_erro_identificavel() {
        let mut s = Session::load(include_bytes!("interrupt_sem_fonte.xbe")).unwrap();
        // O scheduler novo relata ausência de progresso sem pular Clock; o
        // perfil migrado conserva a recusa anterior do dispatcher.
        let mut w=SnapshotWriter::new();s.save(&mut w);let initial=w.finish();
        let error=s.conduzir(LIMITES,sem_fonte::run,|_|Ok(false)).unwrap_err();assert!(matches!(error,Error::WouldBlock{..}));assert_eq!(s.state.clock.cycles(),s.state.retired);
        s.load(&mut SnapshotReader::new(&super::legacy_scheduler_snapshot(&initial)).unwrap()).unwrap();

        let erro = s.conduzir(LIMITES, sem_fonte::run, |_| Ok(false)).unwrap_err();
        let texto = erro.to_string();
        assert!(
            matches!(erro, Error::Kernel(Erro::VarianteNaoImplementada { .. })),
            "{texto}"
        );
        assert!(texto.contains("scheduler ou interrupção"), "{texto}");
        assert_eq!(s.vblanks(), 0, "nenhum tempo correu: não havia o que esperar");
    }

    #[test]
    fn isr_que_nao_retorna_vira_erro_em_vez_de_laco_infinito() {
        let mut s = Session::load(include_bytes!("interrupt_presa.xbe")).unwrap();
        let erro = s.conduzir(LIMITES, presa::run, |_| Ok(false)).unwrap_err();
        assert!(matches!(erro, Error::InterruptStuck { .. }), "{erro}");
    }

    #[test]
    fn snapshot_da_sessao_preserva_vblanks_conexao_e_o_registrador_de_interrupcao() {
        let s = rodar_ok();
        let mut out = SnapshotWriter::new();
        s.save(&mut out);
        let bytes = out.finish();
        let mut restaurada = Session::load(include_bytes!("interrupt_ok.xbe")).unwrap();
        restaurada.load(&mut SnapshotReader::new(&bytes).unwrap()).unwrap();
        assert_eq!(restaurada.vblanks(), s.vblanks());
        assert_eq!(restaurada.bus.nv2a_irq(), s.bus.nv2a_irq());
        let mut de_novo = SnapshotWriter::new();
        restaurada.save(&mut de_novo);
        assert_eq!(de_novo.finish(), bytes, "ida e volta idêntica");
    }
    #[test]
    fn guest_observa_dirql_no_isr_dispatch_no_dpc_e_passive_apos_retorno(){let s=rodar_ok();assert_eq!(u32_at(&s,0x12A34),24);assert_eq!(u32_at(&s,0x12A38),2);assert_eq!(s.kernel.irql(),xbox_kernel::dispatch::Irql::Passive);}
    #[test]
    fn perfil_legado_conserva_irql_inalterado_no_isr(){let mut s=Session::load(include_bytes!("interrupt_ok.xbe")).unwrap();s.state.interrupt_profile=xbox_recomp::runtime::InterruptProfile::LegacyBoundary;assert_eq!(s.conduzir(LIMITES,ok::run,|_|Ok(false)).unwrap(),Desfecho::Retornou);assert_eq!(u32_at(&s,0x12A34),0);assert_eq!(u32_at(&s,0x12A38),2);}

}
