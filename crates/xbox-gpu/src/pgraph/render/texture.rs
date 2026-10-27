//! Textura de estágio no perfil `SoftwareV1` (ADR-0089).
//!
//! Recorte guiado pelo consumidor real (sample `mesh`): formato `LU_IMAGE_A8R8G8B8`
//! (linear, coordenadas em texels), um nível, endereçamento clamp-to-edge e filtros
//! nearest, linear ou convolução 2D gaussiana 3×3. A convolução segue o perfil público
//! do xemu fixado no ADR-0056 (`psh.c`, `apply_convolution_filter`): nove amostras
//! bilineares deslocadas de um texel, pesos 1-2-1/16. Não é medição de hardware.
//! Qualquer outro formato, modo ou filtro é recusado antes do raster.

use super::Render;
use crate::pgraph::{Memory, clear};

/// Formato de cor `LU_IMAGE_A8R8G8B8` (`NV097_SET_TEXTURE_FORMAT_COLOR`).
const LU_IMAGE_A8R8G8B8: u32 = 0x12;
/// Modo de endereçamento clamp-to-edge.
const CLAMP_TO_EDGE: u32 = 3;
/// Limite de texels copiados por estágio (4096², a maior dimensão do NV2A).
const MAX_TEXELS: u64 = 4096 * 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Filtro {
    Nearest,
    Linear,
    Gaussiano,
}

/// Textura de um estágio, com os texels já copiados da RAM no início do draw.
#[derive(Debug, Clone)]
pub(super) struct Textura {
    largura: u32,
    altura: u32,
    filtro: Filtro,
    /// `A8R8G8B8`, linha a linha, sem pitch.
    texels: Vec<u32>,
}

fn filtro(campo: u32, gaussiano: bool) -> Result<Filtro, &'static str> {
    Ok(match campo {
        1 => Filtro::Nearest,
        2 => Filtro::Linear,
        // MIN = 7 e MAG = 4 são CONVOLUTION_2D_LOD0; o kernel vem dos bits 15:13.
        7 | 4 if gaussiano => Filtro::Gaussiano,
        _ => return Err("filtro de textura fora do perfil (nearest/linear/gaussiano)"),
    })
}

impl Textura {
    /// Lê a configuração do estágio `i` e copia os texels. Erro em qualquer campo fora
    /// do recorte, antes de qualquer escrita do draw.
    pub(super) fn ler(state: &Render, i: u16, mem: &impl Memory) -> Result<Self, &'static str> {
        let reg = |base: u16| state.reg(base + i * 0x40);
        let offset = reg(0x1B00);
        let formato = reg(0x1B04);
        let endereco = reg(0x1B08);
        let controle = reg(0x1B0C);
        let pitch = reg(0x1B10) >> 16;
        let filtro_raw = reg(0x1B14);
        let retangulo = reg(0x1B1C);
        if controle & (1 << 30) == 0 {
            return Err("estagio de textura usado pelo shader sem TX_ENABLE");
        }
        let contexto = match formato & 3 {
            1 => state.reg(0x184),
            2 => state.reg(0x188),
            _ => return Err("contexto DMA de textura invalido"),
        };
        let cubemap = formato & 4 != 0;
        let dimensoes = (formato >> 4) & 15;
        let cor = (formato >> 8) & 0xFF;
        let niveis = (formato >> 16) & 15;
        if cubemap || dimensoes != 2 || cor != LU_IMAGE_A8R8G8B8 || niveis != 1 {
            return Err("formato de textura fora do perfil LU_IMAGE_A8R8G8B8 2D de um nivel");
        }
        if endereco & 0xF != CLAMP_TO_EDGE || (endereco >> 8) & 0xF != CLAMP_TO_EDGE {
            return Err("enderecamento de textura exige clamp-to-edge neste perfil");
        }
        let gaussiano = (filtro_raw >> 13) & 7 == 2;
        let minimo = filtro((filtro_raw >> 16) & 0x3F, gaussiano)?;
        let maximo = filtro((filtro_raw >> 24) & 0xF, gaussiano)?;
        if minimo != maximo {
            return Err("filtros MIN/MAG diferentes exigem LOD, sem modelo neste perfil");
        }
        let largura = retangulo >> 16;
        let altura = retangulo & 0xFFFF;
        if largura == 0
            || altura == 0
            || u64::from(largura) * u64::from(altura) > MAX_TEXELS
            || pitch < largura * 4
            || pitch & 3 != 0
        {
            return Err("dimensoes ou pitch de textura invalidos");
        }
        let (base, limite) = clear::descriptor(mem, contexto, false)?;
        let fim =
            u64::from(offset) + u64::from(pitch) * u64::from(altura - 1) + u64::from(largura) * 4;
        if offset & 3 != 0 || fim > u64::from(limite) + 1 || u64::from(base) + fim > mem.ram_len() {
            return Err("textura fora de DMA/RAM alinhada");
        }
        let mut texels = Vec::with_capacity((largura * altura) as usize);
        for y in 0..altura {
            for x in 0..largura {
                texels.push(mem.read_ram(base + offset + y * pitch + x * 4)?);
            }
        }
        Ok(Self {
            largura,
            altura,
            filtro: minimo,
            texels,
        })
    }

    fn texel(&self, x: i64, y: i64) -> [f64; 4] {
        // Clamp-to-edge.
        let x = usize::try_from(x.clamp(0, i64::from(self.largura) - 1)).expect("clamp >= 0");
        let y = usize::try_from(y.clamp(0, i64::from(self.altura) - 1)).expect("clamp >= 0");
        let argb = self.texels[y * self.largura as usize + x];
        let canal = |deslocamento: u32| f64::from((argb >> deslocamento) & 0xFF) / 255.0;
        [canal(16), canal(8), canal(0), canal(24)]
    }

    fn bilinear(&self, u: f64, v: f64) -> [f64; 4] {
        let (pos_x, pos_y) = (u - 0.5, v - 0.5);
        let (base_x, base_y) = (pos_x.floor(), pos_y.floor());
        let (frac_x, frac_y) = (pos_x - base_x, pos_y - base_y);
        #[allow(clippy::cast_possible_truncation)]
        let (base_x, base_y) = (base_x as i64, base_y as i64);
        let [t00, t10, t01, t11] = [
            self.texel(base_x, base_y),
            self.texel(base_x + 1, base_y),
            self.texel(base_x, base_y + 1),
            self.texel(base_x + 1, base_y + 1),
        ];
        std::array::from_fn(|i| {
            (t00[i] * (1.0 - frac_x) + t10[i] * frac_x) * (1.0 - frac_y)
                + (t01[i] * (1.0 - frac_x) + t11[i] * frac_x) * frac_y
        })
    }

    /// Amostra em coordenadas de texel (`u`, `v`), devolvendo RGBA em [0, 1].
    pub(super) fn amostrar(&self, u: f64, v: f64) -> [f64; 4] {
        match self.filtro {
            #[allow(clippy::cast_possible_truncation)]
            Filtro::Nearest => self.texel(u.floor() as i64, v.floor() as i64),
            Filtro::Linear => self.bilinear(u, v),
            Filtro::Gaussiano => {
                const PESOS: [f64; 3] = [1.0, 2.0, 1.0];
                let mut soma = [0.0; 4];
                for (dy, py) in (-1..=1).zip(PESOS) {
                    for (dx, px) in (-1..=1).zip(PESOS) {
                        let amostra = self.bilinear(u + f64::from(dx), v + f64::from(dy));
                        for (s, a) in soma.iter_mut().zip(amostra) {
                            *s += a * px * py / 16.0;
                        }
                    }
                }
                soma
            }
        }
    }

    #[cfg(test)]
    pub(super) fn sintetica(largura: u32, altura: u32, texels: Vec<u32>, filtro: u8) -> Self {
        Self {
            largura,
            altura,
            filtro: match filtro {
                0 => Filtro::Nearest,
                1 => Filtro::Linear,
                _ => Filtro::Gaussiano,
            },
            texels,
        }
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::Textura;

    /// Xadrez 4×4: (x + y) par = branco opaco, ímpar = preto com alfa 0x80.
    fn xadrez(filtro: u8) -> Textura {
        let texels = (0..16)
            .map(|i| {
                if (i % 4 + i / 4) % 2 == 0 {
                    0xFFFF_FFFF
                } else {
                    0x8000_0000
                }
            })
            .collect();
        Textura::sintetica(4, 4, texels, filtro)
    }

    #[test]
    fn nearest_le_o_texel_e_clampa_nas_bordas() {
        let t = xadrez(0);
        assert_eq!(t.amostrar(0.5, 0.5), [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(t.amostrar(1.5, 0.5)[0], 0.0);
        assert_eq!(
            t.amostrar(-10.0, -10.0),
            t.amostrar(0.5, 0.5),
            "clamp-to-edge"
        );
        assert_eq!(t.amostrar(99.0, 99.0), t.amostrar(3.5, 3.5));
    }

    #[test]
    fn linear_no_centro_do_texel_e_exato_e_entre_texels_media() {
        let t = xadrez(1);
        assert_eq!(t.amostrar(0.5, 0.5), [1.0, 1.0, 1.0, 1.0]);
        let meio = t.amostrar(1.0, 0.5);
        assert_eq!(meio[0], 0.5);
        assert!((meio[3] - f64::midpoint(1.0, 128.0 / 255.0)).abs() < 1e-12);
    }

    #[test]
    fn gaussiano_soma_um_e_suaviza_o_xadrez() {
        let t = xadrez(2);
        // Centro de um texel branco: peso 4 no centro (branco), 2 em cada vizinho
        // ortogonal (pretos) e 1 em cada diagonal (brancos): (4 + 4)/16 = 0,5.
        let c = t.amostrar(1.5, 1.5);
        assert!((c[0] - 0.5).abs() < 1e-12, "{c:?}");
        // Cor uniforme permanece uniforme (os pesos somam 1).
        let liso = Textura::sintetica(2, 2, vec![0xFF10_2030; 4], 2);
        let a = liso.amostrar(1.0, 1.0);
        assert!((a[0] - 16.0 / 255.0).abs() < 1e-12 && (a[2] - 48.0 / 255.0).abs() < 1e-12);
    }
}
