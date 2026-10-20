
#[cfg(test)]
mod scan_verification {
    use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
    use xbox_mem::{Bus, GuestVirt, Acesso, Perm};
    use xbox_recomp::{Error, runtime::{Session, State, Exit}};
    type Run = fn(&mut State, &mut Bus, u64) -> xbox_recomp::Result<Exit>;
    type Case = (bool,u32,Run,&'static [u8]);
    /* MODULES */
    const REGISTERS: &[Case] = &[ /* REGISTERS */ ];
    const MEMORY: &[Case] = &[ /* MEMORY */ ];
    const ADDRESSES: &[Case] = &[ /* ADDRESSES */ ];
    fn index(reverse:bool,bits:u32,value:u32)->Option<u32> {
        let candidates=(0..bits).filter(|bit| value & (1u32<<bit) != 0);
        if reverse { candidates.last() } else { candidates.into_iter().next() }
    }
    fn expected(reverse:bool,bits:u32,old:u32,value:u32)->u32 {
        match index(reverse,bits,value) { None=>old,Some(i) if bits==16=>(old&0xFFFF0000)|i,Some(i)=>i }
    }
    fn expected_flags(old:u32,bits:u32,value:u32)->u32 {
        let mask=if bits==16 {0xFFFF} else {u32::MAX};
        (old & !0x8D5) | if value&mask==0 {0x40} else {0}
    }

    #[test]
    fn scans_cobrem_indices_zero_mascaras_word_e_politica_de_flags_indefinidas() {
        for &(reverse,bits,run,image) in REGISTERS {
            let mut s=Session::load(image).unwrap(); let initial=s.state.clone();
            let mut values=vec![0,1,3,0x8000,0x10000,0x80000000,u32::MAX,0xFFFFFFFE];
            values.extend((0..32).map(|bit| 1u32<<bit));
            for value in values { for old_flags in [0xED7,0x602] {
                s.state=initial.clone(); s.state.eflags=old_flags;
                s.state.regs[0]=0xABCDEF01; s.state.regs[1]=value;
                assert_eq!(run(&mut s.state,&mut s.bus,1).unwrap(),Exit::Returned);
                assert_eq!(s.state.regs[0],expected(reverse,bits,0xABCDEF01,value));
                assert_eq!(s.state.regs[1],value); assert_eq!(s.state.eflags,expected_flags(old_flags,bits,value));
                assert_eq!((s.state.retired,s.state.clock.tsc()),(2,2));
            }}
        }
    }

    #[test]
    fn scans_leem_memoria_ro_sem_escrever_nem_confundir_zero_em_word() {
        for &(reverse,bits,run,image) in MEMORY {
            let mut s=Session::load(image).unwrap(); let initial=s.state.clone();
            let frame=s.bus.traduzir(GuestVirt(0x12000),Acesso::Leitura).unwrap();
            for value in [0,1,0x8000,0x10000,0x80000000,0x80341280] {
                s.bus.mapear(GuestVirt(0x12000),frame,1,Perm::RW).unwrap();
                s.bus.escrever_u32(GuestVirt(0x12020),value).unwrap();
                s.bus.mapear(GuestVirt(0x12000),frame,1,Perm::RO).unwrap();
                s.state=initial.clone(); s.state.eflags=0xED7; s.state.regs[0]=0xABCDEF01; s.state.regs[6]=0x12020;
                run(&mut s.state,&mut s.bus,1).unwrap();
                assert_eq!(s.state.regs[0],expected(reverse,bits,0xABCDEF01,value));
                assert_eq!(s.state.eflags,expected_flags(0xED7,bits,value));
                assert!(s.bus.mmu().acessado(GuestVirt(0x12020))); assert!(!s.bus.mmu().sujo(GuestVirt(0x12020)));
                assert_eq!(s.bus.ler_u32(GuestVirt(0x12020)).unwrap(),value);
            }
        }
    }

    #[test]
    fn scan_com_falta_ou_leitura_cruzando_pagina_preserva_cpu_ate_resolucao() {
        for &(reverse,bits,run,image) in MEMORY {
            let mut s=Session::load(image).unwrap(); s.state.eflags=0xED7; s.state.regs[0]=0xABCDEF01; s.state.regs[6]=0x50000FFF;
            let before=s.state.clone();
            assert!(matches!(run(&mut s.state,&mut s.bus,1),Err(Error::Memory(f)) if f.acesso==Acesso::Leitura)); assert_eq!(s.state,before);
            let frame=s.bus.frames_mut().alocar().unwrap(); s.bus.mapear(GuestVirt(0x50000000),frame,1,Perm::RW).unwrap();
            s.bus.escrever_u8(GuestVirt(0x50000FFF),0x80).unwrap();
            assert!(matches!(run(&mut s.state,&mut s.bus,1),Err(Error::Memory(f)) if f.endereco==GuestVirt(0x50001000))); assert_eq!(s.state,before);
            let frame=s.bus.frames_mut().alocar().unwrap(); s.bus.mapear(GuestVirt(0x50001000),frame,1,Perm::RW).unwrap();
            s.bus.escrever_bytes(GuestVirt(0x50001000),&[0x12,0x34,0x80]).unwrap();
            run(&mut s.state,&mut s.bus,1).unwrap();
            assert_eq!(s.state.regs[0],expected(reverse,bits,0xABCDEF01,0x80341280));
            assert_eq!(s.state.eflags,expected_flags(0xED7,bits,0x80341280));
        }
    }

    #[test]
    fn scans_capturam_aliases_enderecos_fs_sib_e_destinos_sp_esp() {
        for (reverse,bits,self_run,sp_run,self_image,sp_image) in [ /* ALIASES */ ] {
            for value in [0,0x80000020] {
                let mut s=Session::load(self_image).unwrap(); s.state.regs[0]=value; s.state.eflags=0xED7;
                self_run(&mut s.state,&mut s.bus,1).unwrap(); assert_eq!(s.state.regs[0],expected(reverse,bits,value,value));
                let mut s=Session::load(sp_image).unwrap(); let esp=s.state.regs[4];
                s.state.regs[0]=value; s.state.eflags=0xED7;
                assert_eq!(sp_run(&mut s.state,&mut s.bus,1).unwrap(),Exit::Breakpoint{pc:0x11003+u32::from(bits==16)});
                assert_eq!(s.state.regs[4],expected(reverse,bits,esp,value));
                assert_eq!(s.state.eflags,expected_flags(0xED7,bits,value));
            }
        }
        for &(reverse,bits,run,image) in ADDRESSES {
            let mut s=Session::load(image).unwrap(); s.state.regs[0]=0x10; s.state.regs[1]=3;
            s.bus.escrever_u32(GuestVirt(s.state.fs_base+0x20),0x80000020).unwrap();
            run(&mut s.state,&mut s.bus,1).unwrap(); assert_eq!(s.state.regs[0],expected(reverse,bits,0x10,0x80000020));
        }
    }

    #[test]
    fn zf_alimenta_jz_e_snapshot_replay_preserva_zero_e_fatias() {
        for value in [0,0x80000020] {
            let mut s=Session::load(include_bytes!("scan_branch.xbe")).unwrap();
            s.state.regs[0]=0xABCDEF01; s.state.regs[1]=value; s.state.eflags=0xED7;
            let before=s.state.clone(); assert_eq!(scan_branch::run(&mut s.state,&mut s.bus,0).unwrap(),Exit::Budget); assert_eq!(s.state,before);
            assert_eq!(scan_branch::run(&mut s.state,&mut s.bus,1).unwrap(),Exit::Budget);
            let mut writer=SnapshotWriter::new(); s.state.save(&mut writer); let checkpoint=writer.finish();
            assert_eq!(scan_branch::run(&mut s.state,&mut s.bus,20).unwrap(),Exit::Returned);
            assert_eq!(s.state.regs[3],if value==0 {2} else {1});
            assert_eq!(s.state.regs[0],if value==0 {0xABCDEF01} else {31});
            let complete=s.state.clone();
            s.state.load(&mut SnapshotReader::new(&checkpoint).unwrap()).unwrap();
            scan_branch::run(&mut s.state,&mut s.bus,20).unwrap(); assert_eq!(s.state,complete);
            assert_eq!(s.state.clock.tsc(),5);
        }
    }

    #[test]
    fn scans_respeitam_limite_do_clock_antes_de_mudar_destino_e_flags() {
        let mut s=Session::load(include_bytes!("bsr_32_reg.xbe")).unwrap();
        s.state.regs[0]=0xABCDEF01; s.state.eflags=0xED7; s.state.clock.advance(u64::MAX);
        let before=s.state.clone(); assert!(matches!(bsr_32_reg::run(&mut s.state,&mut s.bus,1),Err(Error::TimeExhausted{pc:0x11000}))); assert_eq!(s.state,before);
        s.state.clock=xbox_core::Clock::new(); s.state.clock.advance(u64::MAX-1);
        assert!(matches!(bsr_32_reg::run(&mut s.state,&mut s.bus,1),Err(Error::TimeExhausted{pc:0x11003})));
        assert_eq!(s.state.regs,before.regs); assert_eq!(s.state.eflags,0x642);
        assert_eq!((s.state.retired,s.state.clock.tsc()),(1,u64::MAX));
    }

    #[cfg(target_arch="x86_64")]
    #[test]
    fn scans_rust_gerado_conferem_com_cpu_em_262144_casos_definidos() {
        use xbox_cpu::oracle::{Width,scan::bit_scan};
        let mut count=0; let mut zeros=0;
        for &(reverse,bits,run,image) in REGISTERS {
            let mut s=Session::load(image).unwrap(); let initial=s.state.clone();
            let width=if bits==16 {Width::Word} else {Width::Dword}; let mut random=0x12345678u32;
            for i in 0..65536 {
                random=random.wrapping_mul(1664525).wrapping_add(1013904223);
                let source=if bits==16 {0xA5A50000|i} else {match i {0=>0,1..=32=>1u32<<(i-1),33=>u32::MAX,_=>random}};
                let destination=random.rotate_left(7); let (result,flags)=bit_scan(reverse,width,destination,source);
                s.state=initial.clone(); s.state.regs[0]=destination; s.state.regs[1]=source;
                s.state.eflags=if i%2==0 {0xED7} else {0x602};
                let old_flags=s.state.eflags; run(&mut s.state,&mut s.bus,1).unwrap();
                let masked=if bits==16 {source&0xFFFF} else {source};
                if masked!=0 { assert_eq!(s.state.regs[0],result); } else { zeros+=1; }
                assert_eq!(s.state.eflags&0x40,flags&0x40);
                assert_eq!(s.state.eflags&0x895,0); assert_eq!(s.state.eflags&!0x8D5,old_flags&!0x8D5);
                count+=1;
            }
        }
        assert_eq!(count,262144); assert!(zeros>=4);
        println!("bsf/bsr: {count} comparações físicas; {zeros} origens zero comparadas apenas em ZF");
    }
}
