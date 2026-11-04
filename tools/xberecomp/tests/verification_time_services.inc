#[cfg(test)]
mod time_services_verification {
    use xbox_mem::GuestVirt;
    use xbox_recomp::runtime::{Desfecho, Limites, Session};
    mod guest { include!("kernel_time_services.rs"); }

    const LIMITES: Limites = Limites { blocos_por_fatia: 1000, fatias: 100 };

    fn word(s: &Session, at: u32) -> u32 {
        s.bus.ler_u32(GuestVirt(at)).unwrap()
    }

    /// No Rust **gerado**: a palavra alta dos retornos de 64 bits chega ao EDX, e a conversão
    /// de calendário faz a ida e volta pela ABI de `TIME_FIELDS`.
    #[test]
    fn contador_frequencia_e_calendario_pela_abi_do_guest() {
        let mut s = Session::load(include_bytes!("kernel_time_services.xbe")).unwrap();
        let desfecho = s.conduzir(LIMITES, guest::run, |_| Ok(false)).unwrap();
        assert_eq!(desfecho, Desfecho::Retornou);
        assert_eq!(word(&s, 0x12500), 3_375_000, "KeQueryPerformanceFrequency, palavra baixa");
        assert_eq!(word(&s, 0x12504), 0, "palavra alta em EDX");
        assert_eq!(word(&s, 0x1250C), 0, "o contador ainda cabe em 32 bits");
        assert_eq!(word(&s, 0x12510) & 0xFF, 1, "RtlTimeFieldsToTime devolve TRUE");
        let ticks = u64::from(word(&s, 0x12620)) | (u64::from(word(&s, 0x12624)) << 32);
        assert_eq!(ticks, 125_963_876_967_890_000);
        assert_eq!(word(&s, 0x12640), 0x0003_07D0);
        assert_eq!(word(&s, 0x12644), 0x000C_0001);
        assert_eq!(word(&s, 0x12648), 0x0038_0022);
        assert_eq!(word(&s, 0x1264C), 0x0003_0315, "dia da semana 3 = quarta-feira");
        assert_eq!(s.kernel.recusadas(), 0);
    }
}
