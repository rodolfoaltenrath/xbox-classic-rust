
#[cfg(test)]
mod exchange_verification {
    use xbox_mem::{Bus, GuestVirt, Perm, Acesso};
    use xbox_recomp::runtime::{Session, State, Exit};
    type Run = fn(&mut State, &mut Bus, u64) -> xbox_recomp::Result<Exit>;
    /* MODULES */
    fn mask(bits: u32) -> u32 { u32::MAX >> (32 - bits) }
    fn fresh(s: &mut Session, esp: u32) {
        s.state = State::new(0x11000);
        s.state.regs = [0xA5F07F80, 0xDEAD1234, 0x0123FEFF, 0x76543210, esp, 0x12030, 0x12020, 0x12020];
        s.state.eflags = 0xED7;
    }
    fn part(regs: &[u32; 8], bits: u32, encoding: usize) -> u32 {
        let (r, shift) = if bits == 8 && encoding >= 4 { (encoding - 4, 8) } else { (encoding, 0) };
        (regs[r] >> shift) & mask(bits)
    }
    fn put(regs: &mut [u32; 8], bits: u32, encoding: usize, value: u32) {
        let (r, shift) = if bits == 8 && encoding >= 4 { (encoding - 4, 8) } else { (encoding, 0) };
        regs[r] = (regs[r] & !(mask(bits) << shift)) | ((value & mask(bits)) << shift);
    }
    #[test]
    fn cmpxchg_registradores_preservam_fatias_e_capturam_aliases() {
        for (bits, dst, src, run, image) in [ /* REGISTERS */ ] {
            let mut s = Session::load(image).unwrap(); let esp = s.state.regs[4];
            for equal in [false, true] {
                fresh(&mut s, esp);
                if bits != 8 && dst == 4 {
                    s.state.regs[0] = (s.state.regs[0] & !mask(bits)) | ((esp + u32::from(!equal) * 4) & mask(bits));
                }
                if equal { let a = s.state.regs[0]; put(&mut s.state.regs, bits, dst, a); }
                let a = part(&s.state.regs, bits, 0); let b = part(&s.state.regs, bits, dst);
                let source = part(&s.state.regs, bits, src);
                let mut expected = s.state.regs;
                if a == b { put(&mut expected, bits, dst, source); } else { put(&mut expected, bits, 0, b); }
                assert_eq!(run(&mut s.state, &mut s.bus, 8).unwrap(), Exit::Returned);
                expected[4] = esp + 4;
                assert_eq!(s.state.regs, expected, "bits={bits}, dst={dst}, src={src}");
            }
        }
    }
    #[test]
    fn cmpxchg_memoria_e_lock_preservam_o_resultado_e_flags() {
        for (bits, _, run, image) in [ /* MEMORY */ ] {
            let mut s = Session::load(image).unwrap(); let esp = s.state.regs[4];
            for equal in [false, true] {
                fresh(&mut s, esp); let a = s.state.regs[0];
                let before = if equal { a } else { a ^ 1 };
                s.bus.escrever_u32(GuestVirt(0x12020), before).unwrap();
                let source = s.state.regs[2];
                assert_eq!(run(&mut s.state, &mut s.bus, 8).unwrap(), Exit::Returned);
                assert_eq!(s.bus.ler_u32(GuestVirt(0x12020)).unwrap(), if equal { (before & !mask(bits)) | (source & mask(bits)) } else { before });
                assert_eq!(s.state.regs[0], if equal { a } else { (a & !mask(bits)) | (before & mask(bits)) });
                assert_eq!(s.state.eflags & 0x40 != 0, equal);
                assert_eq!(s.state.eflags & !0x8D5, 0xED7 & !0x8D5);
            }
        }
    }
    #[test]
    fn cmpxchg_desigual_em_ro_falha_sem_alterar_cpu_mesmo_sem_lock() {
        for (_, _, run, image) in [ /* MEMORY */ ] {
            let mut s = Session::load(image).unwrap(); let esp = s.state.regs[4]; fresh(&mut s, esp);
            s.state.regs[6] = 0x11000; s.state.regs[0] = 0;
            let before = s.state.clone();
            assert!(run(&mut s.state, &mut s.bus, 8).is_err()); assert_eq!(s.state, before);
            s.state.regs[6]=0x50000000; let before=s.state.clone();
            let error=run(&mut s.state,&mut s.bus,8).unwrap_err();
            assert!(matches!(error,xbox_recomp::Error::Memory(fault) if fault.acesso==Acesso::Escrita));
            assert_eq!(s.state,before);
        }
    }
    #[test]
    fn cmpxchg_endereco_sib_com_eax_fs_e_regiao_mmio() {
        let mut s=Session::load(include_bytes!("cx_sib.xbe")).unwrap();
        s.state.regs[0]=0x12010; s.state.regs[1]=3; s.state.regs[2]=99;
        s.bus.escrever_u32(GuestVirt(0x12020),7).unwrap();
        cx_sib::run(&mut s.state,&mut s.bus,8).unwrap();
        assert_eq!(s.state.regs[0],7); assert_eq!(s.bus.ler_u32(GuestVirt(0x12020)).unwrap(),7);
        assert!(s.bus.mmu().sujo(GuestVirt(0x12020)));
        let mut s=Session::load(include_bytes!("cx_fs.xbe")).unwrap();
        s.state.regs[6]=0x14; s.state.regs[0]=0; s.state.regs[2]=99;
        cx_fs::run(&mut s.state,&mut s.bus,8).unwrap();
        assert_eq!(s.bus.ler_u32(GuestVirt(s.state.fs_base+0x14)).unwrap(),99);
        let mut s=Session::load(include_bytes!("cx_mem_32_1.xbe")).unwrap();
        s.bus.mapear(GuestVirt(0x50000000),xbox_mem::GuestPhys(0xFD000000),1,Perm::RW).unwrap();
        s.state.regs[6]=0x50000000; let before=s.state.clone();
        let error=cx_mem_32_1::run(&mut s.state,&mut s.bus,8).unwrap_err();
        assert!(error.to_string().contains("CMPXCHG em MMIO")); assert_eq!(s.state,before);
    }
    #[test]
    fn cmpxchg_cruzando_pagina_valida_falta_e_snapshot() {
        use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
        let image = include_bytes!("cx_mem_32_1.xbe");
        let mut s = Session::load(image).unwrap(); let esp = s.state.regs[4];
        let frame = s.bus.frames_mut().alocar().unwrap();
        s.bus.mapear(GuestVirt(0x50000000), frame, 1, Perm::RW).unwrap();
        s.bus.escrever_u8(GuestVirt(0x50000FFF), 0x80).unwrap();
        fresh(&mut s, esp); s.state.regs[6] = 0x50000FFF;
        let before = s.state.clone();
        assert!(cx_mem_32_1::run(&mut s.state, &mut s.bus, 1).is_err());
        assert_eq!(s.state, before); assert_eq!(s.bus.ler_u8(GuestVirt(0x50000FFF)).unwrap(), 0x80);
        let frame = s.bus.frames_mut().alocar().unwrap();
        s.bus.mapear(GuestVirt(0x50001000), frame, 1, Perm::RW).unwrap();
        s.bus.escrever_u32(GuestVirt(0x50000FFF), s.state.regs[0]).unwrap();
        let mut writer = SnapshotWriter::new(); s.state.save(&mut writer); s.bus.save(&mut writer); let bytes = writer.finish();
        assert_eq!(cx_mem_32_1::run(&mut s.state, &mut s.bus, 1).unwrap(), Exit::Returned);
        let completed = s.state.clone(); let value = s.bus.ler_u32(GuestVirt(0x50000FFF)).unwrap();
        let mut reader = SnapshotReader::new(&bytes).unwrap(); s.state.load(&mut reader).unwrap(); s.bus.load(&mut reader).unwrap();
        cx_mem_32_1::run(&mut s.state, &mut s.bus, 1).unwrap();
        assert_eq!(s.state, completed); assert_eq!(s.bus.ler_u32(GuestVirt(0x50000FFF)).unwrap(), value);
        assert!(s.bus.traduzir(GuestVirt(0x11000), Acesso::Escrita).is_err());
    }
    fn truth(cc: u32, flags: u32) -> bool {
        let c=flags&1!=0; let p=flags&4!=0; let z=flags&0x40!=0; let s=flags&0x80!=0; let o=flags&0x800!=0;
        [o,!o,c,!c,z,!z,c||z,!c&&!z,s,!s,p,!p,s!=o,s==o,z||s!=o,!z&&s==o][cc as usize]
    }
    #[test]
    fn setcc_todas_condicoes_al_ah_memoria_e_flags_preservadas() {
        for (cc, mode, run, image) in [ /* CONDITIONS */ ] {
            let mut s = Session::load(image).unwrap(); let esp = s.state.regs[4];
            for combination in 0u32..32 {
                fresh(&mut s, esp);
                let flags = 0x602 | (combination & 1) | ((combination & 2)<<1) | ((combination & 4)<<4) | ((combination & 8)<<4) | ((combination & 16)<<7);
                s.state.eflags = flags; let mut expected = s.state.regs;
                let value = u32::from(truth(cc, flags));
                if mode < 2 { put(&mut expected, 8, if mode==0 {0} else {4}, value); }
                run(&mut s.state, &mut s.bus, 8).unwrap(); expected[4] = esp+4;
                assert_eq!(s.state.regs, expected); assert_eq!(s.state.eflags, flags);
                if mode==2 { assert_eq!(s.bus.ler_u8(GuestVirt(0x12020)).unwrap(), value as u8); }
            }
        }
    }
    #[test]
    fn setcc_falha_de_store_preserva_cpu_e_orcamento_zero() {
        let mut s=Session::load(include_bytes!("set_4_2.xbe")).unwrap(); s.state.regs[6]=0x11000;
        let before=s.state.clone(); assert_eq!(set_4_2::run(&mut s.state,&mut s.bus,0).unwrap(),Exit::Budget);
        assert_eq!(s.state,before); assert!(set_4_2::run(&mut s.state,&mut s.bus,8).is_err()); assert_eq!(s.state,before);
    }
    #[cfg(target_arch="x86_64")]
    #[test]
    fn cmpxchg_rust_gerado_confere_com_cpu_fisica() {
        use xbox_cpu::oracle::{Width, exchange::compare_exchange};
        let mut count=0u32;
        for (bits, locked, run, image) in [ /* MEMORY */ ] {
            let mut s=Session::load(image).unwrap(); let esp=s.state.regs[4];
            let width=match bits {8=>Width::Byte,16=>Width::Word,_=>Width::Dword};
            let iterations=if bits==8 {65536} else {4096};
            let mut random=0x12345678u32;
            for i in 0..iterations {
                random=random.wrapping_mul(1664525).wrapping_add(1013904223);
                let a=if bits==8 {0xABCD0000 | ((i>>8)&255)} else {random};
                let b=if bits==8 {0x12340000 | (i&255)} else if i%2==0 {a} else {random.rotate_left(7)};
                let source=random.rotate_right(5);
                let (expected_a,expected_b,flags)=compare_exchange(width,a,b,source,locked);
                fresh(&mut s,esp); s.state.regs[0]=a; s.state.regs[2]=source;
                s.bus.escrever_u32(GuestVirt(0x12020),b).unwrap();
                run(&mut s.state,&mut s.bus,8).unwrap();
                assert_eq!(s.state.regs[0],expected_a); assert_eq!(s.bus.ler_u32(GuestVirt(0x12020)).unwrap(),expected_b);
                assert_eq!(s.state.eflags&0x8D5,flags&0x8D5); count+=1;
            }
        }
        println!("cmpxchg: {count} comparações com CPU física");
    }
}
