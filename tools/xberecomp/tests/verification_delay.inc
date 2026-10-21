#[cfg(test)]
mod delay_verification {
    use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
    use xbox_core::time::CPU_HZ;
    use xbox_mem::GuestVirt;
    use xbox_recomp::runtime::{Exit, Session, ESP};
    mod sleep { include!("kernel_delay.rs"); }

    const TSC_ANTES: u32 = 0x12100;
    const TSC_DEPOIS: u32 = 0x1210C;
    const STATUS: u32 = 0x12108;

    fn tsc(s: &Session, at: u32) -> u64 {
        let baixo = u64::from(s.bus.ler_u32(GuestVirt(at)).unwrap());
        let alto = u64::from(s.bus.ler_u32(GuestVirt(at + 4)).unwrap());
        (alto << 32) | baixo
    }

    fn ate_a_chamada(s: &mut Session) {
        assert_eq!(
            sleep::run(&mut s.state, &mut s.bus, 100).unwrap(),
            Exit::KernelCall { ordinal: 99 }
        );
    }

    fn saved(s: &Session) -> Vec<u8> {
        let mut out = SnapshotWriter::new();
        s.save(&mut out);
        out.finish()
    }

    /// Prova ponta a ponta no Rust **gerado**: o guest mede o próprio `Sleep` com `RDTSC`.
    ///
    /// Não há como o guest observar tempo senão pelos relógios derivados do Clock, então
    /// a diferença entre os dois `RDTSC` é a prova do que ele enxerga — não um valor que
    /// o teste lê direto do estado da sessão.
    #[test]
    fn guest_enxerga_o_sleep_pelo_rdtsc_com_o_intervalo_exato_mais_o_custo_das_instrucoes() {
        let mut s = Session::load(include_bytes!("kernel_delay.xbe")).unwrap();
        ate_a_chamada(&mut s);
        let antes_do_servico = s.state.clock.cycles();

        s.atender_kernel().unwrap();

        // O serviço em si avança exatamente 2 s de ciclos — nem mais, nem menos.
        assert_eq!(
            s.state.clock.cycles() - antes_do_servico,
            2 * CPU_HZ,
            "o avanço do Clock tem de ser o intervalo pedido, sem custo extra de HLE"
        );
        assert_eq!(s.kernel.atendidas(), 1);

        // Retorno aplicado: STATUS_SUCCESS em EAX e os 12 bytes de argumentos removidos.
        assert_eq!(s.state.regs[0], 0);

        assert_eq!(
            sleep::run(&mut s.state, &mut s.bus, 100).unwrap(),
            Exit::Returned
        );
        assert_eq!(s.bus.ler_u32(GuestVirt(STATUS)).unwrap(), 0, "STATUS_SUCCESS");

        // O que o guest vê: dois RDTSC separados por 2 s mais o custo sintético das
        // instruções entre eles (ADR-0025: um ciclo por instrução concluída).
        let decorrido = tsc(&s, TSC_DEPOIS) - tsc(&s, TSC_ANTES);
        assert!(decorrido >= 2 * CPU_HZ, "o guest viu menos que o intervalo: {decorrido}");
        assert!(
            decorrido - 2 * CPU_HZ < 32,
            "o excesso deveria ser só o custo de poucas instruções: {}",
            decorrido - 2 * CPU_HZ
        );
    }

    /// O avanço independe de como a execução é fatiada: orçamento 1 ou 100 dão o mesmo.
    #[test]
    fn fatiar_a_execucao_nao_muda_o_tempo_que_o_guest_enxerga() {
        let mut referencia = Session::load(include_bytes!("kernel_delay.xbe")).unwrap();
        ate_a_chamada(&mut referencia);
        referencia.atender_kernel().unwrap();
        assert_eq!(
            sleep::run(&mut referencia.state, &mut referencia.bus, 100).unwrap(),
            Exit::Returned
        );
        let esperado = tsc(&referencia, TSC_DEPOIS) - tsc(&referencia, TSC_ANTES);

        let mut fatiado = Session::load(include_bytes!("kernel_delay.xbe")).unwrap();
        loop {
            match sleep::run(&mut fatiado.state, &mut fatiado.bus, 1).unwrap() {
                Exit::Budget => {}
                Exit::KernelCall { .. } => fatiado.atender_kernel().unwrap(),
                Exit::Returned => break,
                outro => panic!("saída inesperada: {outro:?}"),
            }
        }
        assert_eq!(tsc(&fatiado, TSC_DEPOIS) - tsc(&fatiado, TSC_ANTES), esperado);
    }

    /// O snapshot tirado com a chamada pendente e o tirado depois do `Sleep` restauram
    /// para sessões que terminam idênticas, com o Clock no mesmo valor.
    #[test]
    fn snapshot_com_a_chamada_pendente_restaura_e_chega_ao_mesmo_estado_final() {
        let mut s = Session::load(include_bytes!("kernel_delay.xbe")).unwrap();
        ate_a_chamada(&mut s);
        let pendente = saved(&s);

        s.atender_kernel().unwrap();
        assert_eq!(sleep::run(&mut s.state, &mut s.bus, 100).unwrap(), Exit::Returned);
        let final_direto = saved(&s);

        let mut restaurada = Session::load(include_bytes!("kernel_delay.xbe")).unwrap();
        let mut inp = SnapshotReader::new(&pendente).unwrap();
        Snapshot::load(&mut restaurada, &mut inp).unwrap();
        assert_eq!(inp.remaining(), 0);
        // Pendente na chamada: o pedido de tempo ainda não existe, só é criado ao atender.
        restaurada.atender_kernel().unwrap();
        assert_eq!(
            sleep::run(&mut restaurada.state, &mut restaurada.bus, 100).unwrap(),
            Exit::Returned
        );
        assert!(
            saved(&restaurada) == final_direto,
            "restaurar a chamada pendente e prosseguir tem de dar o mesmo estado final"
        );
    }

    /// `TimeExhausted` deixa a CPU **intacta** na chamada pendente, como toda recusa.
    #[test]
    fn relogio_no_limite_recusa_antes_de_aplicar_o_retorno_e_nao_deixa_pedido_velho() {
        let mut s = Session::load(include_bytes!("kernel_delay.xbe")).unwrap();
        ate_a_chamada(&mut s);
        // Clock a 1 ciclo do limite: qualquer avanço de 2 s transborda `u64`.
        s.state.clock.set(u64::MAX - 1);
        let antes = s.state.clone();
        let esp = s.state.regs[ESP];

        let erro = s.atender_kernel().unwrap_err();
        assert!(
            matches!(erro, xbox_recomp::Error::TimeExhausted { .. }),
            "veio {erro:?}"
        );
        assert_eq!(s.state, antes, "a CPU não pode mudar quando o relógio estoura");
        assert_eq!(s.state.regs[ESP], esp);

        // O pedido não pode ter ficado pendente: o atendimento seguinte, com relógio
        // normal, aplicaria um avanço velho por cima do dele.
        assert_eq!(s.kernel.take_time_request(), None);
    }
}
