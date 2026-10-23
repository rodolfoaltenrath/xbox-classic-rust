#[cfg(test)]
mod options_verification {
    fn parse(args:&[&str])->Result<super::Opcoes,String> { super::ler_opcoes(args.iter().map(|s|(*s).to_owned())) }
    #[test]
    fn flags_em_qualquer_ordem_preservam_orcamento_e_caminhos_com_espacos() {
        for args in [vec!["--janela","--fatias","320","--quadro","dir with spaces/frame.bgra"],
                     vec!["--quadro","dir with spaces/frame.bgra","--fatias","320","--janela"]] {
            let result=parse(&args).unwrap();assert_eq!(result.fatias,320);assert!(result.janela);
            assert_eq!(result.quadro.unwrap(),std::path::PathBuf::from("dir with spaces/frame.bgra"));
        }
    }
    #[test]
    fn configuracao_ausente_mantem_o_padrao_sem_janela_ou_exportacao() {
        let result=parse(&[]).unwrap();assert_eq!(result.fatias,super::FATIAS_PADRAO);
        assert!(!result.janela);assert_eq!(result.quadro,None);
    }
    #[test]
    fn argumentos_desconhecidos_repetidos_ou_incompletos_recusam() {
        for args in [vec!["--unknown"],vec!["--janela","--janela"],vec!["--fatias"],
                     vec!["--fatias","1","--fatias","2"],vec!["--quadro"],vec!["--quadro","--janela"],
                     vec!["--quadro","a","--quadro","b"],vec!["--fatias","1","--unknown"]] {
            assert!(parse(&args).is_err(),"{args:?}");
        }
    }
    #[test]
    fn limites_nulos_negativos_e_transbordados_recusam_antes_de_executar() {
        for limit in ["0","-1","4294967296","abc"] {assert!(parse(&["--fatias",limit]).is_err());}
        assert_eq!(parse(&["--fatias","4294967295"]).unwrap().fatias,u32::MAX);
    }
    #[test]
    fn estados_validam_caminhos_e_repeticoes_antes_de_executar() {
        let result=parse(&["--salvar","novo estado.obsstate","--carregar","estado anterior.obsstate"]).unwrap();
        assert_eq!(result.salvar.unwrap(),std::path::PathBuf::from("novo estado.obsstate"));
        assert_eq!(result.carregar.unwrap(),std::path::PathBuf::from("estado anterior.obsstate"));
        for args in [vec!["--salvar"],vec!["--carregar","--janela"],vec!["--salvar",""],
            vec!["--salvar","a","--salvar","b"],vec!["--carregar","a","--carregar","b"]] {
            assert!(parse(&args).is_err(),"{args:?}");
        }
    }
    #[test]
    fn continuo_exige_janela_e_estados_exigem_continuo() {
        assert!(parse(&["--continuo"]).is_err());
        assert!(parse(&["--estados","saves"]).is_err());
        assert!(parse(&["--janela","--continuo","--continuo"]).is_err());
        assert!(parse(&["--janela","--continuo","--estados","a","--estados","b"]).is_err());
        let options=parse(&["--continuo","--estados","meus estados","--janela"]).unwrap();
        assert!(options.continuo); assert_eq!(options.estados.unwrap(),std::path::PathBuf::from("meus estados"));
    }
}
