// Fixture de migração: mantém CPU/Bus/kernel e substitui apenas a política
// nova por LegacyBootstrap canônico, sem promover snapshots antigos.
#[cfg(test)]
fn legacy_scheduler_snapshot(bytes:&[u8])->Vec<u8>{
 let at=bytes.windows(4).rposition(|v|v==b"SCHD").expect("SCHD");let len=u64::from_le_bytes(bytes[at+12..at+20].try_into().unwrap()) as usize;assert_eq!(at+20+len,bytes.len());let mut out=bytes[..at+8].to_vec();out.extend(0u32.to_le_bytes());out.extend(0u64.to_le_bytes());out
}
#[cfg(test)]
mod scheduler_verification {
 use xbox_core::{Snapshot,SnapshotReader,SnapshotWriter,Clock};
 use xbox_mem::GuestVirt;
 use xbox_recomp::runtime::{Session,Exit,Limites,Desfecho,SchedulingProfile};
 mod sleep{include!("scheduler_sleep.rs");}
 mod deadlines{include!("scheduler_deadlines.rs");}
 mod waits{include!("scheduler_waits.rs");}
 mod detach{include!("scheduler_detach.rs");}
 mod quantum{include!("scheduler_quantum.rs");}
 fn loaded()->Session{Session::load(include_bytes!("scheduler_sleep.xbe")).unwrap()}
 fn word(s:&Session,at:u32)->u32{s.bus.ler_u32(GuestVirt(at)).unwrap()}
 fn saved(s:&Session)->Vec<u8>{let mut w=SnapshotWriter::new();s.save(&mut w);w.finish()}
 fn start(s:&mut Session,run:fn(&mut xbox_recomp::runtime::State,&mut xbox_mem::Bus,u64)->xbox_recomp::Result<Exit>)->u64 {
  assert_eq!(run(&mut s.state,&mut s.bus,1000).unwrap(),Exit::KernelCall{ordinal:255});s.atender_kernel().unwrap();assert_eq!(s.kernel.current_thread(),2);
  assert_eq!(run(&mut s.state,&mut s.bus,1000).unwrap(),Exit::KernelCall{ordinal:99});let before=s.state.clock.cycles();s.atender_kernel().unwrap();assert_eq!(s.state.clock.cycles(),before);assert_eq!(s.kernel.current_thread(),1);assert_eq!(word(s,0x12100),1);before
 }
 fn finish(s:&mut Session,run:fn(&mut xbox_recomp::runtime::State,&mut xbox_mem::Bus,u64)->xbox_recomp::Result<Exit>){assert_eq!(s.conduzir(Limites{blocos_por_fatia:10000,fatias:100},run,|_|Ok(false)).unwrap(),Desfecho::Retornou);}
 fn context_results(s:&Session){assert_eq!(word(s,0x12100),2);assert_eq!(word(s,0x12104),1);assert_eq!(word(s,0x12108),0xBBBB);assert_eq!(word(s,0x12118),0xAAAA);assert_eq!(word(s,0x1210C),3.5f32.to_bits());assert_eq!(word(s,0x12120),2.5f32.to_bits());assert_eq!(word(s,0x12110),1f32.to_bits());assert_eq!(word(s,0x12124),0);}
 #[test]
 fn delay_bloqueia_filha_sem_pular_tempo_enquanto_pai_executavel(){
  let mut s=loaded();assert_eq!(s.scheduling_profile(),SchedulingProfile::TwoThreadV1);let before=start(&mut s,sleep::run);finish(&mut s,sleep::run);context_results(&s);assert!(s.state.clock.cycles()>=before+Clock::hundred_ns_to_cycles(20000).unwrap());
 }
 #[test]
 fn snapshot_com_filha_dormindo_reproduz_contexto_fpu_sse_e_clock_global(){
  let mut s=loaded();start(&mut s,sleep::run);let checkpoint=saved(&s);finish(&mut s,sleep::run);let complete=saved(&s);let mut r=loaded();r.load(&mut SnapshotReader::new(&checkpoint).unwrap()).unwrap();assert!(saved(&r)==checkpoint);finish(&mut r,sleep::run);context_results(&r);assert!(saved(&r)==complete);
 }
 #[test]
 fn duas_threads_dormindo_escolhem_primeiro_deadline_e_preservam_segundo(){
  let mut s=Session::load(include_bytes!("scheduler_deadlines.xbe")).unwrap();let before=start(&mut s,deadlines::run);assert_eq!(deadlines::run(&mut s.state,&mut s.bus,1000).unwrap(),Exit::KernelCall{ordinal:99});let parent_before=s.state.clock.cycles();s.atender_kernel().unwrap();assert_eq!(s.kernel.current_thread(),2);assert_eq!(s.state.clock.cycles(),before+Clock::hundred_ns_to_cycles(20000).unwrap());finish(&mut s,deadlines::run);context_results(&s);assert!(s.state.clock.cycles()>=parent_before+Clock::hundred_ns_to_cycles(40000).unwrap());
 }
 #[test]
 fn quantum_deixa_pai_correr_durante_loop_sem_chamadas_da_filha(){
  let mut s=Session::load(include_bytes!("scheduler_quantum.xbe")).unwrap();finish(&mut s,quantum::run);assert_eq!(word(&s,0x12100),10000);let observed=word(&s,0x12128);assert!(observed>0&&observed<10000,"observed={observed}");assert_eq!(word(&s,0x12104),1);
 }
 #[test]
 fn pai_fecha_handle_e_retorna_mas_filha_dorme_e_continua_com_replay(){
  let image=include_bytes!("scheduler_detach.xbe");let mut s=Session::load(image).unwrap();start(&mut s,detach::run);
  assert_eq!(detach::run(&mut s.state,&mut s.bus,1000).unwrap(),Exit::KernelCall{ordinal:187});s.atender_kernel().unwrap();assert_eq!(s.state.regs[0],0);
  let checkpoint=saved(&s);finish(&mut s,detach::run);context_results(&s);assert_eq!(word(&s,0x1212C),0);let complete=saved(&s);
  let mut r=Session::load(image).unwrap();r.load(&mut SnapshotReader::new(&checkpoint).unwrap()).unwrap();finish(&mut r,detach::run);assert!(saved(&r)==complete);
 }

 #[test]
 fn tctx_v1_migra_shadow_zero_e_v2_recusa_shadow_impossivel(){
  let mut s=loaded();start(&mut s,sleep::run);let current=saved(&s);let at=current.windows(4).rposition(|v|v==b"TCTX").unwrap();let shadow=at+8+54+128+4+104;assert_eq!(current[shadow],0);
  let mut old=current.clone();old.remove(shadow);old[at+4..at+8].copy_from_slice(&1u32.to_le_bytes());let sch=old.windows(4).rposition(|v|v==b"SCHD").unwrap();let len=u64::from_le_bytes(old[sch+12..sch+20].try_into().unwrap());old[sch+12..sch+20].copy_from_slice(&(len-1).to_le_bytes());let mut r=loaded();r.load(&mut SnapshotReader::new(&old).unwrap()).unwrap();assert!(saved(&r)==current);finish(&mut s,sleep::run);finish(&mut r,sleep::run);assert!(saved(&r)==saved(&s));
  for value in [2,255]{let mut bad=current.clone();bad[shadow]=value;assert!(loaded().load(&mut SnapshotReader::new(&bad).unwrap()).is_err());}
  let mut bad=current.clone();bad[shadow]=1;bad[at+52..at+56].copy_from_slice(&2u32.to_le_bytes());assert!(loaded().load(&mut SnapshotReader::new(&bad).unwrap()).is_err());
 }

 #[test]
 fn segunda_chamada_wait_pendente_recusa_sem_sobrescrever_evento_ou_continuacao(){
  let mut s=Session::load(include_bytes!("scheduler_waits.xbe")).unwrap();let result=s.conduzir(Limites{blocos_por_fatia:10000,fatias:100},waits::run,|_|Ok(false));let error=result.unwrap_err();assert!(matches!(error,xbox_recomp::Error::Kernel(xbox_kernel::dispatch::Erro::VarianteNaoImplementada{ordinal:233,..})));assert!(error.to_string().contains("uma única chamada NtWait"));assert_eq!(word(&s,0x12100),1);let checkpoint=saved(&s);assert!(s.atender_kernel().is_err());assert!(saved(&s)==checkpoint);
  let mut r=Session::load(include_bytes!("scheduler_waits.xbe")).unwrap();r.load(&mut SnapshotReader::new(&checkpoint).unwrap()).unwrap();assert!(r.atender_kernel().is_err());assert!(saved(&r)==checkpoint);
 }

}
