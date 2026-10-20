
#[cfg(test)]
mod shift_verification {
    use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
    use xbox_mem::{Bus, GuestVirt};
    use xbox_recomp::ir::{Shift, Width};
    use xbox_recomp::runtime::{Exit, Session, State};

    /* MODULES */

    type Run = fn(&mut State, &mut Bus, u64) -> xbox_recomp::Result<Exit>;

    struct Case {
        op: Shift,
        width: Width,
        count: Option<u8>,
        run: Run,
        image: &'static [u8],
    }

    // A matriz física só existe em x86_64; memória é verificada em todo host.
    #[cfg(target_arch = "x86_64")]
    const CASES: &[Case] = &[
        /* CASES */
    ];
    const MEMORY_CASES: &[Case] = &[
        /* MEMORY_CASES */
    ];

    #[cfg(target_arch = "x86_64")]
    fn check_hardware(case: &Case, s: &mut Session, initial: &State, a: u32, count: u32, old: u32) {
        use xbox_cpu::oracle::{self, execute_shift};
        let op = match case.op {
            Shift::Shl => oracle::Shift::Shl,
            Shift::Shr => oracle::Shift::Shr,
            Shift::Sar => oracle::Shift::Sar,
        };
        let (width, bits) = match case.width {
            Width::Byte => (oracle::Width::Byte, 8),
            Width::Word => (oracle::Width::Word, 16),
            Width::Dword => (oracle::Width::Dword, 32),
        };
        let (expected, flags) = execute_shift(op, width, a, count, old);
        s.state = initial.clone();
        s.state.regs[0] = a;
        s.state.regs[1] = 0xA55A0000 | count;
        s.state.eflags = old;
        assert_eq!((case.run)(&mut s.state, &mut s.bus, 1).unwrap(), Exit::Returned);
        assert_eq!(s.state.regs[0], expected, "{:?}/{:?} a={a:08X} count={count}", case.op, case.width);
        assert_eq!(s.state.regs[1], 0xA55A0000 | count);

        // Máscara independente: só flags que o SDM define para esta contagem.
        let n = count & 31;
        let mask = if n == 0 {
            0x8D5
        } else {
            0xC4 | u32::from(case.op == Shift::Sar || n < bits) | if n == 1 { 0x800 } else { 0 }
        };
        assert_eq!(s.state.eflags & mask, flags & mask,
            "{:?}/{:?} a={a:08X} count={count} old={old:08X}", case.op, case.width);
        assert_eq!(s.state.eflags & !0x8D5, old & !0x8D5);
        assert_eq!(s.state.eflags & (0x8D5 & !mask), 0, "flags indefinidas são zero");
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn shifts_cl_imediatos_e_implicitos_concordam_com_cpu_em_52470_casos() {
        const VALUES: [u32; 11] = [0, 1, 0x7F, 0x80, 0xFF, 0x7FFF, 0x8000, 0xFFFF, 0x7FFFFFFF, 0x80000000, u32::MAX];
        let mut checked = 0;
        for case in CASES {
            let mut s = Session::load(case.image).unwrap();
            let initial = s.state.clone();
            for count in 0..=255 {
                if case.count.is_some_and(|c| u32::from(c) != count) { continue; }
                for a in VALUES {
                    for old in [0x602, 0xED7] {
                        check_hardware(case, &mut s, &initial, a, count, old);
                        checked += 1;
                    }
                }
            }
        }
        assert_eq!(checked, 52470);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn shifts_de_byte_esgotam_valores_e_contagens_em_393216_casos() {
        let mut checked = 0;
        for case in CASES.iter().filter(|c| c.width == Width::Byte && c.count.is_none()) {
            let mut s = Session::load(case.image).unwrap();
            let initial = s.state.clone();
            for a in 0..=255 {
                for count in 0..=255 {
                    for old in [0x602, 0xED7] {
                        check_hardware(case, &mut s, &initial, 0xA55A0000 | a, count, old);
                        checked += 1;
                    }
                }
            }
        }
        assert_eq!(checked, 393216);
    }

    #[test]
    fn shifts_em_ah_cl_cx_ecx_capturam_cl_antes_da_escrita() {
        let cases: &[(Run, &[u8], u32, u32, u32, u32)] = &[
            (shift_ah::run, include_bytes!("shift_ah.xbe"), 0x12348156, 1, 0x12340256, 1),
            (shift_cl::run, include_bytes!("shift_cl.xbe"), 0, 0xA55A0081, 0, 0xA55A0040),
            (shift_cx::run, include_bytes!("shift_cx.xbe"), 0, 0xA55A8001, 0, 0xA55AC000),
            (shift_ecx::run, include_bytes!("shift_ecx.xbe"), 0, 0x40000001, 0, 0x80000002),
        ];
        for &(run, image, eax, ecx, expected_eax, expected_ecx) in cases {
            let mut s = Session::load(image).unwrap();
            s.state.regs[0] = eax;
            s.state.regs[1] = ecx;
            assert_eq!(run(&mut s.state, &mut s.bus, 1).unwrap(), Exit::Returned);
            assert_eq!(s.state.regs[0], expected_eax);
            assert_eq!(s.state.regs[1], expected_ecx);
        }
    }

    #[test]
    fn shifts_em_memoria_respeitam_largura_sinal_e_contagem_zero() {
        for case in MEMORY_CASES {
            let mut s = Session::load(case.image).unwrap();
            let initial = s.state.clone();
            let (sign, outside) = match case.width {
                Width::Byte => (0x80, 0xA55A3300),
                Width::Word => (0x8000, 0xA55A0000),
                Width::Dword => (0x80000000, 0),
            };
            for count in [case.count.map_or(0, u32::from), 1, 32] {
                s.state = initial.clone();
                s.state.regs[1] = count;
                s.state.eflags = 0xED7;
                s.bus.escrever_u32(GuestVirt(0x12020), outside | sign).unwrap();
                // Sentinela após o operando de dword também deve permanecer intacta.
                s.bus.escrever_u32(GuestVirt(0x12024), 0xDEADBEEF).unwrap();
                assert_eq!((case.run)(&mut s.state, &mut s.bus, 1).unwrap(), Exit::Returned);
                let (expected, flags) = if count != 1 {
                    (sign, 0xED7)
                } else {
                    match case.op {
                        Shift::Shl => (0, 0xE47),
                        Shift::Shr => (sign >> 1, 0xE02 | if case.width == Width::Byte { 0 } else { 4 }),
                        Shift::Sar => (sign | (sign >> 1), 0x686),
                    }
                };
                assert_eq!(s.bus.ler_u32(GuestVirt(0x12020)).unwrap(), outside | expected);
                assert_eq!(s.bus.ler_u32(GuestVirt(0x12024)).unwrap(), 0xDEADBEEF);
                assert_eq!(s.state.eflags, flags);
            }
        }
    }

    #[test]
    fn shifts_com_zero_tambem_validam_acesso_a_memoria_sem_comprometer_cpu() {
        let cases: &[(Run, &[u8])] = &[
            /* FAULT_CASES */
        ];
        for &(run, image) in cases {
            let mut s = Session::load(image).unwrap();
            for count in [0, 1, 32] {
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
    fn carry_de_shift_sobrevive_a_pausa_e_snapshot_antes_de_adc() {
        let mut s = Session::load(include_bytes!("shift_snapshot.xbe")).unwrap();
        s.state.regs[0] = 0x80000000;
        assert_eq!(shift_snapshot::run(&mut s.state, &mut s.bus, 1).unwrap(), Exit::Budget);
        assert_eq!(s.state.eflags & 1, 1);
        let mut writer = SnapshotWriter::new();
        s.state.save(&mut writer);
        let bytes = writer.finish();
        let mut restored = State::new(0);
        restored.load(&mut SnapshotReader::new(&bytes).unwrap()).unwrap();
        assert_eq!(shift_snapshot::run(&mut restored, &mut s.bus, 1).unwrap(), Exit::Returned);
        assert_eq!(restored.regs[0], 0);
        assert_eq!(restored.regs[2], 1);
        assert_eq!(restored.retired, 4);
    }
}
