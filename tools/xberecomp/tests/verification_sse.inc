#[cfg(test)]
mod sse_verification {
    use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
    use xbox_mem::GuestVirt;
    use xbox_recomp::Error;
    use xbox_recomp::runtime::{Exit, Session};
    mod ok { include!("sse_ok.rs"); }
    mod falta { include!("sse_falta.rs"); }

    fn rodar() -> Session {
        let mut s = Session::load(include_bytes!("sse_ok.xbe")).unwrap();
        assert_eq!(ok::run(&mut s.state, &mut s.bus, 10_000).unwrap(), Exit::Returned);
        s
    }

    fn f(s: &Session, at: u32) -> f32 {
        f32::from_bits(s.bus.ler_u32(GuestVirt(at)).unwrap())
    }

    fn u(s: &Session, at: u32) -> u32 {
        s.bus.ler_u32(GuestVirt(at)).unwrap()
    }

    fn palavras(s: &Session, at: u32) -> [u32; 4] {
        [0, 4, 8, 12].map(|i| u(s, at + i))
    }

    #[test]
    fn aritmetica_escalar_de_precisao_simples() {
        let s = rodar();
        assert_eq!(f(&s, 0x12300), 10.0, "addss");
        assert_eq!(f(&s, 0x12304), 2.0, "subss");
        assert_eq!(f(&s, 0x12308), 24.0, "mulss");
        assert_eq!(f(&s, 0x1230C), 1.5, "divss");
        assert_eq!(f(&s, 0x12310), 2.0, "sqrtss");
        assert_eq!(f(&s, 0x12314), 4.0, "minss");
        assert_eq!(f(&s, 0x12318), 6.0, "maxss");
        assert_eq!(f(&s, 0x12340), 24.0, "mulss com origem em memória");
    }

    #[test]
    fn conversoes_inteiro_float() {
        let s = rodar();
        assert_eq!(f(&s, 0x1231C), 7.0, "cvtsi2ss");
        assert_eq!(u(&s, 0x12320).cast_signed(), -7, "cvttss2si trunca -7,9");
        assert_eq!(u(&s, 0x12324).cast_signed(), -8, "cvtss2si arredonda -7,9");
        assert_eq!(u(&s, 0x12328), 0x8000_0000, "infinito vira indefinido, não satura");
    }

    #[test]
    fn comparacao_ucomiss_codifica_a_relacao_em_flags() {
        let s = rodar();
        let b = |at| s.bus.ler_u8(GuestVirt(at)).unwrap();
        assert_eq!(b(0x1232C), 1, "6 > 4: seta");
        assert_eq!(b(0x12330), 0, "6 > 4: setb");
        assert_eq!(b(0x12334), 1, "4 < 6: setb");
        assert_eq!(b(0x12338), 1, "6 == 6: sete");
        assert_eq!(b(0x1233C), 1, "NaN: setp");
    }

    #[test]
    fn movimentos_de_128_bits_e_a_regra_de_lanes_do_movss() {
        let s = rodar();
        let base = [0x1111_1111, 0x2222_2222, 0x3333_3333, 0x4444_4444];
        assert_eq!(palavras(&s, 0x12350), base, "movaps copia 128 bits");
        assert_eq!(palavras(&s, 0x12360), [0; 4], "xorps xmm,xmm zera");
        assert_eq!(
            palavras(&s, 0x12370),
            [4.0f32.to_bits(), 0x2222_2222, 0x3333_3333, 0x4444_4444],
            "movss reg,reg troca só a palavra baixa"
        );
        assert_eq!(
            palavras(&s, 0x12380),
            [6.0f32.to_bits(), 0, 0, 0],
            "movss mem,carga zera as palavras altas"
        );
        assert_eq!(f(&s, 0x12390), -6.0, "xorps com máscara de sinal nega");
        assert_eq!(
            palavras(&s, 0x123A0),
            [0x2222_2222, 0x3333_3333, 0x4444_4444, 0x5555_5555],
            "movups lê endereço desalinhado"
        );
    }

    #[test]
    fn movaps_desalinhado_e_falta_sem_efeito_no_registrador() {
        let mut s = Session::load(include_bytes!("sse_falta.xbe")).unwrap();
        s.state.xmm[0] = [9, 9, 9, 9];
        let erro = falta::run(&mut s.state, &mut s.bus, 100).unwrap_err();
        assert!(
            matches!(erro, Error::MisalignedSse { address: 0x12244, .. }),
            "{erro}"
        );
        assert_eq!(s.state.xmm[0], [9, 9, 9, 9], "a falta não pode alterar XMM0");
    }

    #[test]
    fn snapshot_preserva_xmm_e_mxcsr() {
        let s = rodar();
        let mut out = SnapshotWriter::new();
        s.save(&mut out);
        let bytes = out.finish();
        let mut restaurada = Session::load(include_bytes!("sse_ok.xbe")).unwrap();
        restaurada.load(&mut SnapshotReader::new(&bytes).unwrap()).unwrap();
        assert_eq!(restaurada.state.xmm, s.state.xmm);
        assert_eq!(restaurada.state.mxcsr, s.state.mxcsr);
        assert_ne!(s.state.xmm, [[0; 4]; 8], "o programa deveria ter escrito em XMM");
    }
}
