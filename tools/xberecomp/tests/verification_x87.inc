#[cfg(test)]
mod x87_verification {
    use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
    use xbox_mem::GuestVirt;
    use xbox_recomp::Error;
    use xbox_recomp::runtime::x87::{Extended, StackFault};
    use xbox_recomp::runtime::{Exit, Session};
    mod ok { include!("x87_ok.rs"); }
    mod vazia { include!("x87_vazia.rs"); }
    mod cheia { include!("x87_cheia.rs"); }
    mod deixa { include!("x87_deixa.rs"); }

    fn rodar() -> Session {
        let mut s = Session::load(include_bytes!("x87_ok.xbe")).unwrap();
        assert_eq!(ok::run(&mut s.state, &mut s.bus, 10_000).unwrap(), Exit::Returned);
        s
    }

    fn u32_at(s: &Session, at: u32) -> u32 {
        s.bus.ler_u32(GuestVirt(at)).unwrap()
    }

    fn u64_at(s: &Session, at: u32) -> u64 {
        s.bus.ler_u64(GuestVirt(at)).unwrap()
    }

    #[test]
    fn inteiro_grande_arredonda_uma_vez_e_o_resto_e_exato() {
        let s = rodar();
        // 2^53 + 1 empata entre 2^53 e 2^53 + 2: vai para o par, 2^53.
        assert_eq!(u64_at(&s, 0x12700), ((1u64 << 53) as f64).to_bits());
        assert_eq!(f32::from_bits(u32_at(&s, 0x12710)), -5.0);
        assert_eq!(f32::from_bits(u32_at(&s, 0x12718)), -2.0, "fild de 16 bits");
    }

    #[test]
    fn constantes_e_formato_de_80_bits() {
        let s = rodar();
        assert_eq!(u64_at(&s, 0x12720), 0, "fldz sobrescreve os 1s");
        // 1.0 em 80 bits: mantissa com o bit inteiro, expoente 0x3FFF.
        assert_eq!(u64_at(&s, 0x12730), 0x8000_0000_0000_0000);
        assert_eq!(s.bus.ler_u16(GuestVirt(0x12738)).unwrap(), 0x3FFF);
    }

    #[test]
    fn fst_nao_retira_e_fstp_retira() {
        let s = rodar();
        assert_eq!(f64::from_bits(u64_at(&s, 0x12740)), 1.5);
        assert_eq!(f32::from_bits(u32_at(&s, 0x12748)), 1.5);
        assert_eq!(s.state.fpu.depth(), 0, "pilha vazia ao final");
    }

    #[test]
    fn ida_e_volta_por_80_bits_nao_perde_nada() {
        let s = rodar();
        assert_eq!(u64_at(&s, 0x12760), 0.1f64.to_bits());
    }

    #[test]
    fn pilha_vazia_recusa_o_store_sem_escrever() {
        let mut s = Session::load(include_bytes!("x87_vazia.xbe")).unwrap();
        let erro = vazia::run(&mut s.state, &mut s.bus, 100).unwrap_err();
        assert!(matches!(erro, Error::FpuStack(StackFault::Underflow)), "{erro}");
        assert_eq!(u32_at(&s, 0x12700), 0xAAAA_AAAA, "o store não pode ter escrito");
    }

    #[test]
    fn nono_push_estoura_a_pilha_sem_empilhar() {
        let mut s = Session::load(include_bytes!("x87_cheia.xbe")).unwrap();
        let erro = cheia::run(&mut s.state, &mut s.bus, 100).unwrap_err();
        assert!(matches!(erro, Error::FpuStack(StackFault::Overflow)), "{erro}");
        assert_eq!(s.state.fpu.depth(), 8);
    }

    #[test]
    fn snapshot_preserva_a_pilha() {
        let mut s = Session::load(include_bytes!("x87_deixa.xbe")).unwrap();
        assert_eq!(deixa::run(&mut s.state, &mut s.bus, 100).unwrap(), Exit::Returned);
        assert_eq!(s.state.fpu.depth(), 2);
        assert_eq!(s.state.fpu.peek().unwrap(), Extended::ZERO);
        let mut out = SnapshotWriter::new();
        s.save(&mut out);
        let bytes = out.finish();
        let mut restaurada = Session::load(include_bytes!("x87_deixa.xbe")).unwrap();
        restaurada.load(&mut SnapshotReader::new(&bytes).unwrap()).unwrap();
        assert_eq!(restaurada.state.fpu, s.state.fpu);
    }
}
