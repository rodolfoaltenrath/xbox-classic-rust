#[cfg(test)]
mod dma_verification {
    use xbox_core::{Snapshot,SnapshotReader,SnapshotWriter};
    use xbox_mem::GuestVirt;
    use xbox_recomp::runtime::{Session,Exit,Limites,Desfecho};
    mod guest {include!("dma_guest.rs");}
    fn loaded()->Session{Session::load(include_bytes!("dma_guest.xbe")).unwrap()}
    fn saved(s:&Session)->Vec<u8>{let mut w=SnapshotWriter::new();s.save(&mut w);w.finish()}
    fn pending(s:&mut Session){assert_eq!(guest::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::KernelCall{ordinal:1});assert_eq!(s.bus.ler_u32(GuestVirt(0xFD003244)).unwrap(),0);}
    fn finish(s:&mut Session){s.atender_kernel().unwrap();assert_eq!(guest::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::Returned);assert_eq!(s.bus.ler_u32(GuestVirt(0x12100)).unwrap(),0x20000);}
    #[test]
    fn drive_consume_jump_real_e_retorna_get_sem_instrucoes_ou_clock_extras(){
        let mut s=loaded();s.state.eflags=0xED7;
        assert_eq!(s.conduzir(Limites{blocos_por_fatia:1000,fatias:4},guest::run,|_|Ok(false)).unwrap(),Desfecho::Retornou);
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12100)).unwrap(),0x20000);assert_eq!(s.state.eflags,0xED7);assert_eq!(s.state.retired,15);assert_eq!(s.state.clock.cycles(),15);
    }
    #[test]
    fn snapshot_antes_do_consumo_reproduz_jump_e_estado_integral(){
        let mut s=loaded();pending(&mut s);let checkpoint=saved(&s);let cpu=s.state.clone();assert_eq!(s.bus.service_gpu(1).unwrap().words,1);assert_eq!(s.state,cpu);finish(&mut s);let complete=saved(&s);
        let mut restored=loaded();restored.load(&mut SnapshotReader::new(&checkpoint).unwrap()).unwrap();assert!(saved(&restored)==checkpoint);restored.bus.service_gpu(1).unwrap();finish(&mut restored);assert!(saved(&restored)==complete);
    }
    #[test]
    fn descritor_invalido_para_gpu_nao_avanca_cpu_e_reparo_permite_consumir(){
        let mut s=loaded();pending(&mut s);s.bus.escrever_u32(GuestVirt(0xFD7111C0),0xB003).unwrap();let cpu=s.state.clone();
        assert!(s.bus.service_gpu(1).is_err());assert_eq!(s.state,cpu);assert_eq!(s.bus.ler_u32(GuestVirt(0xFD003244)).unwrap(),0);
        s.bus.escrever_u32(GuestVirt(0xFD7111C0),0xB002).unwrap();assert!(s.bus.service_gpu(1).unwrap().drained);finish(&mut s);
    }
}
