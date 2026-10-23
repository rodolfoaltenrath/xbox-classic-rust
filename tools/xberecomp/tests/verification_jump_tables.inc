#[cfg(test)]
mod jump_table_verification {
 use xbox_recomp::{Error,runtime::{Session,Exit}};
 use xbox_core::{Snapshot,SnapshotReader,SnapshotWriter};
 use xbox_mem::GuestVirt;
 mod table {include!("jump_table_guest.rs");}
 mod callback {include!("callback_flow_guest.rs");}
 fn loaded()->Session{Session::load(include_bytes!("jump_table_guest.xbe")).unwrap()}
 fn saved(s:&Session)->Vec<u8>{let mut w=SnapshotWriter::new();s.save(&mut w);w.finish()}
 #[test]
 fn guarda_e_tabela_readonly_executam_cases_e_default_sem_raizes(){
  for (index,expected) in [(0,11),(1,22),(2,99),(u32::MAX,99)] {let mut s=loaded();s.state.regs[0]=index;assert_eq!(table::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::Returned);assert_eq!(s.state.regs[0],expected);}
 }
 #[test]
 fn snapshot_das_flags_da_guarda_reproduz_destino_e_estado_integral(){
  let mut s=loaded();s.state.regs[0]=1;assert_eq!(table::run(&mut s.state,&mut s.bus,1).unwrap(),Exit::Budget);let checkpoint=saved(&s);assert_eq!(table::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::Returned);let complete=saved(&s);
  let mut restored=loaded();restored.load(&mut SnapshotReader::new(&checkpoint).unwrap()).unwrap();assert!(saved(&restored)==checkpoint);assert_eq!(table::run(&mut restored.state,&mut restored.bus,100).unwrap(),Exit::Returned);assert!(saved(&restored)==complete);
 }
 #[test]
 fn callback_escapado_le_ram_atual_e_recusa_alvo_nao_traduzido(){
  let mut initial=Session::load(include_bytes!("callback_flow_guest.xbe")).unwrap();assert_eq!(callback::run(&mut initial.state,&mut initial.bus,4).unwrap(),Exit::Budget);let checkpoint=saved(&initial);
  for (target,expected) in [(0x11040,11),(0x11050,22)] {let mut s=Session::load(include_bytes!("callback_flow_guest.xbe")).unwrap();s.load(&mut SnapshotReader::new(&checkpoint).unwrap()).unwrap();s.bus.escrever_u32(GuestVirt(0x12020),target).unwrap();assert_eq!(callback::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::Returned);assert_eq!(s.state.regs[0],expected);}
  initial.bus.escrever_u32(GuestVirt(0x12020),0x11060).unwrap();let cpu=initial.state.clone();assert!(matches!(callback::run(&mut initial.state,&mut initial.bus,100),Err(Error::UnknownTarget{target:0x11060,..})));assert_eq!(initial.state,cpu);
 }
}
