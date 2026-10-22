//! `Session::conduzir` com um `run` de mentira: o laço de execução sem projeto gerado.
use xbox_formats::fixtures::xbe_com_codigo;
use xbox_recomp::Error;
use xbox_recomp::runtime::{Desfecho, Exit, Limites, Session};

const LIMITES: Limites = Limites {
    blocos_por_fatia: 10,
    fatias: 5,
};

fn sessao() -> Session {
    Session::load(&xbe_com_codigo(&[0xC3]).unwrap()).unwrap()
}

#[test]
fn retorno_do_titulo_encerra_com_retornou() {
    let mut s = sessao();
    let mut chamadas = 0;
    let r = s.conduzir(
        LIMITES,
        |st, _, _| {
            chamadas += 1;
            st.returned = true;
            Ok(Exit::Returned)
        },
        |_| panic!("não há o que apresentar sem espera nem fim de fatia"),
    );
    assert_eq!(r.unwrap(), Desfecho::Retornou);
    assert_eq!(chamadas, 1);
}

#[test]
fn fechar_a_janela_durante_a_execucao_para_sem_erro_e_sem_novas_fatias() {
    let mut s = sessao();
    let mut fatias = 0;
    let mut apresentacoes = 0;
    let r = s.conduzir(
        LIMITES,
        |_, _, _| {
            fatias += 1;
            Ok(Exit::Budget)
        },
        |_| {
            apresentacoes += 1;
            Ok(apresentacoes == 2)
        },
    );
    assert_eq!(r.unwrap(), Desfecho::ParadoPeloHost);
    assert_eq!(fatias, 2, "continuou executando depois do pedido de parar");
    assert_eq!(apresentacoes, 2);
}

#[test]
fn estado_fica_intacto_e_retomavel_apos_parada_pelo_host() {
    let mut s = sessao();
    let antes = s.state.eip;
    s.conduzir(LIMITES, |_, _, _| Ok(Exit::Budget), |_| Ok(true))
        .unwrap();
    assert_eq!(s.state.eip, antes);
    // Uma segunda condução prossegue normalmente do mesmo estado.
    let r = s.conduzir(
        LIMITES,
        |st, _, _| {
            st.returned = true;
            Ok(Exit::Returned)
        },
        |_| Ok(false),
    );
    assert_eq!(r.unwrap(), Desfecho::Retornou);
}

#[test]
fn orcamento_esgotado_e_erro_com_o_total_e_o_relogio() {
    let mut s = sessao();
    let mut fatias = 0;
    let r = s.conduzir(
        LIMITES,
        |_, _, _| {
            fatias += 1;
            Ok(Exit::Budget)
        },
        |_| Ok(false),
    );
    assert_eq!(fatias, 5);
    match r.unwrap_err() {
        Error::BudgetExhausted { blocks, .. } => assert_eq!(blocks, 50),
        e => panic!("erro inesperado: {e}"),
    }
}

#[test]
fn breakpoint_e_hlt_viram_erro_com_o_proximo_eip() {
    let mut s = sessao();
    s.state.eip = 0x11001;
    let e = s
        .conduzir(
            LIMITES,
            |_, _, _| Ok(Exit::Breakpoint { pc: 0x11000 }),
            |_| Ok(false),
        )
        .unwrap_err();
    let texto = e.to_string();
    assert!(
        texto.contains("#BP (vetor 3) por INT3 em 0x00011000"),
        "{texto}"
    );
    assert!(texto.contains("próximo EIP=0x00011001"), "{texto}");

    let e = s
        .conduzir(
            LIMITES,
            |_, _, _| {
                Ok(Exit::Halted {
                    pc: 0x11001,
                    interrupts_enabled: false,
                })
            },
            |_| Ok(false),
        )
        .unwrap_err();
    let texto = e.to_string();
    assert!(texto.contains("HLT em 0x00011001"), "{texto}");
    assert!(texto.contains("IF=0"), "{texto}");
}

#[test]
fn falha_do_run_e_do_apresentador_propagam() {
    let mut s = sessao();
    let e = s
        .conduzir(
            LIMITES,
            |_, _, _| Err(Error::UnknownBlock(0x1234)),
            |_| Ok(false),
        )
        .unwrap_err();
    assert!(matches!(e, Error::UnknownBlock(0x1234)));

    let e = s
        .conduzir(
            LIMITES,
            |_, _, _| Ok(Exit::Budget),
            |_| Err(Error::Presentation("sem display".into())),
        )
        .unwrap_err();
    assert!(e.to_string().contains("sem display"));
}

mod tick_count {
    use super::*;
    use xbox_kernel::loader::data::{DATA_RO_BASE, OFF_KE_TICK_COUNT};
    use xbox_mem::GuestVirt;

    const ENDERECO: GuestVirt = GuestVirt(DATA_RO_BASE + OFF_KE_TICK_COUNT);

    fn sessao_com_tick() -> Session {
        let xbe = xbox_formats::fixtures::xbe_com_imports(&[0xC3], &[156]).unwrap();
        Session::load(&xbe).unwrap()
    }

    #[test]
    fn imagem_sem_dados_importados_nao_falha_ao_publicar() {
        let mut s = sessao();
        s.publicar_tick_count().unwrap();
        assert!(
            s.bus.ler_u32(ENDERECO).is_err(),
            "a região não deveria existir"
        );
    }

    #[test]
    fn comeca_em_zero_e_o_guest_nao_consegue_escrever() {
        let mut s = sessao_com_tick();
        assert_eq!(s.bus.ler_u32(ENDERECO).unwrap(), 0);
        assert!(
            s.bus.escrever_u32(ENDERECO, 7).is_err(),
            "o guest só lê o contador do kernel"
        );
    }

    #[test]
    fn fim_de_fatia_publica_o_relogio_guest_em_milissegundos() {
        let mut s = sessao_com_tick();
        s.conduzir(
            LIMITES,
            |st, _, _| {
                // 1,5 s de tempo guest decorridos durante a fatia.
                let ciclos = xbox_core::Clock::hundred_ns_to_cycles(15_000_000).unwrap();
                st.clock.set(ciclos);
                st.returned = true;
                Ok(Exit::Returned)
            },
            |_| Ok(false),
        )
        .unwrap();
        let ms = s.bus.ler_u32(ENDERECO).unwrap();
        assert!((1499..=1500).contains(&ms), "KeTickCount = {ms}");
    }

    #[test]
    fn valor_de_32_bits_da_a_volta_em_vez_de_saturar() {
        let mut s = sessao_com_tick();
        let ms: u64 = (1 << 32) + 5;
        xbox_kernel::loader::data::publish_tick_count(&mut s.bus, ms).unwrap();
        assert_eq!(s.bus.ler_u32(ENDERECO).unwrap(), 5);
    }
}
