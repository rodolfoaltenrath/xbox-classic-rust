#[cfg(test)]
mod waits_verification {
    use xbox_core::time::Clock;
    use xbox_core::{Snapshot, SnapshotReader, SnapshotWriter};
    use xbox_kernel::dispatch::STATUS_TIMEOUT;
    use xbox_mem::GuestVirt;
    use xbox_recomp::Error;
    use xbox_recomp::runtime::{Desfecho, Limites, Session};
    mod guest { include!("kernel_waits.rs"); }

    fn loaded() -> Session {
        Session::load(include_bytes!("kernel_waits.xbe")).unwrap()
    }
    fn word(s: &Session, at: u32) -> u32 {
        s.bus.ler_u32(GuestVirt(at)).unwrap()
    }
    fn saved(s: &Session) -> Vec<u8> {
        let mut w = SnapshotWriter::new();
        s.save(&mut w);
        w.finish()
    }
    fn run(s: &mut Session, fatias: u32) -> Result<Desfecho, Error> {
        s.conduzir(Limites { blocos_por_fatia: 1000, fatias }, guest::run, |_| Ok(false))
    }
    fn finish(s: &mut Session) {
        assert_eq!(run(s, 200).unwrap(), Desfecho::Retornou);
    }

    /// No Rust **gerado**, com o scheduler de duas threads: um timeout que expira, uma
    /// espera por qualquer de dois eventos satisfeita pela filha e uma espera por todos
    /// sem esperar.
    #[test]
    fn timeout_espera_multipla_e_poll_no_scheduler() {
        let mut s = loaded();
        finish(&mut s);
        assert_eq!(word(&s, 0x12190), STATUS_TIMEOUT, "ninguém sinalizou E0: venceu o timeout");
        assert_eq!(word(&s, 0x12194), 1, "WaitAny: a filha sinalizou E1, índice 1");
        assert_eq!(word(&s, 0x12198), STATUS_TIMEOUT, "WaitAll com timeout zero e E0 apagado");
        // O tempo guest correu: 2 ms do primeiro timeout + 1 ms do sono da filha, e muito
        // menos que os 100 ms de prazo da segunda espera, que foi satisfeita antes.
        let ciclos = s.state.clock.cycles();
        assert!(ciclos >= Clock::ms_to_cycles(3), "só {ciclos} ciclos");
        assert!(ciclos < Clock::ms_to_cycles(50), "esperou o prazo inteiro: {ciclos}");
        assert_eq!(s.kernel.recusadas(), 0);
    }

    /// Para cada fatia em que a execução pode parar (inclusive com o pai bloqueado em
    /// `KernelWaitUntil`), o snapshot restaurado chega ao mesmo estado final.
    #[test]
    fn replay_de_qualquer_fronteira_de_fatia_chega_ao_mesmo_estado() {
        let mut reference = loaded();
        finish(&mut reference);
        let complete = saved(&reference);
        let mut parou_bloqueado = 0;
        for fatias in 1..=30 {
            let mut s = loaded();
            match run(&mut s, fatias) {
                Ok(Desfecho::Retornou) => break,
                Err(Error::BudgetExhausted { .. }) => {}
                other => panic!("fatias={fatias}: {other:?}"),
            }
            let checkpoint = saved(&s);
            if checkpoint.windows(4).any(|w| w == b"WAIT")
                && checkpoint.windows(13).any(|w| w.starts_with(b"WAIT\x01\0\0\0\x01"))
            {
                parou_bloqueado += 1;
            }
            let mut r = loaded();
            r.load(&mut SnapshotReader::new(&checkpoint).unwrap()).unwrap();
            assert!(saved(&r) == checkpoint, "fatias={fatias}: ida e volta");
            finish(&mut r);
            assert!(saved(&r) == complete, "fatias={fatias}: estado final diverge");
        }
        assert!(parou_bloqueado > 0, "nenhum ponto de parada pegou uma espera pendente");
    }

    #[test]
    fn corromper_o_prazo_da_espera_pendente_e_recusado() {
        // Para cedo o bastante para o pai estar bloqueado com prazo.
        let mut s = loaded();
        let mut bloqueado = None;
        for fatias in 1..=30 {
            let mut t = loaded();
            if run(&mut t, fatias).is_err() {
                let bytes = saved(&t);
                if let Some(at) = bytes.windows(4).rposition(|w| w == b"WAIT")
                    && bytes[at + 8] == 1
                {
                    bloqueado = Some((bytes, at));
                    break;
                }
            }
        }
        let (bytes, at) = bloqueado.expect("nenhuma fatia parou com espera pendente");
        // Marca "tem prazo" mas zera o prazo: não canônico.
        let mut bad = bytes.clone();
        assert_eq!(bad[at + 13], 1);
        bad[at + 14..at + 22].copy_from_slice(&0u64.to_le_bytes());
        assert!(s.load(&mut SnapshotReader::new(&bad).unwrap()).is_err());
        // Thread fora do perfil.
        let mut bad = bytes.clone();
        bad[at + 9..at + 13].copy_from_slice(&7u32.to_le_bytes());
        assert!(loaded().load(&mut SnapshotReader::new(&bad).unwrap()).is_err());
    }
}
