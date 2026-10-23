use xbox_core::{SnapshotReader, SnapshotWriter};
use xbox_gpu::pgraph::{Memory, Nv2aPgraph, PgraphProfile};
#[derive(Clone)]
struct Ram {
    words: Vec<u32>,
    instance: Vec<u32>,
}
impl Memory for Ram {
    fn ram_len(&self) -> u64 {
        self.words.len() as u64 * 4
    }
    fn read_instance(&self, at: u32) -> Result<u32, &'static str> {
        self.instance
            .get(at as usize / 4)
            .copied()
            .ok_or("instance")
    }
    fn read_ram(&self, at: u32) -> Result<u32, &'static str> {
        self.words.get(at as usize / 4).copied().ok_or("ram")
    }
    fn write_ram(&mut self, at: u32, value: u32) -> Result<(), &'static str> {
        *self.words.get_mut(at as usize / 4).ok_or("ram")? = value;
        Ok(())
    }
}
fn cmd(g: &mut Nv2aPgraph, m: &mut Ram, method: u16, value: u32) -> Result<(), &'static str> {
    g.execute(0x97, 0, method, value, value, m)
}
fn saved(g: &Nv2aPgraph) -> Vec<u8> {
    let mut w = SnapshotWriter::new();
    g.save(&mut w);
    w.finish()
}
fn upload(g: &mut Nv2aPgraph, m: &mut Ram, words: [u32; 4]) {
    for (i, value) in words.into_iter().enumerate() {
        cmd(g, m, 0xB00 + i as u16 * 4, value).unwrap();
    }
}
fn mov(input: u32, output: u32, last: bool) -> [u32; 4] {
    [
        0,
        (1 << 21) | (input << 9) | 0x1B,
        2 << 26,
        0xF800 | (output << 3) | u32::from(last),
    ]
}
fn fixture() -> (Nv2aPgraph, Ram) {
    let mut g = Nv2aPgraph::default();
    let mut m = Ram {
        words: vec![0; 0x1800],
        instance: vec![0; 16],
    };
    for (at, flags, base) in [(0, 3, 0x2000), (4, 3, 0x4000), (8, 2, 0)] {
        m.instance[at..at + 3].copy_from_slice(&[flags, 0x5FFF, base]);
    }
    let vertices: [[f32; 6]; 3] = [
        [0., 0., 1., 1., 0., 0.],
        [4., 0., 1., 0., 1., 0.],
        [0., 4., 1., 0., 0., 1.],
    ];
    for (i, value) in vertices.into_iter().flatten().enumerate() {
        m.words[0x400 + i] = value.to_bits();
    }
    for (method, value) in [
        (0x194, 0),
        (0x198, 16),
        (0x19C, 32),
        (0x200, 4 << 16),
        (0x204, 4 << 16),
        (0x208, 0x128),
        (0x20C, 0x100010),
        (0x1D98, 3 << 16),
        (0x1D9C, 3 << 16),
        (0x1D8C, 0xFFFFFFAA),
        (0x1D94, 3),
        (0x1720, 0x1000),
        (0x172C, 0x100C),
        (0x1760, 0x1832),
        (0x176C, 0x1832),
        (0x260, 0x10101010),
        (0xAC0, 0x10101010),
        (0x1E60, 1),
        (0x288, 4),
        (0x28C, 0x1400),
        (0x358, 0x01010101),
        (0x2C0, 3 << 16),
        (0x2E0, 3 << 16),
        (0x398, 0x4B7FFFFF),
        (0x354, 0x203),
        (0x30C, 1),
        (0x35C, 1),
        (0x1E94, 6),
    ] {
        cmd(&mut g, &mut m, method, value).unwrap();
    }
    upload(&mut g, &mut m, mov(0, 0, false));
    upload(&mut g, &mut m, mov(3, 3, true));
    cmd(&mut g, &mut m, 0x17FC, 5).unwrap();
    (g, m)
}
fn draw(g: &mut Nv2aPgraph, m: &mut Ram) -> Result<(), &'static str> {
    cmd(g, m, 0x1810, 2 << 24)
}
fn rgb(m: &Ram) -> Vec<u8> {
    m.words[0x800..0x810]
        .iter()
        .flat_map(|&v| [(v >> 16) as u8, (v >> 8) as u8, v as u8])
        .collect()
}
#[test]
fn golden_arrays_vertex_program_e_raster_comparam_imagem_rgb_versionada() {
    let (mut g, mut m) = fixture();
    draw(&mut g, &mut m).unwrap();
    let text = include_str!("reference/triangle-4x4.ppm");
    let tokens: Vec<_> = text
        .lines()
        .filter(|line| !line.starts_with('#'))
        .flat_map(str::split_whitespace)
        .collect();
    assert_eq!(&tokens[..4], &["P3", "4", "4", "255"]);
    let expected: Vec<u8> = tokens[4..].iter().map(|v| v.parse().unwrap()).collect();
    assert_eq!(rgb(&m), expected);
    for i in 0..16 {
        let covered = [0, 1, 2, 4, 5, 8].contains(&i);
        assert_eq!(m.words[0x800 + i] >> 24, if covered { 255 } else { 0 });
        assert_eq!(
            m.words[0x1000 + i],
            if covered { 0x1AA } else { 0xFFFFFFAA }
        );
    }
    assert_eq!(g.draw_stats().calls, 1);
    assert_eq!(g.draw_stats().triangles, 1);
    assert_eq!(g.draw_stats().pixels, 6);
}
#[test]
fn depth_cull_mascara_e_window_preservam_pixels_fora_do_recorte() {
    let (mut g, mut m) = fixture();
    cmd(&mut g, &mut m, 0x308, 1).unwrap();
    cmd(&mut g, &mut m, 0x39C, 0x405).unwrap();
    cmd(&mut g, &mut m, 0x3A0, 0x901).unwrap();
    draw(&mut g, &mut m).unwrap();
    assert_eq!(g.draw_stats().pixels, 0);
    cmd(&mut g, &mut m, 0x3A0, 0x900).unwrap();
    cmd(&mut g, &mut m, 0x358, 0x10000).unwrap();
    draw(&mut g, &mut m).unwrap();
    assert_eq!(m.words[0x800], 191 << 16);
    cmd(&mut g, &mut m, 0x354, 0x201).unwrap();
    cmd(&mut g, &mut m, 0x358, 0x01010101).unwrap();
    let before = m.words.clone();
    draw(&mut g, &mut m).unwrap();
    assert_eq!(m.words, before);
    cmd(&mut g, &mut m, 0x354, 0x203).unwrap();
    cmd(&mut g, &mut m, 0x308, 0).unwrap();
    m.words[0x800..0x810].fill(0);
    for i in 0..8 {
        cmd(&mut g, &mut m, 0x2C0 + i * 4, 0x00020001).unwrap();
        cmd(&mut g, &mut m, 0x2E0 + i * 4, 0).unwrap();
    }
    draw(&mut g, &mut m).unwrap();
    assert_eq!(m.words[0x800], 0);
    assert_ne!(m.words[0x801], 0);
    assert_ne!(m.words[0x802], 0);
    assert_eq!(m.words[0x804], 0);
}
#[test]
fn erros_de_dma_shader_e_pipeline_nao_publicam_ram_ou_estatisticas() {
    for (method, value) in [
        (0x1760, 0x1831),
        (0x1720, 0x5FF0),
        (0x1E70, 1),
        (0x304, 1),
        (0x1EA0, 135),
    ] {
        let (mut g, mut m) = fixture();
        cmd(&mut g, &mut m, method, value).unwrap();
        let before = m.words.clone();
        assert!(draw(&mut g, &mut m).is_err());
        assert_eq!(m.words, before);
        assert_eq!(g.draw_stats().calls, 0);
    }
    let (mut g, mut m) = fixture();
    cmd(&mut g, &mut m, 0x1E9C, 0).unwrap();
    upload(&mut g, &mut m, [0, 15 << 21, 0, 1]);
    let before = m.words.clone();
    assert!(draw(&mut g, &mut m).is_err());
    assert_eq!(m.words, before);
}
#[test]
fn snapshot_no_meio_do_upload_e_begin_reproduz_ram_e_estado_integral() {
    let (mut g, mut m) = fixture();
    cmd(&mut g, &mut m, 0x1E9C, 1).unwrap();
    cmd(&mut g, &mut m, 0xB00, 0).unwrap();
    let checkpoint = saved(&g);
    let mut r = Nv2aPgraph::default();
    r.load(&mut SnapshotReader::new(&checkpoint).unwrap())
        .unwrap();
    assert_eq!(saved(&r), checkpoint);
    let mut mr = m.clone();
    for (graph, memory) in [(&mut g, &mut m), (&mut r, &mut mr)] {
        let words = mov(3, 3, true);
        for (i, value) in words.into_iter().enumerate().skip(1) {
            cmd(graph, memory, 0xB00 + i as u16 * 4, value).unwrap();
        }
        draw(graph, memory).unwrap();
        cmd(graph, memory, 0x17FC, 0).unwrap();
    }
    assert_eq!(mr.words, m.words);
    assert_eq!(saved(&r), saved(&g));
}
#[test]
fn pgrf_v1_migra_para_clear_sem_raster_e_nao_promove_no_replay() {
    let (g, mut m) = fixture();
    let mut bytes = saved(&g);
    let prnd = bytes.windows(4).rposition(|v| v == b"PRND").unwrap();
    bytes.truncate(prnd);
    bytes[16..20].copy_from_slice(&1u32.to_le_bytes());
    bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
    let len = (bytes.len() - 32) as u64;
    bytes[24..32].copy_from_slice(&len.to_le_bytes());
    let mut r = Nv2aPgraph::default();
    r.load(&mut SnapshotReader::new(&bytes).unwrap()).unwrap();
    assert_eq!(r.profile(), PgraphProfile::ClearV1);
    let before = m.words.clone();
    draw(&mut r, &mut m).unwrap();
    assert_eq!(m.words, before);
    assert_eq!(r.draw_stats().calls, 0);
    let migrated = saved(&r);
    r.load(&mut SnapshotReader::new(&migrated).unwrap())
        .unwrap();
    assert_eq!(saved(&r), migrated);
}

#[test]
fn programa_mov_mul_add_mad_rcp_obedece_constantes_e_nao_reconhece_bytecode() {
    let (mut g, mut m) = fixture();
    // Captura de oito instruções NV2A do sample público (pin/CG no ADR-0066).
    // Matriz identidade nos dados dá a mesma imagem analítica; outra matriz
    // produz outra geometria sem modificar código, programa ou nome de título.
    let program = [
        [0, 0x004C2055, 0x0836186C, 0x2F0007F8],
        [0, 0x008C0000, 0x0836186C, 0x1F0007F8],
        [0, 0x008C40AA, 0x0836186C, 0x1F0007F8],
        [0, 0x006C601B, 0x0436106C, 0x3F0007F8],
        [0, 0x0400001B, 0x08361300, 0x101807F8],
        [0, 0x0040001B, 0x0400286C, 0x2070E800],
        [0, 0x0020061B, 0x0836106C, 0x2070F818],
        [0, 0x0020001B, 0x0436106C, 0x20701801],
    ];
    cmd(&mut g, &mut m, 0x1E9C, 0).unwrap();
    for words in program {
        upload(&mut g, &mut m, words);
    }
    cmd(&mut g, &mut m, 0x1EA4, 96).unwrap();
    for i in 0..16 {
        cmd(
            &mut g,
            &mut m,
            0xB80 + i * 4,
            if i % 5 == 0 { 1.0f32.to_bits() } else { 0 },
        )
        .unwrap();
    }
    draw(&mut g, &mut m).unwrap();
    let full = rgb(&m);
    assert_eq!(g.draw_stats().pixels, 6);
    let expected: Vec<u8> = include_str!("reference/triangle-4x4.ppm")
        .lines()
        .filter(|l| !l.starts_with('#'))
        .flat_map(str::split_whitespace)
        .skip(4)
        .map(|v| v.parse().unwrap())
        .collect();
    assert_eq!(full, expected);
    cmd(&mut g, &mut m, 0x1EA4, 96).unwrap();
    cmd(&mut g, &mut m, 0xB80, 0.5f32.to_bits()).unwrap();
    m.words[0x800..0x810].fill(0);
    draw(&mut g, &mut m).unwrap();
    assert_ne!(rgb(&m), full);
    assert!(g.draw_stats().pixels < 12);
}

#[test]
fn snapshot_recusa_cursores_registros_mascaras_e_versao_sem_publicar_backend() {
    let (g, _) = fixture();
    let bytes = saved(&g);
    let at = bytes.windows(4).rposition(|v| v == b"PRND").unwrap();
    let n = u32::from_le_bytes(bytes[at + 8..at + 12].try_into().unwrap()) as usize;
    let cursor = at + 12 + n * 6;
    for offset in [at + 4, at + 8, at + 12, cursor, cursor + 8] {
        let mut bad = bytes.clone();
        bad[offset] = 255;
        let mut r = Nv2aPgraph::legacy();
        let before = r.clone();
        assert!(
            r.load(&mut SnapshotReader::new(&bad).unwrap()).is_err(),
            "offset={offset}"
        );
        assert_eq!(r, before);
    }
}
#[test]
fn overflow_estatisticas_raster_recusa_sem_publicar_pixels() {
    let (g, mut m) = fixture();
    let original = saved(&g);
    let before = m.words.clone();
    for pixel_counter in [false, true] {
        let mut bytes = original.clone();
        let end = bytes.len();
        let count = if pixel_counter { 1u64 } else { u64::MAX };
        bytes[end - 24..end - 16].copy_from_slice(&count.to_le_bytes());
        bytes[end - 16..end - 8].copy_from_slice(&count.to_le_bytes());
        if pixel_counter {
            bytes[end - 8..end].copy_from_slice(&u64::MAX.to_le_bytes());
        }
        let mut restored = Nv2aPgraph::default();
        restored
            .load(&mut SnapshotReader::new(&bytes).unwrap())
            .unwrap();
        assert!(draw(&mut restored, &mut m).is_err());
        assert_eq!(m.words, before);
        assert_eq!(saved(&restored), bytes);
    }
}

#[test]
fn entradas_vp_nao_definidas_recusam_em_vez_de_inventar_cor_zero() {
    for (d1, d2) in [
        ((1 << 21) | 0x1B, 1 << 26),
        ((1 << 21) | (96 << 13) | 0x1B, 3 << 26),
    ] {
        let (mut g, mut m) = fixture();
        cmd(&mut g, &mut m, 0x1E9C, 1).unwrap();
        upload(&mut g, &mut m, [0, d1, d2, 0xF819]);
        let before = m.words.clone();
        assert!(draw(&mut g, &mut m).is_err());
        assert_eq!(m.words, before);
    }
}
