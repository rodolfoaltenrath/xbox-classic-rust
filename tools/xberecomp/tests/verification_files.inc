#[cfg(test)]
mod files_verification {
 use xbox_core::{Snapshot,SnapshotReader,SnapshotWriter};
 use xbox_mem::{GuestVirt,Acesso};
 use xbox_recomp::{runtime::{Session,Exit,ESP},checkpoint};
 mod guest {include!("kernel_files.rs");}
 const IMAGE:&[u8]=include_bytes!("kernel_files.xbe");
 fn loaded()->Session {Session::load(IMAGE).unwrap()}
 fn saved(s:&Session)->Vec<u8>{let mut w=SnapshotWriter::new();s.save(&mut w);w.finish()}
 fn pending(s:&mut Session,ordinal:u32){assert_eq!(guest::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::KernelCall{ordinal});}
 fn attend(s:&mut Session){let call=s.kernel_call().unwrap();let before=s.state.clone();s.atender_kernel().unwrap();let mut expected=before;expected.eip=call.return_pc;expected.regs[ESP]=call.return_esp;expected.regs[0]=0;assert_eq!(s.state,expected);}
 fn finish(s:&mut Session){for ordinal in [211,219,187]{pending(s,ordinal);attend(s);}assert_eq!(guest::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::Returned);assert_eq!(s.state.regs[0],123);}
 fn opened(s:&mut Session){pending(s,190);attend(s);pending(s,219);attend(s);}
 #[test]
 fn arquivo_nativo_le_xbe_original_em_vez_de_headers_modificados_na_ram(){
  let mut s=loaded();let esp=s.state.regs[ESP];s.state.eflags=0xED7;
  assert_eq!(s.kernel.executable_media(),Some(IMAGE));
  let pa=s.bus.traduzir(GuestVirt(0x10000),Acesso::Leitura).unwrap();s.bus.escrever_fisico(pa,b"RAM!").unwrap();
  opened(&mut s);assert_eq!(s.bus.ler_u32(GuestVirt(0x12400)).unwrap(),u32::from_le_bytes(*b"XBEH"));
  finish(&mut s);assert_eq!(s.state.regs[ESP],esp+4);assert_eq!(s.state.eflags,0xED7);assert_eq!(s.kernel.atendidas(),5);
  let mut bytes=[0;8];s.bus.ler_bytes(GuestVirt(0x12408),&mut bytes).unwrap();assert_eq!(&bytes,&IMAGE[4..12]);
  assert_eq!(s.bus.ler_u32(GuestVirt(0x12468)).unwrap(),IMAGE.len() as u32);
 }
 #[test]
 fn estado_com_handle_aberto_reproduz_cursor_midia_e_continuacao_sem_io_host(){
  let mut s=loaded();opened(&mut s);let raw=saved(&s);let file=checkpoint::encode(&s,IMAGE).unwrap();
  finish(&mut s);let expected=saved(&s);let mut restored=checkpoint::decode(&file,IMAGE).unwrap();assert!(saved(&restored)==raw);
  finish(&mut restored);assert!(saved(&restored)==expected);
 }
 #[test]
 fn recusa_preserva_cpu_e_krnl_v13_restaura_sem_midia_ou_promocao(){
  let mut s=loaded();pending(&mut s,190);attend(&mut s);pending(&mut s,219);
  let esp=s.state.regs[ESP];s.bus.escrever_u32(GuestVirt(esp+28),0).unwrap();let before=s.state.clone();
  assert!(s.atender_kernel().is_err());assert_eq!(s.state,before);assert_eq!(s.bus.ler_u32(GuestVirt(0x12400)).unwrap(),0);
  let mut old=saved(&loaded());super::boot_dma_verification::strip_file_media(&mut old);let at=old.windows(4).position(|v|v==b"KRNL").unwrap();old[at+4..at+8].copy_from_slice(&13u32.to_le_bytes());
  Snapshot::load(&mut s,&mut SnapshotReader::new(&old).unwrap()).unwrap();assert!(s.kernel.executable_media().is_none());
  pending(&mut s,190);let cpu=s.state.clone();assert!(s.atender_kernel().is_err());assert_eq!(s.state,cpu);
  let again=checkpoint::encode(&s,IMAGE).unwrap();assert!(checkpoint::decode(&again,IMAGE).unwrap().kernel.executable_media().is_none());
 }
}
