#[cfg(test)]
mod file_provisionals_verification {
    use xbox_mem::GuestVirt;
    use xbox_recomp::runtime::{Desfecho, Limites, Session};
    mod guest { include!("kernel_file_provisionals.rs"); }

    const LIMITES: Limites = Limites { blocos_por_fatia: 1000, fatias: 100 };

    fn word(s: &Session, at: u32) -> u32 {
        s.bus.ler_u32(GuestVirt(at)).unwrap()
    }

    /// No Rust **gerado**: abre a mídia, consulta atributos pelo nome, define a posição e
    /// confirma que escrever é negado sem alterar a mídia (valores provisórios, P30).
    #[test]
    fn posicao_atributos_e_escrita_negada_pela_abi_do_guest() {
        let mut s = Session::load(include_bytes!("kernel_file_provisionals.xbe")).unwrap();
        let media = s.kernel.executable_media().unwrap().to_vec();
        let desfecho = s.conduzir(LIMITES, guest::run, |_| Ok(false)).unwrap();
        assert_eq!(desfecho, Desfecho::Retornou);
        assert_eq!(word(&s, 0x12500), 0, "NtCreateFile");
        assert_eq!(word(&s, 0x12504), 0, "NtQueryFullAttributesFile");
        assert_eq!(word(&s, 0x12400 + 40), media.len() as u32, "EndOfFile");
        assert_eq!(word(&s, 0x12400 + 48), 1, "somente leitura");
        assert_eq!(word(&s, 0x12508), 0, "NtSetInformationFile");
        assert_eq!(word(&s, 0x1250C), 0xC000_0022, "NtWriteFile: ACCESS_DENIED");
        assert_eq!(word(&s, 0x12510), 0xC000_0008, "handle inválido");
        assert_eq!(s.kernel.executable_media().unwrap(), media.as_slice(), "mídia intacta");
        assert_eq!(s.kernel.recusadas(), 0);
    }
}
