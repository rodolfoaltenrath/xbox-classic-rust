
#[cfg(test)] mod adc_chain;
#[cfg(test)] mod sbb_chain;
#[cfg(test)] mod adc_high;
#[cfg(test)] mod sbb_high;
#[cfg(test)] mod adc_memory;
#[cfg(test)] mod sbb_memory;
#[cfg(test)] mod adc_load;
#[cfg(test)] mod sbb_load;
#[cfg(test)] mod adc_readonly;
#[cfg(test)] mod sbb_readonly;

#[cfg(test)]
mod carry_verification {
    use super::*;
    use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
    use xbox_mem::{Bus, GuestVirt};
    use xbox_recomp::runtime::{Exit, Session, State};

    type Run = fn(&mut State, &mut Bus, u64) -> xbox_recomp::Result<Exit>;

    #[test]
    fn carry_entre_palavras_sobrevive_a_orcamento_e_snapshot() {
        let cases: &[(Run, &[u8], u32, u32, u32, u32)] = &[
            (adc_chain::run, include_bytes!("adc_chain.xbe"), u32::MAX, 0, 0, 1),
            (sbb_chain::run, include_bytes!("sbb_chain.xbe"), 0, 1, u32::MAX, 0),
        ];
        for &(run, image, low, high, expected_low, expected_high) in cases {
            let mut s = Session::load(image).unwrap();
            s.state.regs[0] = low;
            s.state.regs[1] = 1;
            s.state.regs[2] = high;
            s.state.regs[3] = 0;
            assert_eq!(run(&mut s.state, &mut s.bus, 1).unwrap(), Exit::Budget);
            assert_eq!(s.state.eflags & 1, 1);
            assert_eq!(s.state.retired, 2);
            let mut writer = SnapshotWriter::new();
            s.state.save(&mut writer);
            let bytes = writer.finish();
            let mut restored = State::new(0);
            restored.load(&mut SnapshotReader::new(&bytes).unwrap()).unwrap();
            assert_eq!(run(&mut s.state, &mut s.bus, 1).unwrap(), Exit::Returned);
            assert_eq!(run(&mut restored, &mut s.bus, 1).unwrap(), Exit::Returned);
            assert_eq!(restored, s.state);
            assert_eq!(restored.regs[0], expected_low);
            assert_eq!(restored.regs[2], expected_high);
        }
    }

    #[test]
    fn adc_sbb_em_ah_preservam_outros_bits_e_calculam_overflow() {
        let cases: &[(Run, &[u8], u32, u32, u32)] = &[
            (adc_high::run, include_bytes!("adc_high.xbe"), 0x12347F56, 0x12348056, 0x890),
            (sbb_high::run, include_bytes!("sbb_high.xbe"), 0x12348056, 0x12347F56, 0x810),
        ];
        for &(run, image, input, expected, flags) in cases {
            let mut s = Session::load(image).unwrap();
            s.state.regs[0] = input;
            s.state.regs[1] = 0;
            s.state.eflags = 0x603;
            assert_eq!(run(&mut s.state, &mut s.bus, 1).unwrap(), Exit::Returned);
            assert_eq!(s.state.regs[0], expected);
            assert_eq!(s.state.eflags, 0x602 | flags);
        }
    }

    #[test]
    fn adc_sbb_com_memoria_e_imediatos_preservam_largura_e_carry() {
        let cases: &[(Run, &[u8], u32, u32, u32, u32)] = &[
            (adc_memory::run, include_bytes!("adc_memory.xbe"), 0xA55A7F00, 0xA55A7F00, 1, 0x55),
            (sbb_memory::run, include_bytes!("sbb_memory.xbe"), 0xA55A0000, 0xA55A0000, 1, 0x55),
            (adc_memory::run, include_bytes!("adc_memory.xbe"), 0xA55A7F01, 0xA55A7F00, 0, 0x55),
            (sbb_memory::run, include_bytes!("sbb_memory.xbe"), 0xA55A0001, 0xA55A0002, 0, 0x11),
            (adc_load::run, include_bytes!("adc_load.xbe"), u32::MAX, u32::MAX, 1, 0x55),
            (sbb_load::run, include_bytes!("sbb_load.xbe"), u32::MAX, u32::MAX, 1, 0x55),
        ];
        for &(run, image, data, expected_data, carry, flags) in cases {
            let mut s = Session::load(image).unwrap();
            s.bus.escrever_u32(GuestVirt(0x12020), data).unwrap();
            s.state.eflags = 0x602 | carry;
            assert_eq!(run(&mut s.state, &mut s.bus, 1).unwrap(), Exit::Returned);
            assert_eq!(s.state.regs[0], 0);
            assert_eq!(s.bus.ler_u32(GuestVirt(0x12020)).unwrap(), expected_data);
            assert_eq!(s.state.eflags, 0x602 | flags);
        }
    }

    #[test]
    fn escrita_adc_sbb_recusada_preserva_registradores_flags_e_memoria() {
        let cases: &[(Run, &[u8])] = &[
            (adc_readonly::run, include_bytes!("adc_readonly.xbe")),
            (sbb_readonly::run, include_bytes!("sbb_readonly.xbe")),
        ];
        for &(run, image) in cases {
            let mut s = Session::load(image).unwrap();
            s.state.regs[1] = u32::MAX;
            s.state.eflags = 0xAD7;
            let before = s.state.clone();
            let code = s.bus.ler_u32(GuestVirt(0x11000)).unwrap();
            assert!(run(&mut s.state, &mut s.bus, 1).is_err());
            assert_eq!(s.state, before);
            assert_eq!(s.bus.ler_u32(GuestVirt(0x11000)).unwrap(), code);
        }
    }
}
