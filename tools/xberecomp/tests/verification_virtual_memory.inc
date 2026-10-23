#[cfg(test)]
mod virtual_memory_verification {
    use xbox_core::snapshot::{Snapshot,SnapshotReader,SnapshotWriter};
    use xbox_kernel::dispatch::STATUS_NO_MEMORY;
    use xbox_mem::{Acesso,GuestVirt,Perm};
    use xbox_recomp::runtime::{Session,Exit,ESP};
    mod allocate { include!("kernel_virtual_memory.rs"); }

    fn loaded()->Session {
        let mut s=Session::load(include_bytes!("kernel_virtual_memory.xbe")).unwrap();
        s.bus.escrever_u32(GuestVirt(0x12104),0x1001).unwrap();s
    }
    fn boundary(s:&mut Session) { assert_eq!(allocate::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::KernelCall{ordinal:184}); }
    fn saved(s:&Session)->Vec<u8> { let mut out=SnapshotWriter::new();s.save(&mut out);out.finish() }
    fn first(s:&mut Session) {
        s.state.eflags=0xED7;let call=s.kernel_call().unwrap();let before=s.state.clone();
        s.atender_kernel().unwrap();let mut expected=before;expected.regs[0]=0;
        expected.regs[ESP]=call.return_esp;expected.eip=call.return_pc;assert_eq!(s.state,expected);
    }
    fn finish(s:&mut Session) {
        s.atender_kernel().unwrap();assert_eq!(allocate::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::Returned);
        let regions=s.kernel.virtual_memory().allocations();assert_eq!(regions.len(),2);
        assert_eq!(s.bus.ler_u32(GuestVirt(regions[0].base+4094)).unwrap(),0xCAFEBABE);
        assert_eq!(s.bus.ler_u32(GuestVirt(regions[1].base)).unwrap(),0);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12120)).unwrap(),0);assert_eq!(s.kernel.atendidas(),2);
    }
    #[test]
    fn reserva_commit_stdcall_limpa_pilha_preserva_flags_e_guest_usa_memoria_entre_paginas() {
        let mut s=loaded();let esp=s.state.regs[ESP];boundary(&mut s);let used=s.bus.frames().usados();first(&mut s);
        assert_eq!(s.state.regs[ESP],esp);let base=s.bus.ler_u32(GuestVirt(0x12100)).unwrap();
        assert_eq!(base%65536,0);assert_eq!(s.bus.ler_u32(GuestVirt(0x12104)).unwrap(),8192);
        assert_eq!(s.bus.frames().usados(),used+2);assert!(s.bus.traduzir(GuestVirt(base),Acesso::Execucao).is_err());
        boundary(&mut s);finish(&mut s);assert_eq!(s.state.regs[ESP],esp+4);assert_eq!(s.state.eflags,0xED7);
    }
    #[test]
    fn snapshot_restabelece_regioes_ram_frames_e_segunda_alocacao_pendente() {
        let mut s=loaded();boundary(&mut s);first(&mut s);boundary(&mut s);let checkpoint=saved(&s);
        finish(&mut s);let complete=saved(&s);let mut restored=loaded();let mut inp=SnapshotReader::new(&checkpoint).unwrap();
        Snapshot::load(&mut restored,&mut inp).unwrap();assert_eq!(inp.remaining(),0);assert!(saved(&restored)==checkpoint);
        finish(&mut restored);assert!(saved(&restored)==complete);
        let region=restored.kernel.virtual_memory().allocations()[0];
        let pa=restored.bus.mmu().consultar(GuestVirt(region.base)).unwrap().0;
        restored.bus.mapear(GuestVirt(region.base),pa,1,Perm::RO).unwrap();let invalid=saved(&restored);
        assert!(Snapshot::load(&mut loaded(),&mut SnapshotReader::new(&invalid).unwrap()).is_err());
    }
    #[test]
    fn oom_retorna_ntstatus_sem_regioes_frames_ou_saidas_parciais() {
        let mut s=loaded();let size=(s.bus.frames().livres()+1)*4096;
        s.bus.escrever_u32(GuestVirt(0x12104),size).unwrap();boundary(&mut s);
        let used=s.bus.frames().usados();let pages=s.bus.mmu().paginas_mapeadas();let clock=s.state.clock;
        s.atender_kernel().unwrap();assert_eq!(s.state.regs[0],STATUS_NO_MEMORY);assert_eq!(s.state.clock,clock);
        assert_eq!(s.bus.frames().usados(),used);assert_eq!(s.bus.mmu().paginas_mapeadas(),pages);
        assert!(s.kernel.virtual_memory().allocations().is_empty());
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12100)).unwrap(),0);assert_eq!(s.bus.ler_u32(GuestVirt(0x12104)).unwrap(),size);
    }
    #[test]
    fn retorno_invalido_recusa_antes_de_alocar_e_saida_sobre_retorno_usa_continuacao_capturada() {
        let mut s=loaded();boundary(&mut s);let esp=s.state.regs[ESP];let call=s.kernel_call().unwrap();
        s.bus.escrever_u32(GuestVirt(esp),0x50000000).unwrap();let before=s.state.clone();let used=s.bus.frames().usados();
        assert!(s.atender_kernel().is_err());assert_eq!(s.state,before);assert_eq!(s.bus.frames().usados(),used);
        assert!(s.kernel.virtual_memory().allocations().is_empty());
        s.bus.escrever_u32(GuestVirt(esp),call.return_pc).unwrap();s.bus.escrever_u32(GuestVirt(esp+12),esp).unwrap();
        first(&mut s);assert_eq!(s.state.eip,call.return_pc);
        assert_eq!(s.bus.ler_u32(GuestVirt(esp)).unwrap(),(call.return_pc+4095)&!4095);
        boundary(&mut s);finish(&mut s);
    }
    #[test]
    fn aots_legado_krnl_v5_recupera_imagem_e_executa_nova_alocacao() {
        let mut s=loaded();boundary(&mut s);let mut legacy=saved(&s);
        let kernel=legacy.windows(4).rposition(|tag|tag==b"KRNL").unwrap();
        let vm=legacy.windows(4).rposition(|tag|tag==b"VMEM").unwrap();legacy.truncate(vm);
        legacy[kernel+4..kernel+8].copy_from_slice(&5u32.to_le_bytes());
        let session=legacy.windows(4).position(|tag|tag==b"AOTS").unwrap();
        legacy[session+4..session+8].copy_from_slice(&1u32.to_le_bytes());
        let mut restored=loaded();let mut inp=SnapshotReader::new(&legacy).unwrap();Snapshot::load(&mut restored,&mut inp).unwrap();
        assert_eq!(inp.remaining(),0);assert!(restored.kernel.virtual_memory().image().is_some());
        first(&mut s);boundary(&mut s);finish(&mut s);
        first(&mut restored);boundary(&mut restored);finish(&mut restored);
        assert_eq!(restored.scheduling_profile(),xbox_recomp::runtime::SchedulingProfile::LegacyBootstrap);assert!(saved(&restored)==super::legacy_scheduler_snapshot(&saved(&s)));
    }
}
