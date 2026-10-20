
#[cfg(test)]
mod sha_verification {
    use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
    use xbox_mem::GuestVirt;
    use xbox_recomp::runtime::{Session, Exit, ESP};
    mod sha { include!("kernel_sha.rs"); }

    fn loaded() -> Session {
        let mut s = Session::load(include_bytes!("kernel_sha.xbe")).unwrap();
        s.bus.escrever_bytes(GuestVirt(0x12200), b"abc").unwrap();
        s.bus.escrever_bytes(GuestVirt(0x122FF), &[0xAA; 22]).unwrap();
        s
    }

    fn boundary(s: &mut Session, ordinal: u32) {
        assert_eq!(sha::run(&mut s.state, &mut s.bus, 100).unwrap(), Exit::KernelCall { ordinal });
    }

    fn saved(s: &Session) -> Vec<u8> {
        let mut out = SnapshotWriter::new(); s.save(&mut out); out.finish()
    }

    fn finish(s: &mut Session) {
        s.atender_kernel().unwrap(); // Update dos dois bytes restantes, já pendente.
        boundary(s, 337); s.atender_kernel().unwrap();
        assert_eq!(sha::run(&mut s.state, &mut s.bus, 100).unwrap(), Exit::Returned);
    }

    #[test]
    fn sha_gerado_passa_pela_abi_e_restaura_contexto_parcial_na_sessao() {
        let mut s = loaded(); let initial_esp = s.state.regs[ESP];
        boundary(&mut s, 335); s.atender_kernel().unwrap();
        assert_eq!(s.state.regs[ESP], initial_esp);
        boundary(&mut s, 336); s.atender_kernel().unwrap();
        boundary(&mut s, 336);
        let checkpoint = saved(&s);
        finish(&mut s);
        let complete = saved(&s);
        let mut digest = [0; 20]; s.bus.ler_bytes(GuestVirt(0x12300), &mut digest).unwrap();
        assert_eq!(digest, [0xA9,0x99,0x3E,0x36,0x47,0x06,0x81,0x6A,0xBA,0x3E,0x25,0x71,0x78,0x50,0xC2,0x6C,0x9C,0xD0,0xD8,0x9D]);
        assert_eq!(s.bus.ler_u8(GuestVirt(0x122FF)).unwrap(), 0xAA);
        assert_eq!(s.bus.ler_u8(GuestVirt(0x12314)).unwrap(), 0xAA);
        assert_eq!(s.kernel.atendidas(), 4);
        assert_eq!(s.state.regs[ESP], initial_esp + 4); // RET final.
        let mut restored = loaded();
        let mut inp = SnapshotReader::new(&checkpoint).unwrap();
        Snapshot::load(&mut restored, &mut inp).unwrap();
        assert_eq!(inp.remaining(), 0);
        finish(&mut restored);
        assert_eq!(saved(&restored), complete);
    }

    #[test]
    fn falha_sha_preserva_cpu_chamada_pendente_e_memoria() {
        let mut s = loaded(); boundary(&mut s, 335);
        s.bus.escrever_u32(GuestVirt(s.state.regs[ESP]+4), 0x12FDF).unwrap();
        s.bus.escrever_bytes(GuestVirt(0x12FDF), &[0xAA; 33]).unwrap();
        let before = s.state.clone();
        assert!(s.atender_kernel().is_err());
        assert_eq!(s.state, before);
        let mut guard = [0; 33]; s.bus.ler_bytes(GuestVirt(0x12FDF), &mut guard).unwrap();
        assert_eq!(guard, [0xAA;33]);
        assert_eq!(s.kernel.atendidas(), 0);
        // Corrigir a saída permite atender a mesma chamada e continuar.
        s.bus.escrever_u32(GuestVirt(s.state.regs[ESP]+4), 0x12100).unwrap();
        s.atender_kernel().unwrap();
        boundary(&mut s, 336);
    }

    #[test]
    fn contexto_sha_copiado_por_guest_funciona_sem_registro_host() {
        let mut s = loaded(); boundary(&mut s, 335); s.atender_kernel().unwrap();
        boundary(&mut s, 336); s.atender_kernel().unwrap();
        let mut context = [0;116]; s.bus.ler_bytes(GuestVirt(0x12100), &mut context).unwrap();
        s.bus.escrever_bytes(GuestVirt(0x12400), &context).unwrap();
        boundary(&mut s, 336);
        s.bus.escrever_u32(GuestVirt(s.state.regs[ESP]+4), 0x12400).unwrap();
        s.atender_kernel().unwrap(); boundary(&mut s, 337);
        s.bus.escrever_u32(GuestVirt(s.state.regs[ESP]+4), 0x12400).unwrap();
        s.atender_kernel().unwrap();
        assert_eq!(s.bus.ler_u32(GuestVirt(0x12300)).unwrap(), 0x363E99A9);
        let mut original = [0;116]; s.bus.ler_bytes(GuestVirt(0x12100), &mut original).unwrap();
        assert_eq!(original, context);
        assert_eq!(sha::run(&mut s.state,&mut s.bus,100).unwrap(),Exit::Returned);
    }
}
