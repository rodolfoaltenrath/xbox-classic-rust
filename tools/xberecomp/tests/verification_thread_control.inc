#[cfg(test)]
mod thread_control_verification {
    use xbox_core::{Snapshot, SnapshotReader, SnapshotWriter};
    use xbox_kernel::dispatch::STATUS_NO_YIELD_PERFORMED;
    use xbox_mem::GuestVirt;
    use xbox_recomp::Error;
    use xbox_recomp::runtime::{Desfecho, Exit, Limites, Session};
    mod guest { include!("kernel_thread_control.rs"); }
    mod deadlock { include!("kernel_thread_deadlock.rs"); }

    const LIMITES: Limites = Limites { blocos_por_fatia: 1000, fatias: 40 };

    fn loaded() -> Session {
        Session::load(include_bytes!("kernel_thread_control.xbe")).unwrap()
    }
    fn word(s: &Session, at: u32) -> u32 {
        s.bus.ler_u32(GuestVirt(at)).unwrap()
    }
    fn saved(s: &Session) -> Vec<u8> {
        let mut w = SnapshotWriter::new();
        s.save(&mut w);
        w.finish()
    }
    fn boundary(s: &mut Session, ordinal: u32) {
        assert_eq!(
            guest::run(&mut s.state, &mut s.bus, 1000).unwrap(),
            Exit::KernelCall { ordinal }
        );
    }
    fn serve(s: &mut Session) {
        s.atender_kernel().unwrap();
    }
    fn finish(s: &mut Session) {
        assert_eq!(s.conduzir(LIMITES, guest::run, |_| Ok(false)).unwrap(), Desfecho::Retornou);
    }

    fn assert_observations(s: &Session) {
        assert_eq!(word(s, 0x12110), STATUS_NO_YIELD_PERFORMED, "filha ainda não iniciada");
        assert_eq!(word(s, 0x12108), 1, "NtResumeThread: contagem anterior da criação suspensa");
        assert_eq!(word(s, 0x12114), 0, "NtResumeThread: sucesso");
        assert_eq!(word(s, 0x12118), 1, "a filha rodou uma vez antes de se suspender");
        assert_eq!(word(s, 0x1211C), STATUS_NO_YIELD_PERFORMED, "filha suspensa não recebe a CPU");
        assert_eq!(word(s, 0x1210C), 0, "NtSuspendThread: contagem anterior da autossuspensão");
        assert_eq!(word(s, 0x12124), 1, "segundo resume: a filha estava suspensa uma vez");
        assert_eq!(word(s, 0x1212C), 0, "NtYieldExecution cedeu de verdade");
        assert_eq!(word(s, 0x12128), 1, "a filha continuou depois de retomada");
        assert_eq!(word(s, 0x12134), 0, "a autossuspensão voltou com sucesso");
        assert_eq!(word(s, 0x12130), 0, "NtWaitForSingleObject da filha terminada");
        assert_eq!(s.kernel.child_suspend_count(), 0);
        assert_eq!(s.kernel.recusadas(), 0);
    }

    /// O ciclo inteiro no Rust gerado: criação suspensa, retomada, autossuspensão, cessão.
    #[test]
    fn suspensao_retomada_e_cessao_afetam_quem_roda() {
        let mut s = loaded();
        finish(&mut s);
        assert_observations(&s);
    }

    #[test]
    fn replay_a_partir_da_filha_criada_suspensa_e_nunca_iniciada() {
        let mut s = loaded();
        boundary(&mut s, 255);
        serve(&mut s);
        assert_eq!(s.kernel.child_suspend_count(), 1);
        let checkpoint = saved(&s);
        finish(&mut s);
        let complete = saved(&s);
        let mut r = loaded();
        r.load(&mut SnapshotReader::new(&checkpoint).unwrap()).unwrap();
        assert!(saved(&r) == checkpoint, "ida e volta idêntica");
        finish(&mut r);
        assert!(saved(&r) == complete, "o replay chega ao mesmo estado final");
        assert_observations(&r);
    }

    #[test]
    fn replay_com_a_filha_suspensa_depois_de_rodar() {
        let mut s = loaded();
        boundary(&mut s, 255);
        serve(&mut s); // cria suspensa
        boundary(&mut s, 238);
        serve(&mut s); // cede sem ninguém
        boundary(&mut s, 224);
        serve(&mut s); // retoma: a filha inicia
        boundary(&mut s, 231);
        serve(&mut s); // a filha se suspende; o pai volta a rodar
        assert_eq!(s.kernel.current_thread(), 1);
        assert_eq!(s.kernel.child_suspend_count(), 1);
        let checkpoint = saved(&s);
        finish(&mut s);
        let complete = saved(&s);
        let mut r = loaded();
        r.load(&mut SnapshotReader::new(&checkpoint).unwrap()).unwrap();
        assert!(saved(&r) == checkpoint);
        finish(&mut r);
        assert!(saved(&r) == complete);
        assert_observations(&r);
    }

    #[test]
    fn contagem_corrompida_no_snapshot_e_recusada_sem_alterar_a_sessao() {
        let mut s = loaded();
        boundary(&mut s, 255);
        serve(&mut s);
        let good = saved(&s);
        let at = good.windows(4).rposition(|v| v == b"THRD").unwrap();
        // Zera a contagem: a filha Ready deixa de ser "criada suspensa", o que o scheduler
        // recusa porque a filha não iniciada não pode existir sem suspensão.
        let mut bad = good.clone();
        let suspend = at + 8 + 1 + 8 * 4 + 1 + 4 + 1;
        assert_eq!(bad[suspend], 1);
        bad[suspend] = 0;
        let mut r = loaded();
        assert!(r.load(&mut SnapshotReader::new(&bad).unwrap()).is_err());
    }

    #[test]
    fn todas_as_threads_paradas_e_deadlock_identificavel() {
        let mut s = Session::load(include_bytes!("kernel_thread_deadlock.xbe")).unwrap();
        let erro = s.conduzir(LIMITES, deadlock::run, |_| Ok(false)).unwrap_err();
        assert!(matches!(erro, Error::WouldBlock { .. }), "{erro}");
        assert_eq!(s.kernel.child_suspend_count(), 1, "a filha ficou suspensa");
    }
}
