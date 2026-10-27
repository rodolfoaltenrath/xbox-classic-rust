//! Renderização serial do perfil público `SoftwareV1` (ADR-0068).
mod combiner;
mod persistence;
mod raster;
mod texture;
mod vertex;
use super::{Memory, Surface};
use std::collections::BTreeMap;
pub(super) const PROGRAMS: usize = 136;
pub(super) const CONSTANTS: usize = 192;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DrawStats {
    pub calls: u64,
    pub triangles: u64,
    pub pixels: u64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Render {
    registers: BTreeMap<u16, u32>,
    program: Vec<[u32; 4]>,
    constants: Vec<[u32; 4]>,
    program_mask: Vec<u8>,
    constant_mask: Vec<u8>,
    program_load: u32,
    constant_load: u32,
    /// Índices de `INDEX_DATA` recebidos desde o BEGIN, desenhados no END (ADR-0089).
    indices: Vec<u32>,
    pub(super) stats: DrawStats,
}

/// Limite de índices pendentes num BEGIN/END: protege contra pushbuffer corrompido.
pub(super) const MAX_INDICES: usize = 1 << 20;
impl Default for Render {
    fn default() -> Self {
        Self {
            registers: BTreeMap::new(),
            program: vec![[0; 4]; PROGRAMS],
            constants: vec![[0; 4]; CONSTANTS],
            program_mask: vec![0; PROGRAMS],
            constant_mask: vec![0; CONSTANTS],
            program_load: 0,
            constant_load: 0,
            indices: Vec::new(),
            stats: DrawStats::default(),
        }
    }
}
impl Render {
    pub(super) fn models(method: u16) -> bool {
        matches!(method,0x184|0x188|0x19C|0x1A0|0x1B00..=0x1BFC|0x288|0x28C|0x290|0x2A4|0x2B4|0x2C0..=0x2FC|0x300|0x304|0x308|0x30C|0x32C|0x354..=0x378|0x388..=0x3A0|0x260..=0x27C|0xAA0..=0xADC|0xB00..=0xB7C|0xB80..=0xBFC|0x1720..=0x179C|0x17BC|0x17FC|0x1800|0x1810|0x1E40..=0x1E60|0x1E70|0x1E94..=0x1EA4)
            && method.is_multiple_of(4)
    }
    fn reg(&self, method: u16) -> u32 {
        self.registers.get(&method).copied().unwrap_or(0)
    }
    pub(super) fn execute(
        &mut self,
        method: u16,
        raw: u32,
        resolved: u32,
        memory: &mut impl Memory,
        surface: Surface,
    ) -> Result<(), &'static str> {
        match method {
            0x1E9C => {
                if raw >= PROGRAMS as u32 {
                    return Err("cursor de programa fora de faixa");
                }
                self.program_load = raw;
            }
            0x1EA4 => {
                if raw >= CONSTANTS as u32 {
                    return Err("cursor de constantes fora de faixa");
                }
                self.constant_load = raw;
            }
            0xB00..=0xB7C => {
                let lane = usize::from((method - 0xB00) / 4) % 4;
                let at = self.program_load as usize;
                let p = self
                    .program
                    .get_mut(at)
                    .ok_or("upload de programa excede limite")?;
                p[lane] = raw;
                self.program_mask[at] |= 1 << lane;
                if lane == 3 {
                    self.program_load += 1;
                }
            }
            0xB80..=0xBFC => {
                let lane = usize::from((method - 0xB80) / 4) % 4;
                let p = self
                    .constants
                    .get_mut(self.constant_load as usize)
                    .ok_or("upload de constantes excede limite")?;
                p[lane] = raw;
                self.constant_mask[self.constant_load as usize] |= 1 << lane;
                if lane == 3 {
                    self.constant_load += 1;
                }
            }
            0x184 | 0x188 | 0x19C | 0x1A0 => {
                self.registers.insert(method, resolved);
            }
            0x17FC => {
                if !matches!(raw, 0 | 5) {
                    return Err("primitiva fora do perfil: somente TRIANGLES");
                }
                if raw == 5 && self.reg(0x17FC) != 0 {
                    return Err("BEGIN aninhado");
                }
                if raw == 0 && !self.indices.is_empty() {
                    // END: o desenho indexado inteiro, de uma vez; só então limpa a lista.
                    let indices = std::mem::take(&mut self.indices);
                    if let Err(erro) = self.draw(&indices, memory, surface) {
                        self.indices = indices;
                        return Err(erro);
                    }
                }
                self.registers.insert(method, raw);
            }
            0x1800 => {
                if self.reg(0x17FC) != 5 {
                    return Err("INDEX_DATA fora de BEGIN TRIANGLES");
                }
                if self.indices.len() + 2 > MAX_INDICES {
                    return Err("INDEX_DATA excede limite de indices pendentes");
                }
                // Dois índices de 16 bits por palavra, o baixo primeiro.
                self.indices.extend([raw & 0xFFFF, raw >> 16]);
            }
            0x1810 => {
                let count = (raw >> 24) + 1;
                let start = raw & 0xFF_FFFF;
                let indices: Vec<u32> = (start..start + count).collect();
                self.draw(&indices, memory, surface)?;
            }
            _ if Self::models(method) => {
                self.registers.insert(method, raw);
            }
            _ => (),
        }
        Ok(())
    }
    /// Desenha a lista de triângulos formada por `indices` (`DRAW_ARRAYS` gera índices
    /// consecutivos; `INDEX_DATA` traz os do guest). Cada índice é transformado uma vez.
    fn draw(
        &mut self,
        indices: &[u32],
        mem: &mut impl Memory,
        surface: Surface,
    ) -> Result<(), &'static str> {
        self.validate_pipeline()?;
        let count = indices.len() as u32;
        if !count.is_multiple_of(3) {
            return Err("draw deve conter triangulos completos neste perfil");
        }
        let mut cache: BTreeMap<u32, vertex::Vertex> = BTreeMap::new();
        let mut vertices = Vec::with_capacity(indices.len());
        for &index in indices {
            let vertex = if let Some(v) = cache.get(&index) {
                *v
            } else {
                let v = vertex::run(self, &vertex::attributes(self, index, mem)?)?;
                cache.insert(index, v);
                v
            };
            vertices.push(vertex);
        }
        let calls = self
            .stats
            .calls
            .checked_add(1)
            .ok_or("contador de draws esgotado")?;
        let triangles = self
            .stats
            .triangles
            .checked_add(u64::from(count / 3))
            .ok_or("contador de triangulos esgotado")?;
        let pipeline = raster::Pipeline {
            combiner: combiner::Combiner::ler(self)?,
            textura: if self.reg(0x1E70) & 0x1F == 1 {
                Some(texture::Textura::ler(self, 0, mem)?)
            } else {
                None
            },
        };
        let pixels = raster::draw(
            self,
            &pipeline,
            &vertices,
            mem,
            surface,
            u64::MAX - self.stats.pixels,
        )?;
        self.stats = DrawStats {
            calls,
            triangles,
            pixels: self.stats.pixels + pixels,
        };
        Ok(())
    }
    fn validate_pipeline(&self) -> Result<(), &'static str> {
        if self.reg(0x17FC) != 5 || self.reg(0x1E94) != 6 || self.reg(0x1E98) != 0 {
            return Err(
                "draw exige BEGIN TRIANGLES e vertex program privado sem escrita de contexto",
            );
        }
        for reg in [0x308, 0x30C, 0x32C, 0x35C] {
            if self.reg(reg) > 1 {
                return Err("habilitacao raster deve ser zero ou um neste perfil");
            }
        }
        // Combiner e textura são decodificados por `Combiner::ler`/`Textura::ler`, que
        // recusam o que está fora do recorte (ADR-0089). Aqui: shader stage 0 em NONE ou
        // PROJECT2D e estágios 1–3 sem textura.
        if !matches!(self.reg(0x1E70) & 0x1F, 0 | 1) || self.reg(0x1E70) >> 5 != 0 {
            return Err("shader stage fora do perfil (estagio 0 NONE/PROJECT2D)");
        }
        for reg in [0x2A4, 0x300, 0x304, 0x388, 0x17BC] {
            if self.reg(reg) != 0 {
                return Err("fog/alpha/blend/offset/logic sem modelo no raster");
            }
        }
        if self.reg(0x290) & 0x1000 != 0 || self.reg(0x290) & !0x110001 != 0 {
            return Err("controle zeta fora do perfil fixed depth");
        }
        if self.reg(0x32C) != 0
            && (self.reg(0x364) != 0x207
                || [0x370, 0x374, 0x378]
                    .into_iter()
                    .any(|r| self.reg(r) != 0x1E00))
        {
            return Err("stencil exige ALWAYS/KEEP neste perfil");
        }
        if self.reg(0x30C) != 0 && !matches!(self.reg(0x354), 0x201 | 0x203 | 0x207) {
            return Err("comparacao depth fora do perfil");
        }
        if self.reg(0x308) != 0
            && (!matches!(self.reg(0x39C), 0x404..=0x406)
                || !matches!(self.reg(0x3A0), 0x900 | 0x901))
        {
            return Err("culling fora do perfil");
        }
        for reg in [0x38C, 0x390] {
            if !matches!(self.reg(reg), 0 | 0x1B02) {
                return Err("modo de poligono exige FILL");
            }
        }
        if self.reg(0x358) & !0x01010101 != 0 {
            return Err("mascara de cor invalida");
        }
        Ok(())
    }
}
