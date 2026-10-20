
#[cfg(test)]
mod muldiv_verification {
    use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
    use xbox_mem::{Bus, GuestVirt};
    use xbox_recomp::{Error, DivideFault, ir::Width, runtime::{Exit, Session, State}};
    /* MODULES */
    type Run = fn(&mut State, &mut Bus, u64) -> xbox_recomp::Result<Exit>;
    struct Core { signed: bool, divide: bool, width: Width, run: Run, image: &'static [u8] }
    const CORE: &[Core] = &[ /* CORE */ ];
    const MEMORY: &[Core] = &[ /* MEMORY */ ];

    /* HARDWARE */

    #[test]
    fn operandos_em_memoria_e_imul_truncado_respeitam_largura_e_flags() {
        for case in MEMORY {
            let mut s = Session::load(case.image).unwrap();
            let narrow = case.width != Width::Dword;
            s.state.regs[0] = if narrow { 0xA55A002A } else { 42 };
            s.state.regs[2] = if narrow { 0x98760000 } else { 0 };
            s.state.eflags = 0xED7;
            s.bus.escrever_u32(GuestVirt(0x12020), 5).unwrap();
            assert_eq!((case.run)(&mut s.state, &mut s.bus, 1).unwrap(), Exit::Returned);
            let (eax, edx) = if case.divide {
                match case.width { Width::Byte => (0xA55A0208, 0x98760000), Width::Word => (0xA55A0008, 0x98760002), Width::Dword => (8, 2) }
            } else if narrow { (0xA55A00D2, 0x98760000) } else { (210, 0) };
            let flags = if !case.divide && case.signed && case.width == Width::Byte { 0xE03 } else { 0x602 };
            assert_eq!((s.state.regs[0], s.state.regs[2], s.state.eflags), (eax, edx, flags));
            assert_eq!(s.bus.ler_u32(GuestVirt(0x12020)).unwrap(), 5);
        }
        let cases: &[(Run, &[u8], u32)] = &[
            (md_imul_mem2::run, include_bytes!("md_imul_mem2.xbe"), 21),
            (md_imul_mem3::run, include_bytes!("md_imul_mem3.xbe"), 3),
        ];
        for &(run, image, expected) in cases {
            let mut s = Session::load(image).unwrap(); s.state.regs[0] = 0xFFFFFFF9; s.state.regs[2] = 0xDEADBEEF; s.state.eflags = 0xED7;
            s.bus.escrever_u32(GuestVirt(0x12020), 0xFFFFFFFD).unwrap();
            assert_eq!(run(&mut s.state, &mut s.bus, 1).unwrap(), Exit::Returned);
            assert_eq!((s.state.regs[0], s.state.regs[2], s.state.eflags), (expected, 0xDEADBEEF, 0x602));
        }
    }

    #[test]
    fn fontes_alias_e_memoria_somente_leitura_sao_lidas_antes_dos_resultados() {
        let cases: &[(Run, &[u8], u32, u32, u32, u32, u32)] = &[
            (md_mul_ah::run, include_bytes!("md_mul_ah.xbe"), 0xA55A0304, 0x12345678, 0xA55A000C, 0x12345678, 0x602),
            (md_imul_edx::run, include_bytes!("md_imul_edx.xbe"), 0xFFFFFFFD, 0xFFFFFFFE, 6, 0, 0x602),
            (md_idiv_ah::run, include_bytes!("md_idiv_ah.xbe"), 0xA55AFFF9, 0x12345678, 0xA55A0007, 0x12345678, 0x602),
            (md_div_eax::run, include_bytes!("md_div_eax.xbe"), 7, 0, 1, 0, 0x602),
            (md_idiv_edx::run, include_bytes!("md_idiv_edx.xbe"), 0xFFFFFFF9, 0xFFFFFFFF, 7, 0, 0x602),
            (md_mul_readonly::run, include_bytes!("md_mul_readonly.xbe"), 0xA55A0002, 0x12345678, 0xA55A01EC, 0x12345678, 0xE03),
            (md_div_readonly::run, include_bytes!("md_div_readonly.xbe"), 0xA55A01EC, 0x12345678, 0xA55A0002, 0x12345678, 0x602),
        ];
        for &(run, image, low, high, expected_low, expected_high, flags) in cases {
            let mut s = Session::load(image).unwrap(); s.state.regs[0] = low; s.state.regs[2] = high; s.state.eflags = 0xED7;
            let code = s.bus.ler_u32(GuestVirt(0x11000)).unwrap();
            assert_eq!(run(&mut s.state, &mut s.bus, 1).unwrap(), Exit::Returned);
            assert_eq!((s.state.regs[0], s.state.regs[2], s.state.eflags), (expected_low, expected_high, flags));
            assert_eq!(s.bus.ler_u32(GuestVirt(0x11000)).unwrap(), code);
        }
    }

    #[test]
    fn cbw_cwde_cwd_cdq_preparam_operandos_com_sinal_sem_alterar_flags() {
        let cases: &[(Run, &[u8], u32, u32, u32)] = &[
            (md_cbw::run, include_bytes!("md_cbw.xbe"), 0xA55A0080, 0xA55AFF80, 0xDEAD1234),
            (md_cbw::run, include_bytes!("md_cbw.xbe"), 0xA55A807F, 0xA55A007F, 0xDEAD1234),
            (md_cwde::run, include_bytes!("md_cwde.xbe"), 0xA55A8001, 0xFFFF8001, 0xDEAD1234),
            (md_cwde::run, include_bytes!("md_cwde.xbe"), 0xA55A7FFF, 0x7FFF, 0xDEAD1234),
            (md_cwd::run, include_bytes!("md_cwd.xbe"), 0xA55A8001, 0xA55A8001, 0xDEADFFFF),
            (md_cwd::run, include_bytes!("md_cwd.xbe"), 0xA55A7FFF, 0xA55A7FFF, 0xDEAD0000),
            (md_cdq::run, include_bytes!("md_cdq.xbe"), 0x80000001, 0x80000001, u32::MAX),
            (md_cdq::run, include_bytes!("md_cdq.xbe"), 0x7FFFFFFF, 0x7FFFFFFF, 0),
        ];
        for &(run, image, low, expected_low, expected_high) in cases {
            let mut s = Session::load(image).unwrap(); s.state.regs[0] = low; s.state.regs[2] = 0xDEAD1234; s.state.eflags = 0xED7;
            assert_eq!(run(&mut s.state, &mut s.bus, 1).unwrap(), Exit::Returned);
            assert_eq!((s.state.regs[0], s.state.regs[2], s.state.eflags), (expected_low, expected_high, 0xED7));
        }
    }

    fn fault(case: &Core, low: u32, high: u32, divisor: u32, reason: DivideFault) {
        let mut s = Session::load(case.image).unwrap(); s.state.regs[0] = low; s.state.regs[1] = divisor; s.state.regs[2] = high; s.state.eflags = 0xED7;
        let before = s.state.clone();
        assert!(matches!((case.run)(&mut s.state, &mut s.bus, 1), Err(Error::Divide { pc: 0x11000, reason: actual }) if actual == reason));
        assert_eq!(s.state, before);
    }

    #[test]
    fn divisao_respeita_sinal_do_resto_limites_e_de_sem_comprometer_cpu() {
        for case in CORE.iter().filter(|c| c.divide) {
            fault(case, 1, 2, 0, DivideFault::ZeroDivisor);
            let (low, high) = if case.signed {
                match case.width { Width::Byte => (0xA55AFF80, 0x12345678), Width::Word => (0xA55A8000, 0x9876FFFF), Width::Dword => (0x80000000, u32::MAX) }
            } else {
                match case.width { Width::Byte => (0xA55A0100, 0x12345678), Width::Word => (0xA55A0000, 0x98760001), Width::Dword => (0, 1) }
            };
            fault(case, low, high, if case.signed { u32::MAX } else { 1 }, DivideFault::QuotientOverflow);
            if case.signed && case.width == Width::Dword { fault(case, 0, 0x80000000, u32::MAX, DivideFault::QuotientOverflow); }
            let mut s = Session::load(case.image).unwrap();
            let (low, high, expected_low, expected_high) = if case.signed {
                match case.width { Width::Byte => (0xA55AFFF9, 0x12345678, 0xA55AFFFE, 0x12345678), Width::Word => (0xA55AFFF9, 0x9876FFFF, 0xA55AFFFE, 0x9876FFFF), Width::Dword => (0xFFFFFFF9, u32::MAX, 0xFFFFFFFE, u32::MAX) }
            } else {
                match case.width { Width::Byte => (0xA55A00FF, 0x12345678, 0xA55A00FF, 0x12345678), Width::Word => (0xA55AFFFF, 0x98760000, 0xA55AFFFF, 0x98760000), Width::Dword => (u32::MAX, 0, u32::MAX, 0) }
            };
            s.state.regs[0] = low; s.state.regs[1] = if case.signed { 3 } else { 1 }; s.state.regs[2] = high; s.state.eflags = 0xED7;
            assert_eq!((case.run)(&mut s.state, &mut s.bus, 1).unwrap(), Exit::Returned);
            assert_eq!((s.state.regs[0], s.state.regs[2], s.state.eflags), (expected_low, expected_high, 0x602));
        }
    }

    #[test]
    fn falhas_de_leitura_mul_imul_div_idiv_preservam_cpu_e_flags() {
        let cases: &[(Run, &[u8])] = &[
            /* FAULTS */
            (md_imul_fault2::run, include_bytes!("md_imul_fault2.xbe")),
            (md_imul_fault3::run, include_bytes!("md_imul_fault3.xbe")),
        ];
        for &(run, image) in cases {
            let mut s = Session::load(image).unwrap(); s.state.regs[0] = 0x80000000; s.state.regs[2] = 0xFFFFFFFF; s.state.eflags = 0xED7;
            let before = s.state.clone();
            assert!(matches!(run(&mut s.state, &mut s.bus, 1), Err(Error::Memory(_)))); assert_eq!(s.state, before);
        }
    }

    #[test]
    fn snapshot_entre_produto_e_divisao_preserva_ambas_as_metades() {
        let cases: &[(Run, &[u8], u32, u32, u32, u32)] = &[
            (md_snapshot::run, include_bytes!("md_snapshot.xbe"), 10, 0, 5, 0x80000000),
            (md_signed_snapshot::run, include_bytes!("md_signed_snapshot.xbe"), 3, 0x80000000, 0xFFFFFFFE, 0x80000000),
        ];
        for &(run, image, factor, product_low, product_high, expected) in cases {
            let mut s = Session::load(image).unwrap(); s.state.regs[0] = 0x80000000; s.state.regs[1] = factor; s.state.regs[3] = factor;
            assert_eq!(run(&mut s.state, &mut s.bus, 1).unwrap(), Exit::Budget);
            assert_eq!((s.state.regs[0], s.state.regs[2]), (product_low, product_high));
            let mut writer = SnapshotWriter::new(); s.state.save(&mut writer); let bytes = writer.finish();
            let mut restored = State::new(0); restored.load(&mut SnapshotReader::new(&bytes).unwrap()).unwrap();
            assert_eq!(run(&mut restored, &mut s.bus, 1).unwrap(), Exit::Returned);
            assert_eq!((restored.regs[0], restored.regs[2], restored.retired), (expected, 0, 4));
        }
    }
}
