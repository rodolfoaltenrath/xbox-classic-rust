#[cfg(test)]
mod semaphore_verification {
    use xbox_core::{Snapshot,SnapshotReader,SnapshotWriter};
    use xbox_mem::GuestVirt;
    use xbox_recomp::runtime::{Session,Exit,Limites,Desfecho};
    mod guest {include!("kernel_semaphores.rs");}
    fn loaded()->Session {
        let mut s=Session::load(include_bytes!("kernel_semaphores.xbe")).unwrap();
        s.bus.escrever_u32(GuestVirt(0x12200),(-20000i32).cast_unsigned()).unwrap();
        s.bus.escrever_u32(GuestVirt(0x12204),u32::MAX).unwrap();s
    }
    fn word(s:&Session,at:u32)->u32{s.bus.ler_u32(GuestVirt(at)).unwrap()}
    fn saved(s:&Session)->Vec<u8>{let mut w=SnapshotWriter::new();s.save(&mut w);w.finish()}
    fn boundary(s:&mut Session,ordinal:u32){assert_eq!(guest::run(&mut s.state,&mut s.bus,1000).unwrap(),Exit::KernelCall{ordinal});}
    fn blocked()->Session {
        let mut s=loaded();boundary(&mut s,193);let call=s.kernel_call().unwrap();let esp=call.return_esp;
        s.atender_kernel().unwrap();assert_eq!(s.state.regs[4],esp);assert_eq!(s.kernel.semaphore(word(&s,0x12100)),Some((0,1)));
        boundary(&mut s,255);s.atender_kernel().unwrap();assert_eq!(s.kernel.current_thread(),2);
        boundary(&mut s,99);s.atender_kernel().unwrap();assert_eq!(s.kernel.current_thread(),1);
        boundary(&mut s,233);assert_eq!(s.atender_kernel_ou_bloquear().unwrap(),xbox_recomp::runtime::Servico::Bloqueado);s
    }
    fn finish(s:&mut Session){
        assert_eq!(s.conduzir(Limites{blocos_por_fatia:1000,fatias:20},guest::run,|_|Ok(false)).unwrap(),Desfecho::Retornou);
        assert_eq!(word(s,0x12108),0);assert_eq!(word(s,0x12110),1);assert_eq!(word(s,0x12114),1);
        assert_eq!(s.kernel.semaphore(word(s,0x12100)),None);assert_eq!(s.kernel.recusadas(),0);
    }
    #[test]
    fn semaphore_blocks_parent_until_child_releases_and_replays_full_session(){
        let mut s=blocked();let checkpoint=saved(&s);finish(&mut s);let complete=saved(&s);
        let mut r=loaded();r.load(&mut SnapshotReader::new(&checkpoint).unwrap()).unwrap();assert!(saved(&r)==checkpoint);
        finish(&mut r);assert!(saved(&r)==complete);
    }
    #[test]
    fn krnl_v15_migration_keeps_legacy_state_without_inheriting_a_semaphore(){
        let mut source=loaded();boundary(&mut source,193);source.atender_kernel().unwrap();
        let mut old=saved(&source);let at=old.windows(4).rposition(|v|v==b"SEMA").unwrap();
        let n=u32::from_le_bytes(old[at+8..at+12].try_into().unwrap())as usize;assert_eq!(n,1);old.drain(at..at+12+n*12);
        let k=old.windows(4).rposition(|v|v==b"KRNL").unwrap();old[k+4..k+8].copy_from_slice(&15u32.to_le_bytes());
        let mut target=loaded();boundary(&mut target,193);target.atender_kernel().unwrap();let h=word(&target,0x12100);
        target.load(&mut SnapshotReader::new(&old).unwrap()).unwrap();assert_eq!(target.kernel.semaphore(h),None);
        let modern=saved(&target);let mut again=loaded();again.load(&mut SnapshotReader::new(&modern).unwrap()).unwrap();
        assert_eq!(again.kernel.semaphore(h),None);assert!(saved(&again)==modern);
    }
}
