
#[cfg(test)]
mod blocks_verification {
    use xbox_recomp::runtime::{Session,Exit};
    type Run=fn(&mut xbox_recomp::runtime::State,&mut xbox_mem::Bus,u64)->xbox_recomp::Result<Exit>;
    mod blocks_straight { include!("blocks_straight.rs"); }
    mod blocks_many { include!("blocks_many.rs"); }
    #[test]
    fn milhares_de_blocos_e_sequencia_longa_rodam_em_pilha_host_pequena() {
        for (run,image,retired) in [
            (blocks_straight::run as Run,include_bytes!("blocks_straight.xbe").as_slice(),1001),
            (blocks_many::run as Run,include_bytes!("blocks_many.xbe").as_slice(),2001),
        ] {
            let mut s=Session::load(image).unwrap(); s.state.regs[1]=1;
            let state=std::thread::Builder::new().stack_size(128*1024).spawn(move || {
                assert_eq!(run(&mut s.state,&mut s.bus,1).unwrap(),Exit::Budget);
                assert_eq!(run(&mut s.state,&mut s.bus,2048).unwrap(),Exit::Returned);
                s.state
            }).unwrap().join().unwrap();
            assert_eq!(state.regs[0],1000); assert_eq!(state.retired,retired);
            assert_eq!(state.clock.tsc(),retired);
        }
    }
}
