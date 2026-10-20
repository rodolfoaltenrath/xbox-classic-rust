
#[cfg(test)]
mod rotate_verification {
    use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
    use xbox_mem::{Bus, GuestVirt};
    use xbox_recomp::ir::{Rotate, Width};
    use xbox_recomp::runtime::{Exit, Session, State};
    /* MODULES */
    type Run = fn(&mut State, &mut Bus, u64) -> xbox_recomp::Result<Exit>;
    struct Case { op: Rotate, width: Width, count: Option<u8>, run: Run, image: &'static [u8] }
    #[cfg(target_arch = "x86_64")]
    const CASES: &[Case] = &[ /* CASES */ ];
    const MEMORY_CASES: &[Case] = &[ /* MEMORY_CASES */ ];

    #[cfg(target_arch = "x86_64")]
    fn check_hardware(case: &Case, s: &mut Session, initial: &State, a: u32, count: u32, old: u32) {
        use xbox_cpu::oracle::{self, execute_shift};
        let op = match case.op { Rotate::Rol => oracle::Shift::Rol, Rotate::Ror => oracle::Shift::Ror, Rotate::Rcl => oracle::Shift::Rcl, Rotate::Rcr => oracle::Shift::Rcr };
        let width = match case.width { Width::Byte => oracle::Width::Byte, Width::Word => oracle::Width::Word, Width::Dword => oracle::Width::Dword };
        let (value, flags) = execute_shift(op, width, a, count, old);
        s.state = initial.clone();
        s.state.regs[0] = a;
        s.state.regs[1] = 0xA55A0000 | count;
        s.state.eflags = old;
        assert_eq!((case.run)(&mut s.state, &mut s.bus, 1).unwrap(), Exit::Returned);
        assert_eq!(s.state.regs[0], value, "{:?}/{:?} a={a:08X} count={count} old={old:08X}", case.op, case.width);
        assert_eq!(s.state.regs[1], 0xA55A0000 | count);
        let n = count & 31;
        let mask = if n <= 1 { 0x8D5 } else { 0xD5 };
        assert_eq!(s.state.eflags & mask, flags & mask, "{:?}/{:?} a={a:08X} count={count} old={old:08X}", case.op, case.width);
        assert_eq!(s.state.eflags & !0x801, old & !0x801);
        if n > 1 { assert_eq!(s.state.eflags & 0x800, 0); }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn rotacoes_cl_imediatas_e_implicitas_concordam_em_70488_casos() {
        const VALUES: [u32; 11] = [0, 1, 0x7F, 0x80, 0xFF, 0x7FFF, 0x8000, 0xFFFF, 0x7FFFFFFF, 0x80000000, u32::MAX];
        let mut checked = 0;
        for case in CASES {
            let mut s = Session::load(case.image).unwrap();
            let initial = s.state.clone();
            for count in 0..=255 {
                if case.count.is_some_and(|c| u32::from(c) != count) { continue; }
                for a in VALUES { for old in [0x602, 0xED7] {
                    check_hardware(case, &mut s, &initial, a, count, old);
                    checked += 1;
                }}
            }
        }
        assert_eq!(checked, 70488);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn rotacoes_de_byte_esgotam_valores_contagens_e_carry_em_524288_casos() {
        let mut checked = 0;
        for case in CASES.iter().filter(|c| c.width == Width::Byte && c.count.is_none()) {
            let mut s = Session::load(case.image).unwrap();
            let initial = s.state.clone();
            for a in 0..=255 { for count in 0..=255 { for old in [0x602, 0xED7] {
                check_hardware(case, &mut s, &initial, 0xA55A0000 | a, count, old);
                checked += 1;
            }}}
        }
        assert_eq!(checked, 524288);
    }

    #[test]
    fn rotacoes_de_ah_cl_cx_ecx_capturam_contagem_e_carry_antes_da_escrita() {
        let cases: &[(Run, &[u8], u32, u32, u32, u32)] = &[
            (rotate_ah::run, include_bytes!("rotate_ah.xbe"), 0x12348056, 1, 0x12340156, 1),
            (rotate_cl::run, include_bytes!("rotate_cl.xbe"), 0, 0xA55A0081, 0, 0xA55A00C0),
            (rotate_cx::run, include_bytes!("rotate_cx.xbe"), 0, 0xA55A8001, 0, 0xA55AC000),
            (rotate_ecx::run, include_bytes!("rotate_ecx.xbe"), 0, 0x80000001, 0, 3),
        ];
        for &(run, image, eax, ecx, expected_eax, expected_ecx) in cases {
            let mut s = Session::load(image).unwrap();
            s.state.regs[0] = eax;
            s.state.regs[1] = ecx;
            s.state.eflags = 0xED7;
            assert_eq!(run(&mut s.state, &mut s.bus, 1).unwrap(), Exit::Returned);
            assert_eq!(s.state.regs[0], expected_eax);
            assert_eq!(s.state.regs[1], expected_ecx);
            assert_eq!(s.state.eflags & !0x801, 0x6D6);
        }
    }

    #[test]
    fn rotacoes_em_memoria_preservam_flags_e_tratam_voltas_completas() {
        for case in MEMORY_CASES {
            let mut s = Session::load(case.image).unwrap();
            let initial = s.state.clone();
            let (bits, sign, outside) = match case.width { Width::Byte => (8, 0x80, 0xA55A3300), Width::Word => (16, 0x8000, 0xA55A0000), Width::Dword => (32, 0x80000000, 0) };
            let carry_op = matches!(case.op, Rotate::Rcl | Rotate::Rcr);
            let cycle = bits + u32::from(carry_op);
            for count in [case.count.map_or(0, u32::from), 1, 32, cycle] {
                // Um anel de 33 bits não cabe na contagem efetiva de cinco bits.
                if cycle == 33 && count == cycle { continue; }
                s.state = initial.clone();
                s.state.regs[1] = count;
                let old = if count == cycle && !carry_op { 0xED6 } else { 0xED7 };
                s.state.eflags = old;
                s.bus.escrever_u32(GuestVirt(0x12020), outside | sign | 1).unwrap();
                s.bus.escrever_u32(GuestVirt(0x12024), 0xDEADBEEF).unwrap();
                assert_eq!((case.run)(&mut s.state, &mut s.bus, 1).unwrap(), Exit::Returned);
                let left = matches!(case.op, Rotate::Rol | Rotate::Rcl);
                let (value, flags) = if count & 31 == 0 { (sign | 1, old) }
                    else if count == 1 { (if left { 3 } else { sign | sign >> 1 }, if left { 0xED7 } else { 0x6D7 }) }
                    else { (sign | 1, 0x6D7) };
                assert_eq!(s.bus.ler_u32(GuestVirt(0x12020)).unwrap(), outside | value);
                assert_eq!(s.bus.ler_u32(GuestVirt(0x12024)).unwrap(), 0xDEADBEEF);
                assert_eq!(s.state.eflags, flags);
            }
        }
    }

    #[test]
    fn rotacoes_com_zero_tambem_validam_leitura_e_escrita_sem_comprometer_cpu() {
        let cases: &[(Run, &[u8])] = &[ /* FAULT_CASES */ ];
        for &(run, image) in cases {
            let mut s = Session::load(image).unwrap();
            for count in [0, 1, 9, 17, 32] {
                s.state.regs[1] = count;
                s.state.eflags = 0xED7;
                let before = s.state.clone();
                let code = s.bus.ler_u32(GuestVirt(0x11000)).unwrap();
                assert!(run(&mut s.state, &mut s.bus, 1).is_err());
                assert_eq!(s.state, before);
                assert_eq!(s.bus.ler_u32(GuestVirt(0x11000)).unwrap(), code);
            }
        }
    }

    #[test]
    fn snapshot_preserva_carry_entre_rcl_rcr_e_shift_duplo() {
        let mut s = Session::load(include_bytes!("rotate_snapshot.xbe")).unwrap();
        s.state.regs[0] = 0x80000000;
        s.state.eflags = 0x603;
        assert_eq!(rotate_snapshot::run(&mut s.state, &mut s.bus, 1).unwrap(), Exit::Budget);
        assert_eq!(s.state.regs[0], 1);
        assert_eq!(s.state.eflags & 1, 1);
        let mut writer = SnapshotWriter::new(); s.state.save(&mut writer);
        let bytes = writer.finish(); let mut restored = State::new(0);
        restored.load(&mut SnapshotReader::new(&bytes).unwrap()).unwrap();
        assert_eq!(rotate_snapshot::run(&mut restored, &mut s.bus, 1).unwrap(), Exit::Returned);
        assert_eq!(restored.regs[0], 3);
        assert_eq!(restored.regs[2], 0x80000000);
        assert_eq!(restored.retired, 5);
    }
}
