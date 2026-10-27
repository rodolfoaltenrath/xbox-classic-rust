//! Desenho indexado, textura, combiner geral e depth do perfil do mesh (ADR-0089).
use xbox_core::{SnapshotReader, SnapshotWriter};
use xbox_gpu::pgraph::{Memory, Nv2aPgraph};

#[derive(Clone, PartialEq, Eq, Debug)]
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

/// `MOV o[saida], v[entrada]` (com FINAL em `ultima`).
fn mov(entrada: u32, saida: u32, ultima: bool) -> [u32; 4] {
    [
        0,
        (1 << 21) | (entrada << 9) | 0x1B,
        2 << 26,
        0xF800 | (saida << 3) | u32::from(ultima),
    ]
}

/// Bytes da RAM (palavras): vértices em 0x1100, textura em 0x5000, cor em 0x2000,
/// depth em 0x4000. Alvo 4×4, cada vértice = posição xyz, cor rgb, textura st.
const VERTICES: u32 = 0x1100;
const TEXTURA: u32 = 0x5000;

fn base(texturizado: bool) -> (Nv2aPgraph, Ram) {
    let mut g = Nv2aPgraph::default();
    let mut m = Ram {
        words: vec![0; 0x1800],
        instance: vec![0; 16],
    };
    for (at, flags, base) in [(0, 3, 0x2000), (4, 3, 0x4000), (8, 2, 0)] {
        m.instance[at..at + 3].copy_from_slice(&[flags, 0x5FFF, base]);
    }
    let mut regs = vec![
        (0x194, 0),
        (0x198, 16),
        (0x19C, 32),
        (0x188, 32),
        (0x200, 4 << 16),
        (0x204, 4 << 16),
        (0x208, 0x128),
        (0x20C, 0x10_0010),
        (0x1D98, 3 << 16),
        (0x1D9C, 3 << 16),
        (0x1D8C, 0xFFFF_FFAA),
        (0x1D94, 3),
        (0x1720, VERTICES),
        (0x172C, VERTICES + 12),
        (0x1744, VERTICES + 24),
        (0x1760, 0x2032),
        (0x176C, 0x2032),
        (0x1784, 0x2022),
        (0x358, 0x0101_0101),
        (0x2C0, 3 << 16),
        (0x2E0, 3 << 16),
        (0x398, 0x4B7F_FFFF),
        (0x354, 0x203),
        (0x30C, 1),
        (0x35C, 1),
        (0x1E94, 6),
        (0x1E60, 1),
        (0x288, 4),
        (0x28C, 0x1400),
    ];
    if texturizado {
        // Combiner do mesh (V0 ← T0 × V0) e textura LU_IMAGE 4×4 nearest.
        regs.extend([
            (0x260, 0xD8D4_1010),
            (0xAA0, 0x40),
            (0xAC0, 0xC8C4_0000),
            (0x1E40, 0x40),
            (0x1E70, 1),
            (0x1B00, TEXTURA),
            (0x1B04, 0x0001_122A),
            (0x1B08, 0x0003_0303),
            (0x1B0C, 0x4003_FFC0),
            (0x1B10, 16 << 16),
            (0x1B14, 0x0101_0000),
            (0x1B1C, (4 << 16) | 4),
        ]);
        for y in 0..4u32 {
            for x in 0..4u32 {
                m.words[(TEXTURA / 4 + y * 4 + x) as usize] =
                    0xFF00_0080 | (60 * x) << 16 | (60 * y) << 8;
            }
        }
    } else {
        regs.extend([(0x260, 0x1010_1010), (0xAC0, 0x1010_1010)]);
    }
    for (method, value) in regs {
        cmd(&mut g, &mut m, method, value).unwrap();
    }
    for (i, palavras) in [mov(0, 0, false), mov(3, 3, false), mov(9, 9, true)]
        .into_iter()
        .enumerate()
    {
        let _ = i;
        for (lane, v) in palavras.into_iter().enumerate() {
            cmd(&mut g, &mut m, 0xB00 + lane as u16 * 4, v).unwrap();
        }
    }
    (g, m)
}

/// Escreve um quad (4 vértices a partir de `primeiro`) cobrindo colunas `x0..x1`.
fn quad(m: &mut Ram, primeiro: u32, x0: f32, x1: f32, z: f32, cor: [f32; 3]) {
    let cantos = [(x0, 0.0), (x1, 0.0), (x0, 4.0), (x1, 4.0)];
    for (k, (x, y)) in cantos.into_iter().enumerate() {
        let v = [x, y, z, cor[0], cor[1], cor[2], x, y];
        let at = (VERTICES / 4 + (primeiro + k as u32) * 8) as usize;
        for (i, f) in v.into_iter().enumerate() {
            m.words[at + i] = f.to_bits();
        }
    }
}

/// Envia os dois triângulos do quad que começa em `primeiro` por `INDEX_DATA`.
fn indices(g: &mut Nv2aPgraph, m: &mut Ram, primeiro: u32) -> Result<(), &'static str> {
    let p = primeiro;
    for (a, b) in [(p, p + 1), (p + 2, p + 1), (p + 3, p + 2)] {
        cmd(g, m, 0x1800, a | b << 16)?;
    }
    Ok(())
}

fn desenhar(g: &mut Nv2aPgraph, m: &mut Ram, quads: &[u32]) -> Result<(), &'static str> {
    cmd(g, m, 0x17FC, 5)?;
    for &q in quads {
        indices(g, m, q)?;
    }
    cmd(g, m, 0x17FC, 0)
}

fn rgb(m: &Ram) -> Vec<u8> {
    m.words[0x800..0x810]
        .iter()
        .flat_map(|&v| [(v >> 16) as u8, (v >> 8) as u8, v as u8])
        .collect()
}

fn ppm(texto: &str) -> Vec<u8> {
    let tokens: Vec<_> = texto
        .lines()
        .filter(|l| !l.starts_with('#'))
        .flat_map(str::split_whitespace)
        .collect();
    assert_eq!(&tokens[..4], &["P3", "4", "4", "255"]);
    tokens[4..].iter().map(|v| v.parse().unwrap()).collect()
}

#[test]
fn golden_textura_4x4_por_indices_e_combiner_do_mesh() {
    let (mut g, mut m) = base(true);
    quad(&mut m, 0, 0.0, 4.0, 1.0, [1.0, 1.0, 1.0]);
    desenhar(&mut g, &mut m, &[0]).unwrap();
    assert_eq!(rgb(&m), ppm(include_str!("reference/texture-4x4.ppm")));
    assert!(
        m.words[0x800..0x810].iter().all(|w| w >> 24 == 0xFF),
        "alfa = T0.a × V0.a"
    );
    let stats = g.draw_stats();
    assert_eq!((stats.calls, stats.triangles, stats.pixels), (1, 2, 16));
}

#[test]
fn golden_depth_leq_com_quads_sobrepostos() {
    let (mut g, mut m) = base(false);
    quad(&mut m, 0, 0.0, 4.0, 10.0, [1.0, 0.0, 0.0]);
    quad(&mut m, 4, 0.0, 2.0, 5.0, [0.0, 0.0, 1.0]);
    quad(&mut m, 8, 0.0, 4.0, 20.0, [0.0, 1.0, 0.0]);
    for q in [0, 4, 8] {
        desenhar(&mut g, &mut m, &[q]).unwrap();
    }
    assert_eq!(
        rgb(&m),
        ppm(include_str!("reference/depth-overlap-4x4.ppm"))
    );
    for (i, &z) in m.words[0x1000..0x1010].iter().enumerate() {
        assert_eq!(
            z >> 8,
            if i % 4 < 2 { 5 } else { 10 },
            "depth gravado no pixel {i}"
        );
    }
}

#[test]
fn indices_e_arrays_produzem_a_mesma_imagem() {
    let (mut g, mut m) = base(false);
    quad(&mut m, 0, 0.0, 4.0, 1.0, [0.2, 0.4, 0.6]);
    let (mut g2, mut m2) = (g.clone(), m.clone());
    desenhar(&mut g, &mut m, &[0]).unwrap();
    // Os mesmos triângulos por DRAW_ARRAYS precisam de vértices repetidos em ordem.
    let ordem = [0u32, 1, 2, 2, 1, 3];
    let copia = m2.words.clone();
    for (k, &v) in ordem.iter().enumerate() {
        let de = (VERTICES / 4 + v * 8) as usize;
        let para = (VERTICES / 4 + (16 + k as u32) * 8) as usize;
        m2.words[para..para + 8].copy_from_slice(&copia[de..de + 8]);
    }
    cmd(&mut g2, &mut m2, 0x17FC, 5).unwrap();
    cmd(&mut g2, &mut m2, 0x1810, (5 << 24) | 16).unwrap();
    cmd(&mut g2, &mut m2, 0x17FC, 0).unwrap();
    assert_eq!(rgb(&m), rgb(&m2));
}

#[test]
fn index_data_fora_de_begin_ou_incompleto_recusa_sem_efeito() {
    let (mut g, mut m) = base(false);
    quad(&mut m, 0, 0.0, 4.0, 1.0, [1.0, 1.0, 1.0]);
    assert!(
        cmd(&mut g, &mut m, 0x1800, 1 << 16).is_err(),
        "fora de BEGIN"
    );
    let antes = m.clone();
    cmd(&mut g, &mut m, 0x17FC, 5).unwrap();
    // Quatro índices: não formam triângulos completos.
    cmd(&mut g, &mut m, 0x1800, 1 << 16).unwrap();
    cmd(&mut g, &mut m, 0x1800, 2 | 3 << 16).unwrap();
    assert!(cmd(&mut g, &mut m, 0x17FC, 0).is_err());
    assert_eq!(m, antes, "draw recusado não escreve pixels");
    assert_eq!(g.draw_stats().calls, 0);
}

#[test]
fn snapshot_no_meio_do_begin_preserva_indices_pendentes() {
    let (mut g, mut m) = base(true);
    quad(&mut m, 0, 0.0, 4.0, 1.0, [1.0, 1.0, 1.0]);
    cmd(&mut g, &mut m, 0x17FC, 5).unwrap();
    cmd(&mut g, &mut m, 0x1800, 1 << 16).unwrap();
    let bytes = saved(&g);
    let mut restaurado = Nv2aPgraph::default();
    restaurado
        .load(&mut SnapshotReader::new(&bytes).unwrap())
        .unwrap();
    assert_eq!(restaurado, g);
    let mut m2 = m.clone();
    for (gpu, ram) in [(&mut g, &mut m), (&mut restaurado, &mut m2)] {
        cmd(gpu, ram, 0x1800, 2 | 1 << 16).unwrap();
        cmd(gpu, ram, 0x1800, 3 | 2 << 16).unwrap();
        cmd(gpu, ram, 0x17FC, 0).unwrap();
    }
    assert_eq!(m, m2);
    assert_eq!(rgb(&m), ppm(include_str!("reference/texture-4x4.ppm")));
}

#[test]
fn prnd_v1_migra_sem_indices_e_recusa_pendentes_corrompidos() {
    let (g, _) = base(true);
    let atual = saved(&g);
    let at = atual.windows(4).rposition(|v| v == b"PRND").unwrap();
    assert_eq!(atual[at + 4..at + 8], 2u32.to_le_bytes());
    assert_eq!(atual[atual.len() - 4..], [0; 4], "nenhum índice pendente");
    // O PGRF guarda o PRND num blob com tamanho u64 nos bytes 24..32.
    let com_tamanho = |mut b: Vec<u8>| {
        let n = (b.len() - 32) as u64;
        b[24..32].copy_from_slice(&n.to_le_bytes());
        b
    };
    // v1: mesma seção sem o contador final.
    let mut v1 = atual[..atual.len() - 4].to_vec();
    v1[at + 4..at + 8].copy_from_slice(&1u32.to_le_bytes());
    let v1 = com_tamanho(v1);
    let mut migrado = Nv2aPgraph::default();
    migrado
        .load(&mut SnapshotReader::new(&v1).unwrap())
        .unwrap();
    assert_eq!(saved(&migrado), atual, "v1 regrava como v2 sem índices");
    // Índices pendentes fora de BEGIN ou acima de 16 bits são recusados.
    for (n, indice) in [(2u32, 1u32), (2, 0x1_0000), (3, 1)] {
        let mut ruim = atual[..atual.len() - 4].to_vec();
        ruim.extend(n.to_le_bytes());
        for _ in 0..n {
            ruim.extend(indice.to_le_bytes());
        }
        let ruim = com_tamanho(ruim);
        assert!(
            Nv2aPgraph::default()
                .load(&mut SnapshotReader::new(&ruim).unwrap())
                .is_err()
        );
    }
}
