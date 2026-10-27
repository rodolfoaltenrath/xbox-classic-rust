//! Register combiners do perfil `SoftwareV1` (ADR-0089).
//!
//! Semântica do perfil público do xemu fixado no ADR-0056 (`psh.c`): cada estágio
//! calcula `ab = A·B`, `cd = C·D` e `ab + cd` com mapeamento de entrada, escala/viés de
//! saída e clamp em [−1, 1]; o combiner final faz `rgb = D + mix(C, B, A)` e
//! `alpha = G`. Palavras ICW/OCW decodificadas como `parse_combiner_inputs/output`.
//!
//! Fora do recorte (recusado antes do raster): produto escalar, mux, blue-to-alpha,
//! constantes C0/C1, fog, especular V1 e os registradores especiais do final
//! (`V1+R0`, `E·F`). O caminho passthrough do triangle é o caso particular em que o
//! estágio descarta tudo e o final devolve `D = V0`.

use super::Render;

/// Valor RGBA em `f64`.
pub(super) type Rgba = [f64; 4];

/// Fontes que um fragmento oferece ao combiner.
#[derive(Debug, Clone, Copy)]
pub(super) struct Entradas {
    pub v0: Rgba,
    pub t: [Rgba; 4],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Entrada {
    registrador: u8,
    /// `true` = canal alfa (na parte RGB, replica alfa; na alfa, lê alfa em vez de azul).
    alfa: bool,
    mapeamento: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Saida {
    cd: u8,
    ab: u8,
    soma: u8,
    escala: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Metade {
    entradas: [Entrada; 4],
    saida: Saida,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Estagio {
    cor: Metade,
    alfa: Metade,
}

/// Configuração completa, decodificada uma vez por draw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Combiner {
    estagios: Vec<Estagio>,
    /// A, B, C, D, E, F, G do combiner final.
    final_: [Entrada; 7],
}

const ZERO: u8 = 0;
const V0: u8 = 4;
const T0: u8 = 8;
const R0: u8 = 0xC;
const R1: u8 = 0xD;

fn entrada(byte: u32) -> Entrada {
    Entrada {
        registrador: (byte & 0xF) as u8,
        alfa: byte & 0x10 != 0,
        mapeamento: ((byte >> 5) & 7) as u8,
    }
}

fn entradas(icw: u32) -> [Entrada; 4] {
    [
        entrada(icw >> 24),
        entrada((icw >> 16) & 0xFF),
        entrada((icw >> 8) & 0xFF),
        entrada(icw & 0xFF),
    ]
}

fn fonte_legivel(registrador: u8, final_: bool) -> Result<(), &'static str> {
    match registrador {
        ZERO | V0 | T0..=0xB | R0 | R1 => Ok(()),
        0xE | 0xF if final_ => Err("combiner final com V1+R0 ou E*F, sem modelo"),
        _ => Err("combiner le C0/C1/fog/especular, sem modelo neste perfil"),
    }
}

fn destino_valido(registrador: u8) -> Result<(), &'static str> {
    match registrador {
        ZERO | V0 | T0..=0xB | R0 | R1 => Ok(()),
        _ => Err("combiner escreve registrador sem modelo"),
    }
}

fn metade(icw: u32, ocw: u32) -> Result<Metade, &'static str> {
    let entradas = entradas(icw);
    let flags = ocw >> 12;
    if flags & 0x7 != 0 || flags & 0xC0 != 0 || ocw >> 20 != 0 {
        return Err("combiner com produto escalar, mux ou blue-to-alpha, sem modelo");
    }
    let saida = Saida {
        cd: (ocw & 0xF) as u8,
        ab: ((ocw >> 4) & 0xF) as u8,
        soma: ((ocw >> 8) & 0xF) as u8,
        escala: ((flags >> 3) & 7) as u8,
    };
    if saida.escala == 5 || saida.escala == 7 {
        return Err("escala de saida do combiner invalida");
    }
    for e in &entradas {
        fonte_legivel(e.registrador, false)?;
    }
    for d in [saida.cd, saida.ab, saida.soma] {
        destino_valido(d)?;
    }
    Ok(Metade { entradas, saida })
}

impl Combiner {
    pub(super) fn ler(state: &Render) -> Result<Self, &'static str> {
        let controle = state.reg(0x1E60);
        let n = controle & 0xFF;
        if n == 0 || n > 8 {
            return Err("numero de estagios do combiner fora de 1..8");
        }
        let mut estagios = Vec::with_capacity(n as usize);
        for i in 0..n as u16 {
            estagios.push(Estagio {
                cor: metade(state.reg(0xAC0 + i * 4), state.reg(0x1E40 + i * 4))?,
                alfa: metade(state.reg(0x260 + i * 4), state.reg(0xAA0 + i * 4))?,
            });
        }
        let [fa, fb, fc, fd] = entradas(state.reg(0x288));
        let cw1 = state.reg(0x28C);
        let [fe, ff, fg, _] = entradas(cw1);
        if cw1 & 0xFF != 0 {
            return Err("combiner final com clamp/complemento de V1+R0, sem modelo");
        }
        let final_ = [fa, fb, fc, fd, fe, ff, fg];
        for entrada in &final_ {
            fonte_legivel(entrada.registrador, true)?;
        }
        Ok(Self { estagios, final_ })
    }

    /// Cor final de um fragmento, RGBA em [0, 1].
    pub(super) fn avaliar(&self, entradas: &Entradas) -> Rgba {
        let mut regs = [[0.0; 4]; 16];
        regs[V0 as usize] = entradas.v0;
        for (i, t) in entradas.t.iter().enumerate() {
            regs[T0 as usize + i] = *t;
        }
        // xemu: r0.a começa com t0.a quando o estágio 0 tem textura (aqui t0 já é
        // (0, 0, 0, 1) sem textura, o que reproduz o `r0.a = 1.0` do outro caso).
        regs[R0 as usize][3] = entradas.t[0][3];
        for estagio in &self.estagios {
            let cor = calcular(&estagio.cor, &regs, false);
            let alfa = calcular(&estagio.alfa, &regs, true);
            // Escritas depois das leituras das duas metades, como no shader gerado.
            escrever(&mut regs, estagio.cor.saida, cor, false);
            escrever(&mut regs, estagio.alfa.saida, alfa, true);
        }
        let ler = |entrada: Entrada, alfa_final: bool| -> Rgba {
            ler_entrada(&regs, entrada, alfa_final).map(|x| mapear(x, entrada.mapeamento))
        };
        let [fa, fb, fc, fd, _, _, fg] = self.final_;
        let (fa, fb, fc, fd) = (
            ler(fa, false),
            ler(fb, false),
            ler(fc, false),
            ler(fd, false),
        );
        let alfa = ler(fg, true)[3];
        let rgb: [f64; 3] = std::array::from_fn(|i| fd[i] + fc[i] * (1.0 - fa[i]) + fb[i] * fa[i]);
        [rgb[0], rgb[1], rgb[2], alfa].map(|x| x.clamp(0.0, 1.0))
    }
}

fn mapear(x: f64, mapeamento: u8) -> f64 {
    match mapeamento {
        0 => x.max(0.0),
        1 => 1.0 - x.clamp(0.0, 1.0),
        2 => 2.0 * x.max(0.0) - 1.0,
        3 => -2.0 * x.max(0.0) + 1.0,
        4 => x.max(0.0) - 0.5,
        5 => -x.max(0.0) + 0.5,
        6 => x,
        _ => -x,
    }
}

/// Lê uma entrada já com o canal escolhido: na parte RGB, `.rgb` ou `.aaa`; na alfa
/// (e em G do final), `.b` ou `.a` replicados.
fn ler_entrada(regs: &[Rgba; 16], entrada: Entrada, alfa: bool) -> Rgba {
    let r = regs[entrada.registrador as usize];
    match (alfa, entrada.alfa) {
        (false, false) => r,
        (true, false) => [r[2]; 4],
        (_, true) => [r[3]; 4],
    }
}

/// `(ab, cd, soma)` de uma metade, já com escala/viés.
fn calcular(metade: &Metade, regs: &[Rgba; 16], alfa: bool) -> (Rgba, Rgba, Rgba) {
    let valores = metade
        .entradas
        .map(|entrada| ler_entrada(regs, entrada, alfa).map(|x| mapear(x, entrada.mapeamento)));
    let escala = |x: f64| match metade.saida.escala {
        0 => x,
        1 => x - 0.5,
        2 => x * 2.0,
        3 => (x - 0.5) * 2.0,
        4 => x * 4.0,
        _ => x / 2.0,
    };
    let ab: Rgba = std::array::from_fn(|i| valores[0][i] * valores[1][i]);
    let cd: Rgba = std::array::from_fn(|i| valores[2][i] * valores[3][i]);
    let soma: Rgba = std::array::from_fn(|i| ab[i] + cd[i]);
    (ab.map(escala), cd.map(escala), soma.map(escala))
}

fn escrever(regs: &mut [Rgba; 16], saida: Saida, (ab, cd, soma): (Rgba, Rgba, Rgba), alfa: bool) {
    for (destino, valor) in [(saida.ab, ab), (saida.cd, cd), (saida.soma, soma)] {
        if destino == ZERO {
            continue;
        }
        let r = &mut regs[destino as usize];
        if alfa {
            r[3] = valor[3].clamp(-1.0, 1.0);
        } else {
            for i in 0..3 {
                r[i] = valor[i].clamp(-1.0, 1.0);
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    fn render(regs: &[(u16, u32)]) -> Render {
        let mut r = Render::default();
        for &(m, v) in regs {
            r.registers.insert(m, v);
        }
        r
    }

    /// Passthrough do triangle: estágio descartado, final D = V0, G = V0.a.
    fn passthrough() -> Render {
        render(&[
            (0x260, 0x1010_1010),
            (0xAA0, 0),
            (0xAC0, 0x1010_1010),
            (0x1E40, 0),
            (0x1E60, 1),
            (0x288, 4),
            (0x28C, 0x1400),
        ])
    }

    /// Mesh: V0 ← T0 × V0 em RGB e alfa (`SIGNED_IDENTITY`), final D = V0.
    fn mesh() -> Render {
        render(&[
            (0x260, 0xD8D4_1010),
            (0xAA0, 0x40),
            (0xAC0, 0xC8C4_0000),
            (0x1E40, 0x40),
            (0x1E60, 1),
            (0x288, 4),
            (0x28C, 0x1400),
        ])
    }

    const COR: Rgba = [0.25, 0.5, 0.75, 0.6];
    const TEXEL: Rgba = [0.5, 1.0, 0.2, 0.5];

    #[test]
    fn passthrough_devolve_a_cor_do_vertice() {
        let c = Combiner::ler(&passthrough()).unwrap();
        let e = Entradas {
            v0: COR,
            t: [[0.0, 0.0, 0.0, 1.0]; 4],
        };
        assert_eq!(c.avaliar(&e), COR);
    }

    #[test]
    fn mesh_multiplica_textura_pela_cor() {
        let c = Combiner::ler(&mesh()).unwrap();
        let e = Entradas {
            v0: COR,
            t: [TEXEL, [0.0, 0.0, 0.0, 1.0], [0.0; 4], [0.0; 4]],
        };
        let r = c.avaliar(&e);
        let esperado = [0.125, 0.5, 0.15, 0.3];
        for i in 0..4 {
            assert!((r[i] - esperado[i]).abs() < 1e-12, "{r:?}");
        }
    }

    #[test]
    fn mapeamentos_e_escalas_seguem_o_xemu() {
        assert_eq!(mapear(-0.5, 0), 0.0);
        assert_eq!(mapear(0.25, 1), 0.75);
        assert_eq!(mapear(0.75, 2), 0.5);
        assert_eq!(mapear(0.75, 3), -0.5);
        assert_eq!(mapear(0.75, 4), 0.25);
        assert_eq!(mapear(0.75, 5), -0.25);
        assert_eq!(mapear(-0.75, 6), -0.75);
        assert_eq!(mapear(0.75, 7), -0.75);
        // Estágio com SHIFTLEFT_1 (escala 2) satura em 1 ao escrever.
        let mut r = mesh();
        r.registers.insert(0x1E40, 0x40 | 2 << 15);
        let c = Combiner::ler(&r).unwrap();
        let e = Entradas {
            v0: [0.8; 4],
            t: [[0.9; 4], [0.0; 4], [0.0; 4], [0.0; 4]],
        };
        assert_eq!(c.avaliar(&e)[0], 1.0);
    }

    #[test]
    fn recursos_sem_modelo_sao_recusados() {
        for (reg, valor) in [
            (0x1E40, 0x40 | 1 << 13), // AB dot product
            (0x1E40, 0x40 | 1 << 14), // mux
            (0x1E40, 0x40 | 1 << 19), // blue to alpha
            (0xAC0, 0xC1C4_0000),     // C0
            (0x28C, 0x1401),          // flags do final
            (0x288, 0x0E),            // D = V1+R0
            (0x1E60, 0),              // nenhum estágio
        ] {
            let mut r = mesh();
            r.registers.insert(reg, valor);
            assert!(Combiner::ler(&r).is_err(), "aceitou {reg:#X}={valor:#X}");
        }
    }
}
