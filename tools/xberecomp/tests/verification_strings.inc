/* MODULES */

#[cfg(test)]
mod string_verification {
    use super::*;
    use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
    use xbox_mem::{GuestVirt, Perm};
    use xbox_recomp::ir::{StringOp, Repeat, Width};
    use xbox_recomp::runtime::{State, Session, Exit};

    type Run = fn(&mut State, &mut xbox_mem::Bus, u64) -> xbox_recomp::Result<Exit>;
    struct Case { op: StringOp, width: Width, repeat: Repeat, address16: bool, run: Run, image: &'static [u8] }
    const CASES: &[Case] = &[ /* CASES */ ];

    fn case(op: StringOp, width: Width, repeat: Repeat, address16: bool) -> &'static Case {
        CASES.iter().find(|c| c.op == op && c.width == width && c.repeat == repeat && c.address16 == address16).unwrap()
    }
    fn mapping(s: &mut Session, va: u32) {
        let frame = s.bus.frames_mut().alocar().unwrap();
        s.bus.mapear(GuestVirt(va), frame, 1, Perm::RW).unwrap();
    }
    fn snapshot(s: &Session) -> Vec<u8> {
        let mut w = SnapshotWriter::new(); s.state.save(&mut w); s.bus.save(&mut w); w.finish()
    }
    fn restore(image: &[u8], bytes: &[u8]) -> Session {
        let mut s = Session::load(image).unwrap();
        let mut r = SnapshotReader::new(bytes).unwrap();
        s.state.load(&mut r).unwrap(); s.bus.load(&mut r).unwrap(); s
    }

    #[test]
    fn count_branches_preservam_flags_wrap_e_cx_alto() {
        let cases = [ /* COUNT_CASES */ ];
        for (op, word, run, image) in cases {
            let mut s = Session::load(image).unwrap();
            let initial_esp = s.state.regs[4];
            for count in [0u32, 1, 2, 0xFFFF, 0x10000, 0xABCD0001, u32::MAX] {
                for flags in [0x202, 0xED7] {
                    s.state = State::new(0x11000); s.state.regs[4] = initial_esp;
                    s.state.regs[1] = count; s.state.eflags = flags;
                    let mask = if word { 0xFFFF } else { u32::MAX };
                    let after = if op == "Zero" { count & mask } else { count.wrapping_sub(1) & mask };
                    let branch = match op { "Zero" => after == 0, "Loop" => after != 0, "Equal" => after != 0 && flags & 0x40 != 0, _ => after != 0 && flags & 0x40 == 0 };
                    assert_eq!(run(&mut s.state, &mut s.bus, 3).unwrap(), Exit::Returned);
                    assert_eq!(s.state.regs[0], u32::from(branch));
                    assert_eq!(s.state.regs[1], (count & !mask) | after);
                    assert_eq!(s.state.eflags, flags); assert_eq!(s.state.retired, 3);
                }
            }
        }
    }

    #[test]
    fn loop_zero_e_rep_enorme_respeitam_orcamento_e_zero_nao_toca_estado() {
        let mut s = Session::load(include_bytes!("count_forever.xbe")).unwrap();
        assert_eq!(count_forever::run(&mut s.state, &mut s.bus, 3).unwrap(), Exit::Budget);
        assert_eq!(s.state.regs[1], u32::MAX - 2); assert_eq!(s.state.eip, 0x11000);
        let c = case(StringOp::Stos, Width::Byte, Repeat::Always, false);
        let mut s = Session::load(c.image).unwrap();
        s.state.regs[1] = u32::MAX; s.state.regs[7] = 0x12020;
        let before = snapshot(&s);
        assert_eq!((c.run)(&mut s.state, &mut s.bus, 0).unwrap(), Exit::Budget);
        assert_eq!(snapshot(&s), before);
        assert_eq!((c.run)(&mut s.state, &mut s.bus, 3).unwrap(), Exit::Budget);
        assert_eq!(s.state.regs[1], u32::MAX - 3); assert_eq!(s.state.regs[7], 0x12023);
        assert_eq!(s.state.eip, 0x11000); assert_eq!(s.state.retired, 0);
    }

    #[test]
    fn rep_zero_nao_acessa_memoria_nem_avalia_zf() {
        for c in CASES.iter().filter(|c| c.repeat != Repeat::None) {
            let mut s = Session::load(c.image).unwrap();
            s.state.regs[1] = if c.address16 { 0xABCD0000 } else { 0 };
            s.state.regs[6] = 0xDEAD0000; s.state.regs[7] = 0xBEEF0000;
            s.state.eflags = 0xED7;
            let before = s.state.clone();
            assert_eq!((c.run)(&mut s.state, &mut s.bus, 1).unwrap(), Exit::Budget);
            assert_eq!(s.state.regs, before.regs); assert_eq!(s.state.eflags, before.eflags);
            assert_eq!(s.state.retired, 1); assert!(s.state.repeat.is_none());
        }
    }

    #[test]
    fn strings_address16_preservam_metades_altas_e_wrap_dos_indices() {
        for c in CASES.iter().filter(|c| c.address16) {
            let mut s = Session::load(c.image).unwrap(); mapping(&mut s, 0); mapping(&mut s, 0xF000);
            s.bus.escrever_u32(GuestVirt(0xFFFC), 0xFFFFFFFF).unwrap();
            s.state.regs[6] = 0xAAAAFFFC; s.state.regs[7] = 0xBBBBFFFC;
            s.state.regs[1] = 0xCCCC0002; s.state.regs[0] = u32::MAX;
            s.state.eflags = 0x202;
            // Igualdade na primeira comparação permite REPE; REPNE para na primeira.
            let elements = if c.repeat == Repeat::None || c.repeat == Repeat::NotEqual { 1 } else { 2 };
            (c.run)(&mut s.state, &mut s.bus, 8).unwrap();
            let low = 0xFFFCu32.wrapping_add(elements * c.width.bytes()) & 0xFFFF;
            assert_eq!(s.state.regs[6], 0xAAAA0000 | if matches!(c.op, StringOp::Movs | StringOp::Lods | StringOp::Cmps) { low } else { 0xFFFC });
            assert_eq!(s.state.regs[7], 0xBBBB0000 | if c.op != StringOp::Lods { low } else { 0xFFFC });
            let count = if c.repeat == Repeat::None { 2 } else { 2 - elements };
            assert_eq!(s.state.regs[1], 0xCCCC0000 | count);
        }
        // Decremento de DI=0 atravessa para 0xFFFF sem tocar a metade alta.
        let c = case(StringOp::Stos, Width::Byte, Repeat::Always, true);
        let mut s = Session::load(c.image).unwrap(); mapping(&mut s, 0); mapping(&mut s, 0xF000);
        s.state.regs[7] = 0xBEEF0000; s.state.regs[1] = 2; s.state.regs[0] = 0xA5; s.state.eflags |= 0x400;
        (c.run)(&mut s.state, &mut s.bus, 8).unwrap();
        assert_eq!(s.state.regs[7], 0xBEEFFFFE);
        assert_eq!(s.bus.ler_u8(GuestVirt(0)).unwrap(), 0xA5);
        assert_eq!(s.bus.ler_u8(GuestVirt(0xFFFF)).unwrap(), 0xA5);
    }

    #[test]
    fn rep_movs_sobreposto_copia_em_ordem_e_separa_setup_do_bloco() {
        let mut s = Session::load(include_bytes!("string_setup.xbe")).unwrap();
        s.bus.escrever_bytes(GuestVirt(0x12020), &[1,2,3,4,5]).unwrap();
        s.state.regs[6] = 0x12020; s.state.regs[7] = 0x12021; s.state.regs[1] = 4;
        string_setup::run(&mut s.state, &mut s.bus, 1).unwrap();
        assert_eq!(s.state.retired, 1); assert_eq!(s.state.regs[1], 4);
        string_setup::run(&mut s.state, &mut s.bus, 4).unwrap();
        assert_eq!(s.state.retired, 2);
        let mut bytes = [0;5]; s.bus.ler_bytes(GuestVirt(0x12020), &mut bytes).unwrap();
        assert_eq!(bytes, [1;5]);
        string_setup::run(&mut s.state, &mut s.bus, 1).unwrap();
        assert_eq!(s.state.retired, 4); assert!(s.state.returned);
    }

    #[test]
    fn snapshot_no_meio_de_cada_rep_reproduz_estado_final() {
        for c in CASES.iter().filter(|c| c.repeat != Repeat::None) {
            let mut s = Session::load(c.image).unwrap();
            if c.address16 { mapping(&mut s, 0x2000); }
            let base = if c.address16 { 0x2020 } else { 0x12020 };
            s.bus.escrever_bytes(GuestVirt(base), &[7;32]).unwrap();
            s.state.regs[0] = 0x07070707; s.state.regs[1] = 3;
            s.state.regs[6] = base; s.state.regs[7] = base;
            // REPNE deve continuar: SCAS usa EAX=0; CMPS compara fontes diferentes.
            if c.repeat == Repeat::NotEqual {
                s.state.regs[0] = 0; s.state.regs[7] = base + 16;
                s.bus.escrever_bytes(GuestVirt(base + 16), &[0;16]).unwrap();
                if c.op == StringOp::Scas { s.bus.escrever_bytes(GuestVirt(base + 16), &[1;16]).unwrap(); }
            }
            assert_eq!((c.run)(&mut s.state, &mut s.bus, 1).unwrap(), Exit::Budget);
            assert_eq!(s.state.regs[1], 2); assert_eq!(s.state.retired, 0);
            let checkpoint = snapshot(&s);
            (c.run)(&mut s.state, &mut s.bus, 8).unwrap();
            let expected = snapshot(&s);
            let mut resumed = restore(c.image, &checkpoint);
            for _ in 0..4 { if (c.run)(&mut resumed.state, &mut resumed.bus, 1).unwrap() == Exit::Returned { break; } }
            assert_eq!(snapshot(&resumed), expected);
        }
    }

    #[test]
    fn faltas_rep_preservam_elementos_anteriores_e_nao_comprometem_o_atual() {
        for op in [StringOp::Movs, StringOp::Stos, StringOp::Lods, StringOp::Cmps, StringOp::Scas] {
            for repeat in if matches!(op, StringOp::Cmps | StringOp::Scas) { &[Repeat::Equal, Repeat::NotEqual][..] } else { &[Repeat::Always][..] } {
                let c = case(op, Width::Byte, *repeat, false);
                let mut s = Session::load(c.image).unwrap();
                let flags = 0xA93;
                s.state.regs[0] = 7; s.state.regs[1] = 3;
                s.state.regs[6] = 0x12020; s.state.regs[7] = 0x12FFF; s.state.eflags = flags;
                s.bus.escrever_u8(GuestVirt(0x12020), 7).unwrap();
                s.bus.escrever_u8(GuestVirt(0x12FFF), if *repeat == Repeat::NotEqual { 8 } else { 7 }).unwrap();
                if op == StringOp::Lods { s.state.regs[6] = 0x12FFF; }
                (c.run)(&mut s.state, &mut s.bus, 1).unwrap();
                let previous = s.state.clone();
                // LODS/MOVS/CMPS precisam de fonte não nula no segundo elemento.
                assert_eq!(previous.regs[1], 2);
                assert!((c.run)(&mut s.state, &mut s.bus, 1).is_err());
                assert_eq!(s.state.regs, previous.regs); assert_eq!(s.state.retired, 0);
                assert_eq!(s.state.eip, 0x11000); assert_eq!(s.state.eflags, flags);
                let checkpoint = snapshot(&s);
                let mut resumed = restore(c.image, &checkpoint);
                mapping(&mut resumed, 0x13000);
                resumed.bus.escrever_bytes(GuestVirt(0x13000), &[if *repeat == Repeat::NotEqual { 8 } else { 7 };2]).unwrap();
                (c.run)(&mut resumed.state, &mut resumed.bus, 8).unwrap();
                assert!(resumed.state.returned); assert!(resumed.state.repeat.is_none());
            }
        }
    }

    #[test]
    fn store_rep_cruzando_pagina_invalida_nao_deixa_escrita_parcial() {
        let c = case(StringOp::Stos, Width::Dword, Repeat::Always, false);
        let mut s = Session::load(c.image).unwrap();
        s.state.regs[0] = u32::MAX; s.state.regs[1] = 2; s.state.regs[7] = 0x12FFE;
        let before = s.state.clone();
        assert!((c.run)(&mut s.state, &mut s.bus, 1).is_err());
        assert_eq!(s.state, before); assert_eq!(s.bus.ler_u16(GuestVirt(0x12FFE)).unwrap(), 0);
        // Fonte válida e destino RX: falha de escrita sem avançar os índices.
        let c = case(StringOp::Movs, Width::Dword, Repeat::Always, false);
        let mut s = Session::load(c.image).unwrap(); s.state.regs[1] = 1;
        s.state.regs[6] = 0x12020; s.state.regs[7] = 0x11000;
        let before = s.state.clone(); assert!((c.run)(&mut s.state, &mut s.bus, 1).is_err()); assert_eq!(s.state, before);
    }

    #[test]
    fn std_cld_so_alteram_df() {
        let mut s = Session::load(include_bytes!("direction.xbe")).unwrap();
        s.state.eflags = 0xAD7;
        direction::run(&mut s.state, &mut s.bus, 1).unwrap(); assert_eq!(s.state.eflags, 0xED7);
        direction::run(&mut s.state, &mut s.bus, 1).unwrap(); assert_eq!(s.state.eflags, 0xAD7);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn strings_recompiladas_conferem_com_cpu_fisica() {
        use xbox_cpu::oracle::{Width as HW, strings::{self, StringOp as HS, Repeat as HR}};
        let mut checked = 0;
        for c in CASES.iter().filter(|c| !c.address16) {
            let mut s = Session::load(c.image).unwrap(); let esp = s.state.regs[4];
            let width = match c.width { Width::Byte => HW::Byte, Width::Word => HW::Word, Width::Dword => HW::Dword };
            let op = match c.op { StringOp::Movs => HS::Movs, StringOp::Stos => HS::Stos, StringOp::Lods => HS::Lods, StringOp::Cmps => HS::Cmps, StringOp::Scas => HS::Scas };
            let repeat = match c.repeat { Repeat::None => HR::None, Repeat::Always => HR::Always, Repeat::Equal => HR::Equal, Repeat::NotEqual => HR::NotEqual };
            for backwards in [false, true] { for count in [0,1,2,7] { for seed in 0u32..12 {
                let mut source = [0u8;32]; let mut destination = [0u8;32];
                for i in 0..32 { source[i] = (i as u32 * 37 + seed * 19) as u8; destination[i] = if seed % 3 == 0 { source[i] } else { (i as u32 * 41 + seed) as u8 }; }
                if seed % 3 == 2 { destination[12..16].copy_from_slice(&source[12..16]); }
                let flags = (if backwards { 0x400 } else { 0 }) | if seed % 2 == 0 { 0xAD7 } else { 0x202 };
                let eax = if seed % 4 == 0 { u32::from_le_bytes(destination[..4].try_into().unwrap()) } else { 0xABCD1234 };
                let result = strings::execute(op, width, repeat, &source, &mut destination, count, eax, flags);
                s.state = State::new(0x11000); s.state.regs[4] = esp;
                let elements = if c.repeat == Repeat::None { 1 } else { count };
                let start = if backwards { (elements * c.width.bytes()).saturating_sub(c.width.bytes()) } else { 0 };
                s.state.regs[0] = eax; s.state.regs[1] = count; s.state.regs[6] = 0x12000 + start; s.state.regs[7] = 0x12040 + start; s.state.eflags = flags;
                s.bus.escrever_bytes(GuestVirt(0x12000), &source).unwrap();
                // Oráculo já alterou destination: reconstruir entrada original.
                let mut input = [0u8;32]; for i in 0..32 { input[i] = if seed % 3 == 0 { source[i] } else { (i as u32 * 41 + seed) as u8 }; }
                if seed % 3 == 2 { input[12..16].copy_from_slice(&source[12..16]); }
                s.bus.escrever_bytes(GuestVirt(0x12040), &input).unwrap();
                assert_eq!((c.run)(&mut s.state, &mut s.bus, 16).unwrap(), Exit::Returned);
                assert_eq!(s.state.regs[0], result.eax); assert_eq!(s.state.regs[1], result.count);
                assert_eq!(s.state.regs[6], (0x12000i64 + i64::from(start) + result.source_delta) as u32);
                assert_eq!(s.state.regs[7], (0x12040i64 + i64::from(start) + result.destination_delta) as u32);
                assert_eq!(s.state.eflags & 0xCD5, result.flags & 0xCD5);
                let mut actual = [0;32]; s.bus.ler_bytes(GuestVirt(0x12040), &mut actual).unwrap(); assert_eq!(actual, destination);
                assert_eq!(s.state.retired, 2); checked += 1;
            } } }
        }
        assert_eq!(checked, 3456); println!("strings: {checked} comparações com CPU física");
    }
}
