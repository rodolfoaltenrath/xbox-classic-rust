#[cfg(test)]
mod directories_verification {
 use xbox_core::{Snapshot,SnapshotWriter};
 use xbox_mem::GuestVirt;
 use xbox_recomp::{runtime::{Session,Exit,ESP},checkpoint};
 mod guest {include!("kernel_directories.rs");}
 const IMAGE:&[u8]=include_bytes!("kernel_directories.xbe");
 fn loaded()->Session{Session::load(IMAGE).unwrap()}
 fn saved(s:&Session)->Vec<u8>{let mut w=SnapshotWriter::new();s.save(&mut w);w.finish()}
 fn pending(s:&mut Session,n:u32){assert_eq!(guest::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::KernelCall{ordinal:n});}
 fn attend(s:&mut Session,status:u32){let call=s.kernel_call().unwrap();let mut expected=s.state.clone();expected.eip=call.return_pc;expected.regs[ESP]=call.return_esp;expected.regs[0]=status;s.atender_kernel().unwrap();assert_eq!(s.state,expected);}
 fn finish(s:&mut Session){for status in [0,0x80000006,0]{pending(s,207);attend(s,status);}pending(s,187);attend(s,0);assert_eq!(guest::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::Returned);assert_eq!(s.state.regs[0],123);}
 #[test]
 fn diretorio_emitido_respeita_abi_nome_ansi_fim_restart_e_replay_integral(){
  let mut s=loaded();let esp=s.state.regs[ESP];s.state.eflags=0xed7;pending(&mut s,202);attend(&mut s,0);
  let file=checkpoint::encode(&s,IMAGE).unwrap();finish(&mut s);let expected=saved(&s);
  assert_eq!(s.state.regs[ESP],esp+4);assert_eq!(s.state.eflags,0xed7);assert_eq!(s.bus.ler_u32(GuestVirt(0x12428)).unwrap(),IMAGE.len()as u32);
  let mut name=[0;11];s.bus.ler_bytes(GuestVirt(0x12440),&mut name).unwrap();assert_eq!(&name,b"default.xbe");
  let mut resumed=checkpoint::decode(&file,IMAGE).unwrap();finish(&mut resumed);assert!(saved(&resumed)==expected);
 }
 #[test]
 fn diretorio_emitido_recusa_variante_sem_consumir_cpu_cursor_ou_buffer(){
  let mut s=loaded();pending(&mut s,202);attend(&mut s,0);pending(&mut s,207);
  let before=s.state.clone();s.bus.escrever_u32(GuestVirt(s.state.regs[ESP]+32),2).unwrap();
  assert!(s.atender_kernel().is_err());assert_eq!(s.state,before);assert_eq!(s.bus.ler_u32(GuestVirt(0x12400)).unwrap(),0);
  s.bus.escrever_u32(GuestVirt(s.state.regs[ESP]+32),1).unwrap();attend(&mut s,0);assert_eq!(s.bus.ler_u32(GuestVirt(0x1243c)).unwrap(),11);
 }
}
