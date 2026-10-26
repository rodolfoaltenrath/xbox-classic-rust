#[cfg(test)]
mod gamma_verification {
    use xbox_core::{Snapshot,SnapshotReader,SnapshotWriter};
    use xbox_mem::{GuestVirt,nv2a_gamma::GammaProfile};
    use xbox_recomp::runtime::{Session,Exit};
    mod guest { include!("kernel_gamma.rs"); }
    fn loaded()->Session { Session::load(include_bytes!("kernel_gamma.xbe")).unwrap() }
    fn saved(s:&Session)->Vec<u8> { let mut w=SnapshotWriter::new();s.save(&mut w);w.finish() }
    fn finish(s:&mut Session) {
        loop {match guest::run(&mut s.state,&mut s.bus,100).unwrap() {
            Exit::KernelCall{..}=>{s.atender_kernel().unwrap();}, Exit::Returned=>break, other=>panic!("{other:?}")
        }}
    }
    fn output(s:&Session) {
        let before=saved(s);
        let raw=super::capturar_quadro(s).unwrap(); let out=super::capturar_saida(s).unwrap();
        assert_eq!(raw.pixel(0,0),Some([0xCC,0x33,0,0xFF]));
        assert_eq!(raw.pixel(0,1),Some([0,0x55,0xAA,0xFF]));
        assert_eq!(out.pixel(0,0),Some([0x99,0x19,0xFF,0xFF]));
        assert_eq!(out.pixel(0,1),Some([0x55,0x2A,0x55,0xFF]));
        for (a,b) in raw.bytes().chunks_exact(4).zip(out.bytes().chunks_exact(4)) { assert_eq!(a[3],b[3]); }
        assert!(saved(s)==before,"captura alterou RAM/CPU/DAC/kernel");
    }
    #[test]
    fn codigo_nativo_programa_dac_e_saida_muda_sem_alterar_ram_ou_alfa() {
        let mut s=loaded(); finish(&mut s);output(&s);assert_eq!(s.kernel.atendidas(),3);
    }
    #[test]
    fn replay_nativo_entre_componentes_reproduz_estado_integral_e_saida() {
        let mut s=loaded();
        while s.bus.nv2a_gamma().cursor()!=1 {
            match guest::run(&mut s.state,&mut s.bus,1).unwrap() {
                Exit::KernelCall{..}=>{s.atender_kernel().unwrap();},Exit::Budget=>{},e=>panic!("{e:?}")
            }
        }
        let checkpoint=saved(&s);finish(&mut s);let end=saved(&s);output(&s);
        let mut restored=loaded();Snapshot::load(&mut restored,&mut SnapshotReader::new(&checkpoint).unwrap()).unwrap();
        assert!(saved(&restored)==checkpoint);finish(&mut restored);assert!(saved(&restored)==end);output(&restored);
    }
    #[test]
    fn sessao_bus_v9_restaura_sem_gamma_e_mantem_saida_bruta_apos_novas_escritas() {
        let s=loaded();let mut legacy=saved(&s);super::boot_dma_verification::strip_gamma(&mut legacy);
        let bus=legacy.windows(4).position(|v|v==b"BUS0").unwrap();legacy[bus+4..bus+8].copy_from_slice(&9u32.to_le_bytes());
        let mut restored=loaded();Snapshot::load(&mut restored,&mut SnapshotReader::new(&legacy).unwrap()).unwrap();
        finish(&mut restored);assert_eq!(restored.bus.nv2a_gamma().profile(),GammaProfile::LegacyUnmodeled);
        assert_eq!(super::capturar_saida(&restored),super::capturar_quadro(&restored));
        let old=saved(&restored);restored.bus.escrever_u8(GuestVirt(0xFD6813C8),7).unwrap();
        Snapshot::load(&mut restored,&mut SnapshotReader::new(&old).unwrap()).unwrap();
        assert!(saved(&restored)==old);assert_eq!(super::capturar_saida(&restored),super::capturar_quadro(&restored));
    }
}
