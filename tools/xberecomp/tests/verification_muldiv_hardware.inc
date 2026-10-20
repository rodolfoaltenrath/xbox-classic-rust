    #[cfg(target_arch = "x86_64")]
    mod hardware {
        use super::*;
        use xbox_cpu::oracle::{self, muldiv::{self, DivideError}};
        const VALUES: [u32; 11] = [0, 1, 0x7F, 0x80, 0xFF, 0x7FFF, 0x8000, 0xFFFF, 0x7FFFFFFF, 0x80000000, u32::MAX];
        struct Truncated { width: Width, factor: Option<u32>, run: Run, image: &'static [u8] }
        const TRUNCATED: &[Truncated] = &[ /* TRUNCATED */ ];
        fn physical_width(width: Width) -> oracle::Width { match width { Width::Byte => oracle::Width::Byte, Width::Word => oracle::Width::Word, Width::Dword => oracle::Width::Dword } }
        fn mask(width: Width) -> u32 { match width { Width::Byte => 255, Width::Word => 65535, Width::Dword => u32::MAX } }

        fn product(case: &Core, s: &mut Session, initial: &State, a: u32, b: u32, old: u32) {
            let (low, high, flags) = muldiv::multiply(physical_width(case.width), case.signed, a, 0xA55A9876, b);
            s.state = initial.clone(); s.state.regs[0] = a; s.state.regs[1] = b; s.state.regs[2] = 0xA55A9876; s.state.eflags = old;
            assert_eq!((case.run)(&mut s.state, &mut s.bus, 1).unwrap(), Exit::Returned);
            assert_eq!((s.state.regs[0], s.state.regs[2]), (low, high), "mul {:?} signed={} a={a:08X} b={b:08X}", case.width, case.signed);
            assert_eq!(s.state.regs[1], b); assert_eq!(s.state.eflags, (old & !0x8D5) | (flags & 0x801));
        }

        #[test]
        fn mul_imul_implicitos_e_truncados_concordam_em_2156_casos() {
            let mut checked = 0;
            for case in CORE.iter().filter(|c| !c.divide) {
                let mut s = Session::load(case.image).unwrap(); let initial = s.state.clone();
                for a in VALUES { for b in VALUES { for old in [0x602, 0xED7] {
                    product(case, &mut s, &initial, a, b, old); checked += 1;
                }}}
            }
            for case in TRUNCATED {
                let mut s = Session::load(case.image).unwrap(); let initial = s.state.clone();
                for a in VALUES { for b in VALUES {
                    if case.factor.is_some() && b != 0 { continue; }
                    for old in [0x602, 0xED7] {
                        let factor = case.factor.unwrap_or(b);
                        let (low, _, flags) = muldiv::multiply(physical_width(case.width), true, a, 0, factor);
                        let destination = if case.factor.is_some() { 0xA55A9876 } else { a };
                        let source = if case.factor.is_some() { a } else { b };
                        s.state = initial.clone(); s.state.regs[0] = destination; s.state.regs[1] = source; s.state.regs[2] = 0xDEADBEEF; s.state.eflags = old;
                        assert_eq!((case.run)(&mut s.state, &mut s.bus, 1).unwrap(), Exit::Returned);
                        assert_eq!(s.state.regs[0], (destination & !mask(case.width)) | (low & mask(case.width)), "imul {:?} a={a:08X} factor={factor:08X}", case.width);
                        assert_eq!(s.state.regs[1], source); assert_eq!(s.state.regs[2], 0xDEADBEEF);
                        assert_eq!(s.state.eflags, (old & !0x8D5) | (flags & 0x801)); checked += 1;
                    }
                }}
            }
            assert_eq!(checked, 2156);
        }

        #[test]
        fn mul_imul_de_byte_esgotam_operandos_em_131072_casos_fisicos() {
            let mut checked = 0;
            for case in CORE.iter().filter(|c| !c.divide && c.width == Width::Byte) {
                let mut s = Session::load(case.image).unwrap(); let initial = s.state.clone();
                for a in 0..=255 { for b in 0..=255 {
                    product(case, &mut s, &initial, 0xA55A5500 | a, b, 0xED7); checked += 1;
                }}
            }
            assert_eq!(checked, 131072);
        }

        fn division(case: &Core, s: &mut Session, initial: &State, low: u32, high: u32, divisor: u32, old: u32) -> bool {
            let expected = muldiv::divide(physical_width(case.width), case.signed, low, high, divisor);
            s.state = initial.clone(); s.state.regs[0] = low; s.state.regs[1] = divisor; s.state.regs[2] = high; s.state.eflags = old;
            let before = s.state.clone(); let actual = (case.run)(&mut s.state, &mut s.bus, 1);
            match expected {
                Ok((eax, edx)) => {
                    assert_eq!(actual.unwrap(), Exit::Returned);
                    assert_eq!((s.state.regs[0], s.state.regs[2]), (eax, edx), "div {:?} signed={} low={low:08X} high={high:08X} divisor={divisor:08X}", case.width, case.signed);
                    assert_eq!(s.state.regs[1], divisor); assert_eq!(s.state.eflags, old & !0x8D5);
                    true
                }
                Err(reason) => {
                    let expected = match reason { DivideError::ZeroDivisor => DivideFault::ZeroDivisor, DivideError::QuotientOverflow => DivideFault::QuotientOverflow };
                    assert!(matches!(actual, Err(Error::Divide { pc: 0x11000, reason }) if reason == expected));
                    assert_eq!(s.state, before, "#DE preserva registradores e FLAGS");
                    false
                }
            }
        }

        #[test]
        fn div_idiv_por_largura_validam_40548_entradas_incluindo_limites_e_erros() {
            let (mut checked, mut successful) = (0, 0);
            let mut seed = 0xC0DE1234u32;
            let mut next = || { seed = seed.wrapping_mul(1664525).wrapping_add(1013904223); seed };
            for case in CORE.iter().filter(|c| c.divide) {
                let mut s = Session::load(case.image).unwrap(); let initial = s.state.clone();
                for low in VALUES { for high in VALUES { for divisor in VALUES { for old in [0x602, 0xED7] {
                    successful += usize::from(division(case, &mut s, &initial, low, high, divisor, old)); checked += 1;
                }}}}
                for _ in 0..4096 {
                    successful += usize::from(division(case, &mut s, &initial, next(), next(), next(), 0xED7)); checked += 1;
                }
            }
            assert_eq!(checked, 40548);
            assert!(successful > 0 && successful < checked);
            println!("div/idiv matrix: {successful} execuções físicas estreitas, {} diagnósticos classificados; {checked} entradas", checked - successful);
        }

        #[test]
        fn div_idiv_de_byte_esgotam_ax_com_dez_divisores_em_1310720_entradas() {
            let divisors = [0, 1, 2, 3, 7, 127, 128, 129, 254, 255];
            let (mut checked, mut successful) = (0, 0);
            for case in CORE.iter().filter(|c| c.divide && c.width == Width::Byte) {
                let mut s = Session::load(case.image).unwrap(); let initial = s.state.clone();
                for ax in 0..=65535 { for divisor in divisors {
                    let old = if ax & 1 == 0 { 0xED7 } else { 0x602 };
                    successful += usize::from(division(case, &mut s, &initial, 0xA55A0000 | ax, 0x12345678, divisor, old)); checked += 1;
                }}
            }
            assert_eq!(checked, 1310720); assert!(successful > 0 && successful < checked);
            println!("div/idiv bytes: {successful} execuções físicas estreitas, {} diagnósticos classificados; {checked} entradas", checked - successful);
        }
    }
