#[cfg(test)]
mod tick_verification {
 use xbox_core::{Snapshot,SnapshotReader,SnapshotWriter,Clock};
 use xbox_mem::GuestVirt;
 use xbox_recomp::runtime::{Session,Exit,TickPublication};
 mod poll{include!("tick_poll.rs");}mod rep{include!("tick_rep.rs");}mod step{include!("tick_step.rs");}
 fn tick(s:&Session)->u32{s.bus.ler_u32(GuestVirt(0x7FFF0300)).unwrap()}
 fn saved(s:&Session)->Vec<u8>{let mut w=SnapshotWriter::new();s.save(&mut w);w.finish()}
 #[test]
 fn polling_guest_enxerga_tick_dentro_da_execucao_sem_kernel_ou_fim_de_fatia(){
  let mut s=Session::load(include_bytes!("tick_poll.xbe")).unwrap();s.state.clock.advance(733329);
  assert_eq!(poll::run(&mut s.state,&mut s.bus,1000).unwrap(),Exit::Returned);assert_eq!(s.state.regs[0],1);assert_eq!(tick(&s),1);assert!(s.state.retired<100);assert_eq!(s.kernel.atendidas(),0);
  assert!(s.bus.escrever_u32(GuestVirt(0x7FFF0300),77).is_err());
 }
 #[test]
 fn perfil_legado_mantem_polling_parado_ate_publicacao_explicita(){
  let mut s=Session::load(include_bytes!("tick_poll.xbe")).unwrap();s.state.tick_publication=TickPublication::BoundariesOnly;s.state.clock.advance(733329);
  assert_eq!(poll::run(&mut s.state,&mut s.bus,1000).unwrap(),Exit::Budget);assert_eq!(tick(&s),0);s.publicar_tick_count().unwrap();assert_eq!(tick(&s),1);assert_eq!(poll::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::Returned);
 }
 #[test]
 fn rep_parcial_publica_tick_e_snapshot_reproduz_cpu_ram_e_perfil(){
  let image=include_bytes!("tick_rep.xbe");let mut s=Session::load(image).unwrap();s.state.clock.advance(733332);s.state.regs[1]=3;s.state.regs[7]=0x12100;s.state.regs[0]=0x44;
  assert_eq!(rep::run(&mut s.state,&mut s.bus,1).unwrap(),Exit::Budget);assert_eq!(tick(&s),0);assert_eq!(s.state.retired,0);
  assert_eq!(rep::run(&mut s.state,&mut s.bus,1).unwrap(),Exit::Budget);assert_eq!(tick(&s),1);assert_eq!(s.state.retired,0);assert_eq!(s.state.regs[1],1);
  let checkpoint=saved(&s);assert_eq!(rep::run(&mut s.state,&mut s.bus,20).unwrap(),Exit::Returned);let complete=saved(&s);
  let mut r=Session::load(image).unwrap();r.load(&mut SnapshotReader::new(&checkpoint).unwrap()).unwrap();assert!(saved(&r)==checkpoint);assert_eq!(rep::run(&mut r.state,&mut r.bus,20).unwrap(),Exit::Returned);assert!(saved(&r)==complete);
 }
 #[test]
 fn tick_ulong_da_volta_sem_saturar_ou_alterar_contagem_de_ciclos(){
  let mut s=Session::load(include_bytes!("tick_step.xbe")).unwrap();let boundary=Clock::ms_to_cycles(1u64<<32);s.state.clock.advance(boundary-1);s.publicar_tick_count().unwrap();assert_eq!(tick(&s),u32::MAX);
  step::run(&mut s.state,&mut s.bus,1).unwrap();assert_eq!(tick(&s),0);assert_eq!(s.state.clock.cycles(),boundary);assert_eq!(s.state.retired,1);
 }
}
