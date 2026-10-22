#[cfg(test)]
mod bit_test_verification {
    use xbox_mem::GuestVirt;
    use xbox_recomp::runtime::{Exit, Session};
    mod programa { include!("bit_test.rs"); }

    fn rodar() -> Session {
        let mut s = Session::load(include_bytes!("bit_test.xbe")).unwrap();
        assert_eq!(programa::run(&mut s.state, &mut s.bus, 10_000).unwrap(), Exit::Returned);
        s
    }

    fn u(s: &Session, at: u32) -> u32 {
        s.bus.ler_u32(GuestVirt(at)).unwrap()
    }

    fn cf(s: &Session, at: u32) -> u8 {
        s.bus.ler_u8(GuestVirt(at)).unwrap()
    }

    #[test]
    fn registrador_copia_o_bit_antigo_para_cf_e_aplica_a_operacao() {
        let s = rodar();
        assert_eq!(u(&s, 0x12500), 0x20, "bts eax,5");
        assert_eq!(cf(&s, 0x12504), 0, "bts: CF é o bit antigo (0)");
        assert_eq!(cf(&s, 0x12508), 1, "bt eax,5");
        assert_eq!(cf(&s, 0x1250C), 1, "btr: CF é o bit antigo (1)");
        assert_eq!(u(&s, 0x12510), 0, "btr eax,5");
        assert_eq!(cf(&s, 0x12514), 0, "btc: CF é o bit antigo (0)");
        assert_eq!(u(&s, 0x12518), 0x8000_0000, "btc eax,31");
        assert_eq!(cf(&s, 0x1251C), 1, "imediato usa só 5 bits: 63 -> 31");
    }

    #[test]
    fn memoria_com_registrador_anda_pelo_deslocamento_com_sinal() {
        let s = rodar();
        assert_eq!(u(&s, 0x12408), 0x40, "bit 70 está na palavra +2, bit 6");
        assert_eq!(cf(&s, 0x12520), 0);
        assert_eq!(
            u(&s, 0x12400),
            0x8000_0000,
            "deslocamento -1 em [0x12404] cai no bit 31 da palavra anterior"
        );
        assert_eq!(u(&s, 0x12404), 0, "a palavra nomeada não foi tocada");
    }

    #[test]
    fn memoria_com_imediato_nao_anda_o_endereco() {
        let s = rodar();
        assert_eq!(u(&s, 0x12410), 2, "bts [mem],33 liga o bit 1 da própria palavra");
        assert_eq!(u(&s, 0x12414), 0);
    }

    #[test]
    fn bt_preserva_as_outras_flags() {
        let s = rodar();
        assert_eq!(cf(&s, 0x12524), 1, "ZF ligado por XOR sobrevive ao BT");
    }
}
