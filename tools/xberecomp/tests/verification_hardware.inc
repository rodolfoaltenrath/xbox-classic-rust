
#[cfg(all(test, target_arch = "x86_64"))]
mod hardware_verification {
    use xbox_cpu::oracle::{Alu, Width, execute};
    use xbox_mem::Bus;
    use xbox_recomp::runtime::{Exit, Session, State};

    /* MODULES */

    type Run = fn(&mut State, &mut Bus, u64) -> xbox_recomp::Result<Exit>;
    const VALUES: [u32; 11] = [0, 1, 0x7F, 0x80, 0xFF, 0x7FFF, 0x8000, 0xFFFF, 0x7FFFFFFF, 0x80000000, u32::MAX];

    #[test]
    fn alu_traduzida_concorda_com_cpu_real_em_9438_casos() {
        let cases: &[(Alu, Width, Run, &[u8])] = &[
            /* ALU_CASES */
        ];
        let mut checked = 0;
        for &(op, width, run, image) in cases {
            let mut s = Session::load(image).unwrap();
            let initial = s.state.clone();
            for a in VALUES {
                for b in VALUES {
                    for carry in [false, true] {
                        s.state = initial.clone();
                        s.state.regs[0] = a;
                        s.state.regs[1] = b;
                        s.state.eflags = 0xAD6 | u32::from(carry);
                        let (expected, flags) = execute(op, width, a, b, carry);
                        assert_eq!(run(&mut s.state, &mut s.bus, 8).unwrap(), Exit::Returned);
                        assert_eq!(s.state.regs[0], expected, "{op:?}/{width:?} a={a:08X} b={b:08X}");
                        // AF é indefinido em operações lógicas. NOT não altera flags.
                        let mask = match op {
                            Alu::Xor | Alu::And | Alu::Or | Alu::Test => 0x8C5,
                            Alu::Not => 0,
                            _ => 0x8D5,
                        };
                        assert_eq!(s.state.eflags & mask, flags & mask, "flags {op:?}/{width:?} a={a:08X} b={b:08X} cf={carry}");
                        if op == Alu::Not { assert_eq!(s.state.eflags, 0xAD6 | u32::from(carry)); }
                        checked += 1;
                    }
                }
            }
        }
        assert_eq!(checked, 9438);
    }

    #[test]
    fn adc_sbb_de_byte_concordam_com_cpu_em_262144_casos() {
        let cases: &[(Alu, Run, &[u8])] = &[
            (Alu::Adc, alu_adc8::run, include_bytes!("alu_adc8.xbe")),
            (Alu::Sbb, alu_sbb8::run, include_bytes!("alu_sbb8.xbe")),
        ];
        let mut checked = 0;
        for &(op, run, image) in cases {
            let mut s = Session::load(image).unwrap();
            let initial = s.state.clone();
            for a in 0..=255 {
                for b in 0..=255 {
                    for carry in [false, true] {
                        s.state = initial.clone();
                        s.state.regs[0] = 0xA55A0000 | a;
                        s.state.regs[1] = b;
                        s.state.eflags = 0x602 | u32::from(carry);
                        let (value, flags) = execute(op, Width::Byte, s.state.regs[0], b, carry);
                        assert_eq!(run(&mut s.state, &mut s.bus, 1).unwrap(), Exit::Returned);
                        assert_eq!(s.state.regs[0], value, "{op:?} a={a} b={b} cf={carry}");
                        assert_eq!(s.state.eflags, 0x602 | (flags & 0x8D5), "{op:?} a={a} b={b} cf={carry}");
                        checked += 1;
                    }
                }
            }
        }
        assert_eq!(checked, 262144);
    }

    fn expected_condition(index: u8, a: u32, b: u32) -> bool {
        let result = a.wrapping_sub(b);
        let overflow = a.cast_signed().overflowing_sub(b.cast_signed()).1;
        let sign = result >> 31 != 0;
        let parity = (result as u8).count_ones().is_multiple_of(2);
        match index {
            0 => overflow, 1 => !overflow, 2 => a < b, 3 => a >= b,
            4 => a == b, 5 => a != b, 6 => a <= b, 7 => a > b,
            8 => sign, 9 => !sign, 10 => parity, 11 => !parity,
            12 => a.cast_signed() < b.cast_signed(), 13 => a.cast_signed() >= b.cast_signed(),
            14 => a.cast_signed() <= b.cast_signed(), 15 => a.cast_signed() > b.cast_signed(),
            _ => unreachable!(),
        }
    }

    #[test]
    fn todos_os_jcc_curtos_e_near_concordam_em_3872_casos() {
        let cases: &[(u8, Run, &[u8])] = &[
            /* BRANCH_CASES */
        ];
        let mut checked = 0;
        for &(condition, run, image) in cases {
            let mut s = Session::load(image).unwrap();
            let initial = s.state.clone();
            for a in VALUES {
                for b in VALUES {
                    s.state = initial.clone();
                    s.state.regs[0] = a;
                    s.state.regs[1] = b;
                    assert_eq!(run(&mut s.state, &mut s.bus, 8).unwrap(), Exit::Returned);
                    assert_eq!(s.state.regs[0], u32::from(expected_condition(condition, a, b)), "jcc={condition} a={a:08X} b={b:08X}");
                    checked += 1;
                }
            }
        }
        assert_eq!(checked, 3872);
    }
}
