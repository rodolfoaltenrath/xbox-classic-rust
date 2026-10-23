use xbox_core::{SnapshotReader, SnapshotWriter};
use xbox_gpu::pgraph::{Memory, Nv2aPgraph, PGRAPH_ERROR, PgraphProfile};
#[derive(Clone)]
struct Ram {
    data: Vec<u32>,
    instance: Vec<u32>,
}
impl Memory for Ram {
    fn ram_len(&self) -> u64 {
        self.data.len() as u64 * 4
    }
    fn read_instance(&self, at: u32) -> Result<u32, &'static str> {
        self.instance
            .get(at as usize / 4)
            .copied()
            .ok_or("instance")
    }
    fn read_ram(&self, at: u32) -> Result<u32, &'static str> {
        self.data.get(at as usize / 4).copied().ok_or("read")
    }
    fn write_ram(&mut self, at: u32, v: u32) -> Result<(), &'static str> {
        *self.data.get_mut(at as usize / 4).ok_or("write")? = v;
        Ok(())
    }
}
fn command(g: &mut Nv2aPgraph, m: &mut Ram, method: u16, v: u32) -> Result<(), &'static str> {
    g.execute(0x97, 2, method, v, v, m)
}
fn fixture() -> (Nv2aPgraph, Ram) {
    let mut g = Nv2aPgraph::default();
    let mut m = Ram {
        data: vec![0x11223344; 128],
        instance: vec![0; 16],
    };
    m.instance[..3].copy_from_slice(&[3, 511, 0]);
    m.instance[4..7].copy_from_slice(&[3, 511, 0]);
    for (method, v) in [
        (0x194, 0),
        (0x198, 16),
        (0x200, 4 << 16),
        (0x204, 3 << 16),
        (0x208, 0x128),
        (0x20C, 0x18 | (0x18 << 16)),
        (0x210, 16),
        (0x214, 256),
        (0x1D8C, 0x55667788),
        (0x1D90, 0xAABBCCDD),
        (0x1D98, 1 | (2 << 16)),
        (0x1D9C, 1 | (2 << 16)),
    ] {
        command(&mut g, &mut m, method, v).unwrap();
    }
    (g, m)
}
fn saved(g: &Nv2aPgraph) -> Vec<u8> {
    let mut w = SnapshotWriter::new();
    g.save(&mut w);
    w.finish()
}
#[test]
fn clear_retangulo_inclusivo_preserva_vizinhos_padding_e_mascaras_de_cor() {
    let (mut g, mut m) = fixture();
    command(&mut g, &mut m, 0x1D94, 0x50).unwrap();
    for i in 0..m.data.len() {
        let cleared = [11, 12, 17, 18].contains(&i);
        assert_eq!(
            m.data[i],
            if cleared { 0x11BB33DD } else { 0x11223344 },
            "word {i}"
        );
    }
    assert_eq!(g.stats().calls, 1);
    assert_eq!(g.stats().pixels, 4);
    assert_eq!(g.stats().first_color, Some(0xAABBCCDD));
}
#[test]
fn clear_zeta_stencil_e_cor_validam_ambos_destinos_antes_de_gravar() {
    let (mut g, mut m) = fixture();
    let before = m.data.clone();
    m.instance[5] = 257;
    assert!(command(&mut g, &mut m, 0x1D94, 0xF3).is_err());
    assert_eq!(m.data, before);
    assert_eq!(g.stats().calls, 0);
    m.instance[5] = 511;
    command(&mut g, &mut m, 0x1D94, 1).unwrap();
    assert_eq!(m.data[71], 0x55667744);
    command(&mut g, &mut m, 0x1D94, 2).unwrap();
    assert_eq!(m.data[71], 0x55667788);
    command(&mut g, &mut m, 0x1D94, 0xF0).unwrap();
    assert_eq!(m.data[11], 0xAABBCCDD);
}
#[test]
fn clear_recusa_formato_pitch_limite_e_flags_sem_escrita() {
    for (method, value) in [(0x208, 0x129), (0x20C, 15), (0x210, 3)] {
        let (mut g, mut m) = fixture();
        command(&mut g, &mut m, method, value).unwrap();
        let before = m.data.clone();
        assert!(command(&mut g, &mut m, 0x1D94, 0xF0).is_err());
        assert_eq!(m.data, before);
    }
    let (mut g, mut m) = fixture();
    let before = m.data.clone();
    assert!(command(&mut g, &mut m, 0x1D94, 4).is_err());
    m.instance[1] = 30;
    assert!(command(&mut g, &mut m, 0x1D94, 0xF0).is_err());
    assert_eq!(m.data, before);
}
#[test]
fn trap_generico_persiste_e_ack_w1c_libera_fifo_sem_interpretar_codigo_guest() {
    let (mut g, mut m) = fixture();
    g.write(0x400720, 1);
    assert!(g.ready());
    command(&mut g, &mut m, 0x100, 0x12345678).unwrap();
    assert!(!g.ready());
    assert!(!g.irq_pending());
    g.write(0x400140, PGRAPH_ERROR);
    assert!(g.irq_pending());
    assert_eq!(g.read(0x400704, 4).unwrap(), Some(0x20100));
    assert_eq!(g.read(0x400708, 4).unwrap(), Some(0x12345678));
    assert_eq!(g.read(0x401A88, 4).unwrap(), Some(0x55667788));
    let bytes = saved(&g);
    let mut restored = Nv2aPgraph::default();
    restored
        .load(&mut SnapshotReader::new(&bytes).unwrap())
        .unwrap();
    assert_eq!(saved(&restored), bytes);
    restored.write(0x400100, 0);
    assert!(!restored.ready());
    restored.write(0x400100, PGRAPH_ERROR);
    assert!(restored.ready());
    assert!(!restored.irq_pending());
    command(&mut restored, &mut m, 0x1D94, 0xF0).unwrap();
    assert_eq!(m.data[11], 0xAABBCCDD);
}
#[test]
fn perfil_legado_e_payload_corrompido_nao_promovem_graficos() {
    let mut g = Nv2aPgraph::legacy();
    let mut m = fixture().1;
    let before = m.data.clone();
    command(&mut g, &mut m, 0x1D94, 0xF0).unwrap();
    assert_eq!(m.data, before);
    assert_eq!(g.profile(), PgraphProfile::LegacyUnmodeled);
    let bytes = saved(&g);
    let mut r = Nv2aPgraph::default();
    r.load(&mut SnapshotReader::new(&bytes).unwrap()).unwrap();
    assert_eq!(saved(&r), bytes);
    let (modern, _) = fixture();
    let good = saved(&modern);
    for at in [20, 44] {
        let mut bad = good.clone();
        bad[at] = 0xFF;
        let previous = r.clone();
        assert!(r.load(&mut SnapshotReader::new(&bad).unwrap()).is_err());
        assert_eq!(r, previous);
    }
    assert!(modern.validate_access(0x400701, 4, true).is_err());
    assert!(modern.validate_access(0x400700, 4, true).is_err());
    assert!(modern.validate_value(0x600800, 513, 512).is_err());
}

#[test]
fn overflow_dos_contadores_recusa_clear_antes_de_alterar_ram() {
    let (g, mut m) = fixture();
    let original = saved(&g);
    let before = m.data.clone();
    for pixel_counter in [false, true] {
        let mut bytes = original.clone();
        let end = bytes.windows(4).rposition(|v| v == b"PRND").unwrap();
        bytes[end - 17..end - 9]
            .copy_from_slice(&(if pixel_counter { 1u64 } else { u64::MAX }).to_le_bytes());
        if pixel_counter {
            bytes[end - 9..end - 1].copy_from_slice(&u64::MAX.to_le_bytes());
        }
        let mut restored = Nv2aPgraph::default();
        restored
            .load(&mut SnapshotReader::new(&bytes).unwrap())
            .unwrap();
        assert!(command(&mut restored, &mut m, 0x1D94, 0xF0).is_err());
        assert_eq!(saved(&restored), bytes);
        assert_eq!(m.data, before);
    }
}
