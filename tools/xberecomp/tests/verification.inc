
#[cfg(test)] mod overflow;
#[cfg(test)] mod signed;
#[cfg(test)] mod sib;
#[cfg(test)] mod branch;
#[cfg(test)] mod fault;
#[cfg(test)] mod write_code;
#[cfg(test)] mod forever;
#[cfg(test)] mod ret_cleanup;

#[cfg(test)]
mod verification {
    use super::*;
    use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
    use xbox_mem::GuestVirt;
    use xbox_recomp::runtime::{Exit, Session};

    fn save(session: &Session) -> Vec<u8> {
        let mut w = SnapshotWriter::new();
        session.state.save(&mut w);
        session.bus.save(&mut w);
        w.finish()
    }

    #[test]
    fn demo_executa_calls_e_escreve_resultado_correto() {
        let mut s = Session::load(include_bytes!("image.xbe")).unwrap();
        let initial_esp = s.state.regs[4];
        assert_eq!(generated::run(&mut s.state, &mut s.bus, 100).unwrap(), Exit::Returned);
        assert_eq!(s.state.regs[0], 62);
        assert_eq!(s.state.regs[1], 0);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12020)).unwrap(), 62);
        assert_eq!(s.state.regs[4], initial_esp + 4);
        assert_eq!(s.state.retired, 37);
        let state = s.state.clone();
        assert_eq!(generated::run(&mut s.state, &mut s.bus, 100).unwrap(), Exit::Returned);
        assert_eq!(s.state, state);
    }

    #[test]
    fn snapshot_restaura_cpu_memoria_e_call_pendente() {
        let mut s = Session::load(include_bytes!("image.xbe")).unwrap();
        // Setup + 10 iterações + CALL: o retorno permanece na pilha do guest.
        assert_eq!(generated::run(&mut s.state, &mut s.bus, 12).unwrap(), Exit::Budget);
        assert_eq!(s.state.eip, 0x11019);
        let checkpoint = save(&s);
        generated::run(&mut s.state, &mut s.bus, 100).unwrap();
        let expected = save(&s);
        let mut resumed = Session::load(include_bytes!("image.xbe")).unwrap();
        let mut r = SnapshotReader::new(&checkpoint).unwrap();
        resumed.state.load(&mut r).unwrap();
        resumed.bus.load(&mut r).unwrap();
        while generated::run(&mut resumed.state, &mut resumed.bus, 1).unwrap() == Exit::Budget {}
        assert_eq!(save(&resumed), expected);
    }

    #[test]
    fn arquivo_de_estado_retoma_call_real_e_replay_integral() {
        let image = include_bytes!("image.xbe");
        let mut s = Session::load(image).unwrap();
        assert_eq!(generated::run(&mut s.state, &mut s.bus, 12).unwrap(), Exit::Budget);
        let checkpoint = xbox_recomp::checkpoint::encode(&s, image).unwrap();
        generated::run(&mut s.state, &mut s.bus, 100).unwrap();
        let expected = xbox_recomp::checkpoint::encode(&s, image).unwrap();
        let mut resumed = xbox_recomp::checkpoint::decode(&checkpoint, image).unwrap();
        assert_eq!(generated::run(&mut resumed.state, &mut resumed.bus, 100).unwrap(), Exit::Returned);
        assert_eq!(xbox_recomp::checkpoint::encode(&resumed, image).unwrap(), expected);
    }

    #[test]
    fn controle_continuo_pausa_codigo_nativo_e_retoma_sem_perder_clock() {
        let image=include_bytes!("image.xbe"); let mut s=Session::load(image).unwrap();
        let limits=Limites {blocos_por_fatia:1,fatias:1};
        let mut control=xbox_recomp::control::Control::default();
        assert_eq!(control.advance(&mut s,limits,generated::run, |_|Ok(false)).unwrap(),None);
        let checkpoint=xbox_recomp::checkpoint::encode(&s,image).unwrap();
        control.toggle_pause();
        for _ in 0..3 {assert_eq!(control.advance(&mut s,limits,generated::run, |_|panic!("pausado")).unwrap(),None);}
        assert_eq!(xbox_recomp::checkpoint::encode(&s,image).unwrap(),checkpoint);
        control.toggle_pause();
        while control.advance(&mut s,limits,generated::run, |_|Ok(false)).unwrap().is_none() {}
        assert_eq!(s.state.regs[0],62); assert_eq!(s.state.retired,37);
    }

    #[test]
    fn flags_overflow_imediato_negativo_e_branch_tomado() {
        let mut s = Session::load(include_bytes!("overflow.xbe")).unwrap();
        overflow::run(&mut s.state, &mut s.bus, 100).unwrap();
        assert_eq!(s.state.regs[0], 0);
        assert_eq!(s.state.eflags & 0x8D5, 0x55);
        let mut s = Session::load(include_bytes!("signed.xbe")).unwrap();
        signed::run(&mut s.state, &mut s.bus, 100).unwrap();
        assert_eq!(s.state.regs[0], u32::MAX);
        let mut s = Session::load(include_bytes!("branch.xbe")).unwrap();
        branch::run(&mut s.state, &mut s.bus, 100).unwrap();
        assert_eq!(s.state.regs[0], 7);
        assert_eq!(s.state.retired, 5);
    }

    #[test]
    fn enderecamento_sib_e_ret_com_limpeza() {
        let mut s = Session::load(include_bytes!("sib.xbe")).unwrap();
        sib::run(&mut s.state, &mut s.bus, 100).unwrap();
        assert_eq!(s.state.regs[2], 0x1234);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12020)).unwrap(), 0x1234);
        let mut s = Session::load(include_bytes!("ret_cleanup.xbe")).unwrap();
        let esp = s.state.regs[4];
        ret_cleanup::run(&mut s.state, &mut s.bus, 100).unwrap();
        assert_eq!(s.state.regs[4], esp + 12);
        assert!(s.state.returned);
    }

    #[test]
    fn falhas_de_memoria_sao_erros_e_nao_abortam_o_processo() {
        let mut s = Session::load(include_bytes!("fault.xbe")).unwrap();
        assert!(fault::run(&mut s.state, &mut s.bus, 100).is_err());
        assert_eq!(s.state.eip, 0x11000);
        assert_eq!(s.state.retired, 0);
        let mut s = Session::load(include_bytes!("write_code.xbe")).unwrap();
        assert!(write_code::run(&mut s.state, &mut s.bus, 100).is_err());
        assert_eq!(s.state.eip, 0x11005);
        assert_eq!(s.state.retired, 1);
    }

    #[test]
    fn orcamento_interrompe_loop_e_zero_nao_muda_estado() {
        let mut s = Session::load(include_bytes!("forever.xbe")).unwrap();
        let before = s.state.clone();
        assert_eq!(forever::run(&mut s.state, &mut s.bus, 0).unwrap(), Exit::Budget);
        assert_eq!(s.state, before);
        assert_eq!(forever::run(&mut s.state, &mut s.bus, 3).unwrap(), Exit::Budget);
        assert_eq!(s.state.retired, 3);
        assert_eq!(s.state.eip, 0x11000);
        s.state.eip = 0x12345678;
        assert!(matches!(forever::run(&mut s.state, &mut s.bus, 1), Err(xbox_recomp::Error::UnknownBlock(0x12345678))));
    }
}
