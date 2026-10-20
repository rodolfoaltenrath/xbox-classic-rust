
#[cfg(test)]
mod conditional_verification {
    use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
    use xbox_mem::{Bus, GuestVirt, GuestPhys, Acesso, Perm};
    use xbox_recomp::{Error, runtime::{Session, State, Exit, RETURN_SENTINEL}};
    type Run = fn(&mut State, &mut Bus, u64) -> xbox_recomp::Result<Exit>;
    type Case = (u8,u32,Run,&'static [u8]);
    /* MODULES */
    const REGISTERS: &[Case] = &[ /* REGISTERS */ ];
    const MEMORY: &[Case] = &[ /* MEMORY */ ];
    fn flags(bits:u32)->u32 {
        let positions=[0,2,4,6,7,11]; let mut result=0x602;
        for (i,position) in positions.into_iter().enumerate() { result|=((bits>>i)&1)<<position; } result
    }
    fn holds(cc:u8, f:u32)->bool {
        let c=f&1!=0; let p=f&4!=0; let z=f&0x40!=0; let s=f&0x80!=0; let o=f&0x800!=0;
        match cc {0=>o,1=>!o,2=>c,3=>!c,4=>z,5=>!z,6=>c||z,7=>!c&&!z,
            8=>s,9=>!s,10=>p,11=>!p,12=>s!=o,13=>s==o,14=>z||s!=o,15=>!z&&s==o,_=>unreachable!()}
    }
    fn merged(old:u32, source:u32,bits:u32)->u32 { if bits==16 { (old&0xFFFF0000)|(source&0xFFFF) } else {source} }
    fn fresh(s:&mut Session, initial:&State, f:u32) { s.state=initial.clone(); s.state.eflags=f; }

    #[test]
    fn cmovcc_registradores_cobrem_condicoes_flags_e_word_preserva_metade_alta() {
        for &(cc,bits,run,image) in REGISTERS {
            let mut s=Session::load(image).unwrap(); let initial=s.state.clone();
            for seed in 0..64 {
                let f=flags(seed); fresh(&mut s,&initial,f);
                s.state.regs[0]=0xABCD1234; s.state.regs[1]=0x5678FEDC;
                let expected=if holds(cc,f) { merged(0xABCD1234,0x5678FEDC,bits) } else {0xABCD1234};
                assert_eq!(run(&mut s.state,&mut s.bus,1).unwrap(),Exit::Returned);
                assert_eq!(s.state.regs[0],expected); assert_eq!(s.state.regs[1],0x5678FEDC);
                assert_eq!(s.state.eflags,f); assert_eq!((s.state.retired,s.state.clock.tsc()),(2,2));
            }
        }
    }

    #[test]
    fn cmovcc_memoria_ro_e_lida_mesmo_com_condicao_falsa_incluindo_mmio() {
        for &(cc,bits,run,image) in MEMORY {
            let mut s=Session::load(image).unwrap(); let initial=s.state.clone();
            s.bus.escrever_u32(GuestVirt(0x12020),0x5678FEDC).unwrap();
            let frame=s.bus.traduzir(GuestVirt(0x12000),Acesso::Leitura).unwrap();
            s.bus.mapear(GuestVirt(0x12000),frame,1,Perm::RO).unwrap();
            for truth in [false,true] {
                let f=(0..64).map(flags).find(|f| holds(cc,*f)==truth).unwrap();
                fresh(&mut s,&initial,f); s.state.regs[0]=0xABCD1234; s.state.regs[6]=0x12020;
                run(&mut s.state,&mut s.bus,1).unwrap();
                assert_eq!(s.state.regs[0],if truth { merged(0xABCD1234,0x5678FEDC,bits) } else {0xABCD1234});
                assert_eq!(s.state.eflags,f); assert!(s.bus.mmu().acessado(GuestVirt(0x12020)));
                assert!(!s.bus.mmu().sujo(GuestVirt(0x12020)));
                assert_eq!(s.bus.ler_u32(GuestVirt(0x12020)).unwrap(),0x5678FEDC);
            }
        }
        // MMIO não implementado só gera telemetria; CMOV falso não elimina a leitura.
        let mut s=Session::load(include_bytes!("cmov_5_32_mem.xbe")).unwrap();
        s.bus.mapear(GuestVirt(0x50000000),GuestPhys(0xFD000000),1,Perm::RW).unwrap();
        s.state.regs[6]=0x50000000; s.state.regs[0]=123; s.state.eflags=0x42;
        cmov_5_32_mem::run(&mut s.state,&mut s.bus,1).unwrap();
        assert_eq!(s.state.regs[0],123); assert_eq!(s.bus.telemetria().total_de_acessos(),1);
    }

    #[test]
    fn origem_nao_mapeada_falha_antes_de_modificar_cpu_independente_da_condicao() {
        for &(cc,_,run,image) in MEMORY {
            let mut s=Session::load(image).unwrap(); let initial=s.state.clone();
            for truth in [false,true] {
                let f=(0..64).map(flags).find(|f| holds(cc,*f)==truth).unwrap();
                fresh(&mut s,&initial,f); s.state.regs[6]=0x50000000;
                let before=s.state.clone(); let err=run(&mut s.state,&mut s.bus,1).unwrap_err();
                assert!(matches!(err,Error::Memory(fault) if fault.acesso==Acesso::Leitura));
                assert_eq!(s.state,before);
            }
        }
    }

    #[test]
    fn cmovcc_cruzando_pagina_falha_ou_le_com_largura_correta() {
        for (bits,run,image) in [(16,cmov_4_16_mem::run as Run,include_bytes!("cmov_4_16_mem.xbe").as_slice()),
            (32,cmov_4_32_mem::run as Run,include_bytes!("cmov_4_32_mem.xbe").as_slice())] {
            let mut s=Session::load(image).unwrap();
            let frame=s.bus.frames_mut().alocar().unwrap(); s.bus.mapear(GuestVirt(0x50000000),frame,1,Perm::RW).unwrap();
            s.bus.escrever_u8(GuestVirt(0x50000FFF),0xDC).unwrap();
            s.state.regs[6]=0x50000FFF; s.state.regs[0]=0xABCD1234;
            for f in [2,0x42] {
                s.state.eflags=f; let before=s.state.clone();
                assert!(matches!(run(&mut s.state,&mut s.bus,1),Err(Error::Memory(fault)) if fault.endereco==GuestVirt(0x50001000)));
                assert_eq!(s.state,before);
            }
            let frame=s.bus.frames_mut().alocar().unwrap(); s.bus.mapear(GuestVirt(0x50001000),frame,1,Perm::RW).unwrap();
            s.bus.escrever_bytes(GuestVirt(0x50001000),&[0xFE,0x78,0x56]).unwrap();
            run(&mut s.state,&mut s.bus,1).unwrap(); assert_eq!(s.state.regs[0],merged(0xABCD1234,0x5678FEDC,bits));
        }
    }

    #[test]
    fn cmovcc_captura_aliases_de_registro_endereco_fs_sib_e_destino_esp() {
        for (bits,self_run,sp_run,self_image,sp_image) in [
            (16,cmov_self16::run as Run,cmov_sp16::run as Run,include_bytes!("cmov_self16.xbe").as_slice(),include_bytes!("cmov_sp16.xbe").as_slice()),
            (32,cmov_self32::run as Run,cmov_sp32::run as Run,include_bytes!("cmov_self32.xbe").as_slice(),include_bytes!("cmov_sp32.xbe").as_slice())] {
            for f in [2,0x42] {
                let mut s=Session::load(self_image).unwrap(); s.state.eflags=f; s.state.regs[0]=0xABCD1234;
                self_run(&mut s.state,&mut s.bus,1).unwrap(); assert_eq!(s.state.regs[0],0xABCD1234);
                let mut s=Session::load(sp_image).unwrap(); s.state.eflags=f; let esp=s.state.regs[4];
                s.bus.escrever_u32(GuestVirt(esp-4),RETURN_SENTINEL).unwrap();
                s.state.regs[0]=if bits==16 {0xABCD0000|((esp-4)&0xFFFF)} else {esp-4};
                sp_run(&mut s.state,&mut s.bus,1).unwrap(); assert_eq!(s.state.regs[4],if f&0x40!=0 {esp} else {esp+4});
            }
        }
        let mut s=Session::load(include_bytes!("cmov_fs_sib.xbe")).unwrap();
        s.state.eflags=0x42; s.state.regs[0]=0x10; s.state.regs[1]=3;
        s.bus.escrever_u32(GuestVirt(s.state.fs_base+0x20),0xBADC0FFE).unwrap();
        cmov_fs_sib::run(&mut s.state,&mut s.bus,1).unwrap(); assert_eq!(s.state.regs[0],0xBADC0FFE);
    }

    #[test]
    fn cmovcc_snapshot_de_flags_fatias_orcamento_e_limite_de_tempo() {
        let image=include_bytes!("cmov_snapshot.xbe"); let mut s=Session::load(image).unwrap();
        s.state.regs[0]=7; s.state.regs[3]=7; s.state.regs[1]=123;
        let before=s.state.clone(); assert_eq!(cmov_snapshot::run(&mut s.state,&mut s.bus,0).unwrap(),Exit::Budget); assert_eq!(s.state,before);
        assert_eq!(cmov_snapshot::run(&mut s.state,&mut s.bus,1).unwrap(),Exit::Budget);
        let mut writer=SnapshotWriter::new(); s.state.save(&mut writer); let checkpoint=writer.finish();
        cmov_snapshot::run(&mut s.state,&mut s.bus,1).unwrap(); assert_eq!(s.state.regs[0],123); let complete=s.state.clone();
        s.state.load(&mut SnapshotReader::new(&checkpoint).unwrap()).unwrap();
        cmov_snapshot::run(&mut s.state,&mut s.bus,8).unwrap(); assert_eq!(s.state,complete);
        let mut limit=Session::load(include_bytes!("cmov_4_32_reg.xbe")).unwrap(); limit.state.clock.advance(u64::MAX); let before=limit.state.clone();
        assert!(matches!(cmov_4_32_reg::run(&mut limit.state,&mut limit.bus,1),Err(Error::TimeExhausted{pc:0x11000}))); assert_eq!(limit.state,before);
    }

    #[cfg(target_arch="x86_64")]
    #[test]
    fn cmovcc_confere_resultados_e_flags_com_cpu_fisica_em_32768_casos() {
        use xbox_cpu::oracle::{Width,conditional::conditional_move};
        let mut count=0;
        for &(cc,bits,run,image) in REGISTERS {
            let mut s=Session::load(image).unwrap(); let initial=s.state.clone();
            let width=if bits==16 {Width::Word} else {Width::Dword}; let mut random=0x12345678u32;
            for seed in 0..64 {
                for case in 0..16 {
                    random=random.wrapping_mul(1664525).wrapping_add(1013904223);
                    let a=match case {0=>0,1=>u32::MAX,2=>0x80000000,3=>0x7FFFFFFF,_=>random};
                    let b=if case<4 {!a} else {random.rotate_left(13)}; let f=flags(seed);
                    let (expected,actual_flags)=conditional_move(cc,width,a,b,f);
                    fresh(&mut s,&initial,f); s.state.regs[0]=a; s.state.regs[1]=b;
                    run(&mut s.state,&mut s.bus,1).unwrap();
                    assert_eq!(s.state.regs[0],expected,"cc={cc}, bits={bits}, flags={f:08X}");
                    assert_eq!(s.state.eflags&0x8D5,actual_flags&0x8D5); assert_eq!(s.state.eflags,f);
                    count+=1;
                }
            }
        }
        assert_eq!(count,32768); println!("cmovcc: {count} comparações com CPU física");
    }
}
