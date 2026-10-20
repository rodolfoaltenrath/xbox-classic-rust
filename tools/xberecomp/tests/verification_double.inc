
#[cfg(test)]
mod double_verification {
    use xbox_mem::{Bus, GuestVirt};
    use xbox_recomp::{Error, ir::{DoubleShift, Width}, runtime::{Exit, Session, State}};
    /* MODULES */
    type Run = fn(&mut State, &mut Bus, u64) -> xbox_recomp::Result<Exit>;
    struct Case { op: DoubleShift, width: Width, count: Option<u8>, run: Run, image: &'static [u8] }
    #[cfg(target_arch = "x86_64")]
    const CASES: &[Case] = &[ /* CASES */ ];
    const MEMORY_CASES: &[Case] = &[ /* MEMORY_CASES */ ];

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn shifts_duplos_concordam_com_cpu_em_196988_casos_definidos() {
        use xbox_cpu::oracle::{self, execute_double_shift};
        const VALUES: [u32; 11] = [0, 1, 0x7F, 0x80, 0xFF, 0x7FFF, 0x8000, 0xFFFF, 0x7FFFFFFF, 0x80000000, u32::MAX];
        let mut checked = 0;
        for case in CASES {
            let (width, bits) = match case.width { Width::Word => (oracle::DoubleWidth::Word, 16), Width::Dword => (oracle::DoubleWidth::Dword, 32), Width::Byte => unreachable!() };
            let op = match case.op { DoubleShift::Shld => oracle::DoubleShift::Shld, DoubleShift::Shrd => oracle::DoubleShift::Shrd };
            let mut s = Session::load(case.image).unwrap(); let initial = s.state.clone();
            for count in 0..=255 {
                if case.count.is_some_and(|c| u32::from(c) != count) || count & 31 > bits { continue; }
                for a in VALUES { for b in VALUES { for old in [0x602, 0xED7] {
                    let (value, flags) = execute_double_shift(op, width, a, b, count, old);
                    s.state = initial.clone(); s.state.regs[0] = a;
                    s.state.regs[1] = 0xA55A0000 | count; s.state.regs[2] = b; s.state.eflags = old;
                    assert_eq!((case.run)(&mut s.state, &mut s.bus, 1).unwrap(), Exit::Returned);
                    assert_eq!(s.state.regs[0], value, "{:?}/{:?} a={a:08X} b={b:08X} count={count}", case.op, case.width);
                    assert_eq!(s.state.regs[1], 0xA55A0000 | count); assert_eq!(s.state.regs[2], b);
                    let mask = match count & 31 { 0 => 0x8D5, 1 => 0x8C5, _ => 0xC5 };
                    assert_eq!(s.state.eflags & mask, flags & mask, "{:?}/{:?} a={a:08X} b={b:08X} count={count}", case.op, case.width);
                    assert_eq!(s.state.eflags & !0x8D5, old & !0x8D5);
                    assert_eq!(s.state.eflags & (0x8D5 & !mask), 0);
                    checked += 1;
                }}}
            }
        }
        assert_eq!(checked, 196988);
    }

    #[test]
    fn shifts_duplos_capturam_ecx_e_fonte_antes_de_escrever_destino() {
        let cases: &[(Run, &[u8], u32, u32, u32, u32)] = &[
            (double_dst_ecx::run, include_bytes!("double_dst_ecx.xbe"), 0x80000000, 0x40000001, 0x80000000, 0x80000003),
            (double_src_ecx::run, include_bytes!("double_src_ecx.xbe"), 0x80000000, 0x40000001, 0xC0000000, 0x40000001),
            (double_same::run, include_bytes!("double_same.xbe"), 0xA55A8001, 0, 0xA55A8001, 0),
        ];
        for &(run, image, eax, ecx, expected_eax, expected_ecx) in cases {
            let mut s = Session::load(image).unwrap(); s.state.regs[0] = eax; s.state.regs[1] = ecx;
            assert_eq!(run(&mut s.state, &mut s.bus, 1).unwrap(), Exit::Returned);
            assert_eq!(s.state.regs[0], expected_eax); assert_eq!(s.state.regs[1], expected_ecx);
        }
    }

    #[test]
    fn shifts_duplos_em_memoria_transferem_fonte_sem_alterar_vizinhos() {
        for case in MEMORY_CASES {
            let mut s = Session::load(case.image).unwrap(); let initial = s.state.clone();
            let (bits, sign, outside) = if case.width == Width::Word { (16, 0x8000, 0xA55A0000) } else { (32, 0x80000000, 0) };
            for count in [case.count.map_or(0, u32::from), 1, 32, bits] {
                s.state = initial.clone(); s.state.regs[1] = count; s.state.regs[2] = sign | 1; s.state.eflags = 0xED7;
                s.bus.escrever_u32(GuestVirt(0x12020), outside | sign | 1).unwrap();
                s.bus.escrever_u32(GuestVirt(0x12024), 0xDEADBEEF).unwrap();
                assert_eq!((case.run)(&mut s.state, &mut s.bus, 1).unwrap(), Exit::Returned);
                let (value, flags) = if count & 31 == 0 { (sign | 1, 0xED7) }
                    else if count == 16 { (sign | 1, 0x683) }
                    else { match case.op { DoubleShift::Shld => (3, 0xE07), DoubleShift::Shrd => (sign | sign >> 1, 0x687) } };
                assert_eq!(s.bus.ler_u32(GuestVirt(0x12020)).unwrap(), outside | value);
                assert_eq!(s.bus.ler_u32(GuestVirt(0x12024)).unwrap(), 0xDEADBEEF);
                assert_eq!(s.state.regs[2], sign | 1); assert_eq!(s.state.eflags, flags);
            }
        }
    }

    #[test]
    fn contagens_indefinidas_de_word_retornam_erro_com_pc_sem_modificar_estado() {
        for case in MEMORY_CASES.iter().filter(|c| c.width == Width::Word) {
            let mut s = Session::load(case.image).unwrap();
            s.bus.escrever_u32(GuestVirt(0x12020), 0xA55A8001).unwrap();
            for count in 0..=255 {
                if count & 31 <= 16 { continue; }
                s.state.regs[1] = count; s.state.eflags = 0xED7;
                let before = s.state.clone();
                assert!(matches!((case.run)(&mut s.state, &mut s.bus, 1), Err(Error::UndefinedDoubleShift { pc: 0x11000, count: n, bits: 16 }) if n == count & 31));
                assert_eq!(s.state, before); assert_eq!(s.bus.ler_u32(GuestVirt(0x12020)).unwrap(), 0xA55A8001);
            }
        }
        let cases: &[(Run, &[u8])] = &[
            (double_bad_imm::run, include_bytes!("double_bad_imm.xbe")),
            (double_bad_unmapped::run, include_bytes!("double_bad_unmapped.xbe")),
        ];
        for &(run, image) in cases {
            let mut s = Session::load(image).unwrap(); let before = s.state.clone();
            assert!(matches!(run(&mut s.state, &mut s.bus, 1), Err(Error::UndefinedDoubleShift { pc: 0x11000, count: 17, bits: 16 })));
            assert_eq!(s.state, before);
        }
    }

    #[test]
    fn shifts_duplos_com_zero_validam_memoria_sem_comprometer_cpu() {
        let cases: &[(Run, &[u8])] = &[ /* FAULT_CASES */ ];
        for &(run, image) in cases {
            let mut s = Session::load(image).unwrap();
            for count in [0, 1, 16, 32] {
                s.state.regs[1] = count; s.state.eflags = 0xED7; let before = s.state.clone();
                let code = s.bus.ler_u32(GuestVirt(0x11000)).unwrap();
                assert!(matches!(run(&mut s.state, &mut s.bus, 1), Err(Error::Memory(_))));
                assert_eq!(s.state, before); assert_eq!(s.bus.ler_u32(GuestVirt(0x11000)).unwrap(), code);
            }
        }
    }
}
