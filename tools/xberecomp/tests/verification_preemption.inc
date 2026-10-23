#[cfg(test)]
mod preemption_verification {
 use xbox_core::{Snapshot,SnapshotReader,SnapshotWriter,Clock};
 use xbox_kernel::{dispatch::Irql,imports,loader::RETURN_SENTINEL};
 use xbox_mem::{GuestVirt,Bus};
 use xbox_recomp::{Result,Error,runtime::{Session,State,Exit,Limites,Desfecho,INTERRUPT_FLAG,InterruptProfile}};
 mod sti{include!("irq_sti.rs");}mod cli{include!("irq_cli.rs");}mod twice{include!("irq_sti_twice.rs");}mod negated{include!("irq_sti_cli.rs");}mod rep{include!("irq_sti_rep.rs");}mod forever{include!("irq_sti_loop.rs");}
 type Run=fn(&mut State,&mut Bus,u64)->Result<Exit>;
 fn saved(s:&Session)->Vec<u8>{let mut w=SnapshotWriter::new();s.save(&mut w);w.finish()}
 fn word(s:&Session,at:u32)->u32{s.bus.ler_u32(GuestVirt(at)).unwrap()}
 fn setup(bytes:&[u8],pending:bool)->Session{
  let mut s=Session::load(bytes).unwrap();let mut regs=s.state.regs;regs[4]-=0x100;
  for (ordinal,args) in [(109,&[0x12800,0x11100,0,0x33,24,0,1][..]),(98,&[0x12800][..])]{s.bus.escrever_u32(GuestVirt(regs[4]),0x11000).unwrap();for(i,arg)in args.iter().enumerate(){s.bus.escrever_u32(GuestVirt(regs[4]+4+i as u32*4),*arg).unwrap();}s.kernel.atender(&mut s.bus,imports::trampoline(ordinal),&regs).unwrap();}
  assert_eq!(word(&s,s.state.regs[4]),RETURN_SENTINEL);s.state.eflags &= !INTERRUPT_FLAG;
  for(at,value)in [(0xFD600140,1),(0xFD000140,1)]{s.bus.escrever_u32(GuestVirt(at),value).unwrap();}
  if pending{s.bus.nv2a_irq_mut().raise_vblank();}s
 }
 fn finish(s:&mut Session,run:Run,expected:u32){
  let mut entered=0;s.conduzir(Limites{blocos_por_fatia:1000,fatias:10},|state,bus,budget|{if state.eip==0x11100{entered+=1;assert_eq!(state.eflags&INTERRUPT_FLAG,0);}run(state,bus,budget)},|_|Ok(false)).unwrap();assert_eq!(entered,1);assert_eq!(word(s,0x12104),expected);assert_eq!(word(s,0x12100),3);assert_eq!(s.kernel.irql(),Irql::Passive);
 }
 #[test]
 fn sti_protege_uma_instrucao_e_replay_retoma_dentro_do_mesmo_bloco(){
  let image=include_bytes!("irq_sti.xbe");let mut s=setup(image,true);
  assert!(matches!(s.conduzir(Limites{blocos_por_fatia:1000,fatias:1},sti::run,|_|Ok(false)),Err(Error::BudgetExhausted{..})));assert_eq!(word(&s,0x12100),2);assert_eq!(word(&s,0x12104),0);assert_eq!(s.state.sti_shadow,0);
  let checkpoint=saved(&s);finish(&mut s,sti::run,2);let complete=saved(&s);let mut r=setup(image,true);r.load(&mut SnapshotReader::new(&checkpoint).unwrap()).unwrap();finish(&mut r,sti::run,2);assert!(saved(&r)==complete);
 }
 #[test]
 fn cli_e_sti_cli_mantem_irq_pendente_sem_executar_isr(){
  for(image,run)in [(include_bytes!("irq_cli.xbe").as_slice(),cli::run as Run),(include_bytes!("irq_sti_cli.xbe").as_slice(),negated::run as Run)]{let mut s=setup(image,true);assert_eq!(s.conduzir(Limites{blocos_por_fatia:1000,fatias:10},run,|_|Ok(false)).unwrap(),Desfecho::Retornou);assert_eq!(word(&s,0x12104),0);assert!(s.bus.gpu_irq_line_asserted());}
 }
 #[test]
 fn segundo_sti_com_if_ligada_nao_estende_shadow(){let mut s=setup(include_bytes!("irq_sti_twice.xbe"),true);finish(&mut s,twice::run,1);}
 #[test]
 fn primeira_iteracao_rep_apos_sti_libera_irq_com_ecx_ainda_pendente(){
  let image=include_bytes!("irq_sti_rep.xbe");let mut s=setup(image,true);s.state.regs[1]=3;s.state.regs[7]=0x12120;s.state.regs[0]=0x44;
  assert!(matches!(s.conduzir(Limites{blocos_por_fatia:1000,fatias:1},rep::run,|_|Ok(false)),Err(Error::BudgetExhausted{..})));assert_eq!(s.state.regs[1],2);assert_eq!(s.state.sti_shadow,0);let mut first=[0;3];s.bus.ler_bytes(GuestVirt(0x12120),&mut first).unwrap();assert_eq!(first,[0x44,0,0]);let checkpoint=saved(&s);
  finish(&mut s,rep::run,1);assert_eq!(word(&s,0x12108),2,"ISR interrompeu após uma de três iterações");let mut out=[0;3];s.bus.ler_bytes(GuestVirt(0x12120),&mut out).unwrap();assert_eq!(out,[0x44;3]);let complete=saved(&s);
  let mut r=setup(image,true);r.load(&mut SnapshotReader::new(&checkpoint).unwrap()).unwrap();finish(&mut r,rep::run,1);assert!(saved(&r)==complete);
 }
 #[test]
 fn dirql_igual_mascara_irq_e_lower_passive_entrega_sem_perder_linha(){
  let mut s=setup(include_bytes!("irq_sti_loop.xbe"),true);s.kernel.restore_irql(Irql::Device24);let limits=Limites{blocos_por_fatia:10,fatias:1};assert!(matches!(s.conduzir(limits,forever::run,|_|Ok(false)),Err(Error::BudgetExhausted{..})));assert_eq!(word(&s,0x12104),0);assert!(s.bus.gpu_irq_line_asserted());s.kernel.restore_irql(Irql::Passive);assert!(matches!(s.conduzir(limits,forever::run,|_|Ok(false)),Err(Error::BudgetExhausted{..})));assert_eq!(word(&s,0x12104),3);assert_eq!(s.kernel.irql(),Irql::Passive);
 }
 #[test]
 fn vblank_preempta_loop_sem_kernel_antes_do_orcamento_total(){
  let mut s=setup(include_bytes!("irq_sti_loop.xbe"),false);let period=Clock::frame_cycles(xbox_core::time::VBLANK_NTSC_MHZ);s.state.clock.advance(period-20);s.publicar_tick_count().unwrap();let result=s.conduzir(Limites{blocos_por_fatia:1000,fatias:2},forever::run,|_|Ok(false));assert!(matches!(result,Err(Error::BudgetExhausted{..})));assert_eq!(word(&s,0x12104),3);assert_eq!(s.vblanks(),1);assert!(s.state.clock.cycles()<period+1100);
 }
 #[test]
 fn perfil_legado_entrega_na_fronteira_mesmo_com_if_desligada(){let mut s=setup(include_bytes!("irq_cli.xbe"),true);s.state.interrupt_profile=InterruptProfile::LegacyBoundary;assert_eq!(s.conduzir(Limites{blocos_por_fatia:1000,fatias:10},cli::run,|_|Ok(false)).unwrap(),Desfecho::Retornou);assert!(!s.bus.gpu_irq_line_asserted());assert_eq!(word(&s,0x12104),0);}
}
