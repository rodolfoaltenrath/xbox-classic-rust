//! Raster próprio: amostras no centro, arestas top/left e precisão declarada no ADR.
use super::{
    Render,
    combiner::{Combiner, Entradas},
    texture::Textura,
    vertex::Vertex,
};
use crate::pgraph::{Memory, Surface, clear};
struct Target {
    base: u32,
    pitch: u32,
    width: u32,
    height: u32,
    words: Vec<u32>,
}
impl Target {
    fn read(surface: Surface, depth: bool, mem: &impl Memory) -> Result<Self, &'static str> {
        let (base, pitch, width, height) = clear::target(surface, depth, mem)?;
        let mut words = Vec::with_capacity((width * height) as usize);
        for y in 0..height {
            for x in 0..width {
                words.push(mem.read_ram(base + y * pitch + x * 4)?);
            }
        }
        Ok(Self {
            base,
            pitch,
            width,
            height,
            words,
        })
    }
    fn write(&self, mem: &mut impl Memory) -> Result<(), &'static str> {
        for y in 0..self.height {
            for x in 0..self.width {
                mem.write_ram(
                    self.base + y * self.pitch + x * 4,
                    self.words[(y * self.width + x) as usize],
                )?;
            }
        }
        Ok(())
    }
}
#[derive(Clone, Copy)]
struct Point {
    x: i64,
    y: i64,
}
fn edge(a: Point, b: Point, p: Point) -> i64 {
    (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x)
}
fn included(a: Point, b: Point, e: i64) -> bool {
    e > 0 || (e == 0 && (b.y < a.y || (b.y == a.y && b.x > a.x)))
}
fn point(v: Vertex) -> Result<Point, &'static str> {
    let [x, y, _, _] = v.position;
    if x.abs() > 1048576.0 || y.abs() > 1048576.0 {
        return Err("coordenadas excedem guard band do perfil software");
    }
    Ok(Point {
        x: (f64::from(x) * 256.0).round() as i64,
        y: (f64::from(y) * 256.0).round() as i64,
    })
}
fn color_mask(raw: u32) -> u32 {
    let mut mask = 0;
    for (bit, channel) in [
        (1, 0x000000FF),
        (0x100, 0x0000FF00),
        (0x10000, 0x00FF0000),
        (0x1000000, 0xFF000000),
    ] {
        if raw & bit != 0 {
            mask |= channel;
        }
    }
    mask
}
fn channel(x: f64) -> u32 {
    u32::try_from((x.clamp(0.0, 1.0) * 255.0).round() as i64).expect("canal 0..255")
}
/// Estado de fragmento decodificado no início do draw (ADR-0089).
pub(super) struct Pipeline {
    pub combiner: Combiner,
    /// Textura do estágio 0 em PROJECT2D, com os texels já copiados.
    pub textura: Option<Textura>,
}

struct Draw<'a> {
    state: &'a Render,
    pipeline: &'a Pipeline,
    color: Target,
    depth: Option<Target>,
    mask: u32,
    pixels: u64,
    capacity: u64,
    /// Pixels de caixa envolvente já percorridos neste draw (orçamento de trabalho).
    area: u64,
}

/// Orçamento de pixels percorridos por draw (soma das caixas envolventes recortadas
/// ao alvo): protege contra pushbuffer que pediria trabalho ilimitado (ADR-0089).
const ORCAMENTO_AREA: u64 = 64 * 1024 * 1024;
impl Draw<'_> {
    #[allow(clippy::cast_precision_loss)] // Pesos f64 aproximam a divisão das arestas i64; precisão software declarada (ADR-0068).
    fn triangle(&mut self, mut v: [Vertex; 3]) -> Result<(), &'static str> {
        let mut p = [point(v[0])?, point(v[1])?, point(v[2])?];
        let mut area = edge(p[0], p[1], p[2]);
        if area == 0 {
            return Ok(());
        }
        let front = if self.state.reg(0x3A0) == 0x900 {
            area > 0
        } else {
            area < 0
        };
        if self.state.reg(0x308) != 0
            && match self.state.reg(0x39C) {
                0x404 => front,
                0x405 => !front,
                0x406 => true,
                _ => false,
            }
        {
            return Ok(());
        }
        if area < 0 {
            v.swap(1, 2);
            p.swap(1, 2);
            area = -area;
        }
        let min_x =
            u32::try_from(((p.iter().map(|p| p.x).min().unwrap() - 128).div_euclid(256)).max(0))
                .expect("guard band positivo");
        let min_y =
            u32::try_from(((p.iter().map(|p| p.y).min().unwrap() - 128).div_euclid(256)).max(0))
                .expect("guard band positivo");
        let max_x = u32::try_from(
            ((p.iter().map(|p| p.x).max().unwrap() + 128).div_euclid(256))
                .max(0)
                .min(i64::from(self.color.width - 1)),
        )
        .expect("largura limitada");
        let max_y = u32::try_from(
            ((p.iter().map(|p| p.y).max().unwrap() + 128).div_euclid(256))
                .max(0)
                .min(i64::from(self.color.height - 1)),
        )
        .expect("altura limitada");
        if min_x <= max_x && min_y <= max_y {
            self.area += u64::from(max_x - min_x + 1) * u64::from(max_y - min_y + 1);
            if self.area > ORCAMENTO_AREA {
                return Err("draw excede orcamento de raster");
            }
        }
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                if !self.window(x, y) {
                    continue;
                }
                let q = Point {
                    x: i64::from(x) * 256 + 128,
                    y: i64::from(y) * 256 + 128,
                };
                let e = [
                    edge(p[1], p[2], q),
                    edge(p[2], p[0], q),
                    edge(p[0], p[1], q),
                ];
                if !included(p[1], p[2], e[0])
                    || !included(p[2], p[0], e[1])
                    || !included(p[0], p[1], e[2])
                {
                    continue;
                }
                let weights = e.map(|e| e as f64 / area as f64);
                self.fragment(x, y, v, weights)?;
            }
        }
        Ok(())
    }
    fn window(&self, x: u32, y: u32) -> bool {
        // Só clip inclusivo é aceito; oito retângulos em união.
        (0..8).any(|i| {
            let h = self.state.reg(0x2C0 + i * 4);
            let v = self.state.reg(0x2E0 + i * 4);
            x >= h & 0xFFF && x <= (h >> 16) & 0xFFF && y >= v & 0xFFF && y <= (v >> 16) & 0xFFF
        })
    }
    /// Cor do fragmento: V0 e coordenadas de textura com perspectiva, amostra do
    /// estágio 0 e combiners (ADR-0089).
    fn sombrear(
        &self,
        vertices: &[Vertex; 3],
        pesos: [f64; 3],
        divisor: f64,
    ) -> Result<[u32; 4], &'static str> {
        let perspectiva = |valor: &dyn Fn(usize) -> f64| {
            (0..3)
                .map(|i| pesos[i] * valor(i) / f64::from(vertices[i].position[3]))
                .sum::<f64>()
                / divisor
        };
        // V0 chega ao combiner com clamp por vértice (`vtxD0` do xemu).
        let v0: [f64; 4] = std::array::from_fn(|lane| {
            perspectiva(&|i| f64::from(vertices[i].color[lane]).clamp(0.0, 1.0))
        });
        let mut texturas = [[0.0, 0.0, 0.0, 1.0]; 4];
        if let Some(textura) = &self.pipeline.textura {
            let homogenea = perspectiva(&|i| f64::from(vertices[i].texcoord[3]));
            if homogenea == 0.0 || !homogenea.is_finite() {
                return Err("coordenada de textura projetiva com q nulo");
            }
            let coord_s = perspectiva(&|i| f64::from(vertices[i].texcoord[0])) / homogenea;
            let coord_t = perspectiva(&|i| f64::from(vertices[i].texcoord[1])) / homogenea;
            texturas[0] = textura.amostrar(coord_s, coord_t);
        }
        let rgba = self
            .pipeline
            .combiner
            .avaliar(&Entradas { v0, t: texturas });
        Ok(rgba.map(channel))
    }
    fn fragment(
        &mut self,
        x: u32,
        y: u32,
        v: [Vertex; 3],
        weights: [f64; 3],
    ) -> Result<(), &'static str> {
        let index = (y * self.color.width + x) as usize;
        let divisor = (0..3)
            .map(|i| weights[i] / f64::from(v[i].position[3]))
            .sum::<f64>();
        if !divisor.is_finite() || divisor <= 0.0 {
            return Err("interpolacao perspectiva invalida");
        }
        // Z_PERSPECTIVE (SET_CONTROL0 bit 16): a profundidade é o W do fragmento com
        // interpolação de perspectiva, 1/Σ(bᵢ/wᵢ), como no xemu (`psh.c`, ADR-0089).
        let z = if self.state.reg(0x290) & 0x10000 != 0 {
            1.0 / divisor
        } else {
            (0..3)
                .map(|i| weights[i] * f64::from(v[i].position[2]))
                .sum::<f64>()
        };
        let min_z = f64::from(f32::from_bits(self.state.reg(0x394)));
        let max_z = f64::from(f32::from_bits(self.state.reg(0x398)));
        if !min_z.is_finite() || !max_z.is_finite() || min_z > max_z {
            return Err("limites depth invalidos");
        }
        if z < min_z || z > max_z {
            return Ok(());
        }
        let depth = u32::try_from(z.clamp(0.0, 16777215.0).round() as i64).expect("depth 24 bits");
        if let Some(target) = &mut self.depth {
            let old = target.words[index] >> 8;
            let pass = match self.state.reg(0x354) {
                0x201 => depth < old,
                0x203 => depth <= old,
                0x207 => true,
                _ => false,
            };
            if !pass {
                return Ok(());
            }
            if self.state.reg(0x35C) != 0 {
                target.words[index] = (depth << 8) | (target.words[index] & 255);
            }
        }
        let channels = self.sombrear(&v, weights, divisor)?;
        let rgba = (channels[3] << 24) | (channels[0] << 16) | (channels[1] << 8) | channels[2];
        self.color.words[index] = (self.color.words[index] & !self.mask) | (rgba & self.mask);
        self.pixels = self
            .pixels
            .checked_add(1)
            .filter(|&n| n <= self.capacity)
            .ok_or("contador de pixels raster esgotado")?;
        Ok(())
    }
}
pub(super) fn draw(
    state: &Render,
    pipeline: &Pipeline,
    vertices: &[Vertex],
    mem: &mut impl Memory,
    surface: Surface,
    capacity: u64,
) -> Result<u64, &'static str> {
    if state.reg(0x2B4) != 0 {
        return Err("window clip exclusivo sem modelo");
    }
    let color = Target::read(surface, false, mem)?;
    let depth = if state.reg(0x30C) != 0 {
        Some(Target::read(surface, true, mem)?)
    } else {
        None
    };
    let mut draw = Draw {
        state,
        pipeline,
        color,
        depth,
        mask: color_mask(state.reg(0x358)),
        pixels: 0,
        capacity,
        area: 0,
    };
    for triangle in vertices.as_chunks::<3>().0 {
        draw.triangle(*triangle)?;
    }
    // Todos os vertices/erros/pixels foram resolvidos em buffers locais. Publicacao
    // somente depois do draw inteiro valido; Memory garante RAM exclusiva estavel.
    draw.color.write(mem)?;
    if let Some(depth) = &draw.depth {
        depth.write(mem)?;
    }
    Ok(draw.pixels)
}
