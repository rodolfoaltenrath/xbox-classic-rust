use super::{CONSTANTS, Render};
use crate::pgraph::{Memory, clear};
#[derive(Debug, Clone, Copy)]
pub(super) struct Vertex {
    pub position: [f32; 4],
    pub color: [f32; 4],
    /// Saída o[9] (TEX0); `[0, 0, 0, 1]` quando o programa não a escreve.
    pub texcoord: [f32; 4],
}
type Vector = [f32; 4];
pub(super) fn attributes(
    state: &Render,
    index: u32,
    mem: &impl Memory,
) -> Result<[Vector; 16], &'static str> {
    let mut attrs = [[0.0, 0.0, 0.0, 1.0]; 16];
    for (i, attr) in attrs.iter_mut().enumerate() {
        let fmt = state.reg(0x1760 + i as u16 * 4);
        let size = (fmt >> 4) & 15;
        if size == 0 {
            continue;
        }
        if fmt & 15 != 2 || size > 4 {
            return Err("atributo exige F32 de um a quatro componentes");
        }
        let stride = fmt >> 8;
        let offset = state.reg(0x1720 + i as u16 * 4);
        let instance = state.reg(if offset >> 31 == 0 { 0x19C } else { 0x1A0 });
        let (base, limit) = clear::descriptor(mem, instance, false)?;
        let at = u64::from(offset & 0x7FFFFFFF) + u64::from(index) * u64::from(stride);
        let end = at + u64::from(size) * 4;
        if at & 3 != 0
            || end > u64::from(limit) + 1
            || u64::from(base) + end > mem.ram_len()
            || u64::from(base) + end > u64::from(u32::MAX) + 1
        {
            return Err("array de vertices fora de DMA/RAM alinhada");
        }
        for lane in 0..size {
            attr[lane as usize] = f32::from_bits(mem.read_ram(base + at as u32 + lane * 4)?);
        }
    }
    Ok(attrs)
}
struct Inputs<'a> {
    attrs: &'a [Vector; 16],
    temps: &'a [Vector; 16],
    temp_masks: &'a [u32; 16],
    constant_masks: &'a [u8],
    constants: &'a [[u32; 4]],
    v: usize,
    c: usize,
}
impl Inputs<'_> {
    /// `usadas`: faixas do operando que entram no resultado (bit 8 = x … bit 1 = w).
    /// Só elas precisam estar definidas; as outras não afetam a saída (ADR-0089).
    fn source(
        &self,
        mux: u32,
        temp: u32,
        swizzle: u32,
        neg: bool,
        usadas: u32,
    ) -> Result<Vector, &'static str> {
        if mux == 1
            && (0..4).any(|lane| {
                usadas & (8 >> lane) != 0
                    && self.temp_masks[temp as usize] & (8 >> ((swizzle >> (6 - lane * 2)) & 3))
                        == 0
            })
        {
            return Err("temporario VP lido antes de ser definido");
        }
        if mux == 3 && self.constant_masks.get(self.c) != Some(&15) {
            return Err("constante VP incompleta ou nao carregada");
        }
        let value = match mux {
            1 => *self
                .temps
                .get(temp as usize)
                .ok_or("temporario VP fora de faixa")?,
            2 => self.attrs[self.v],
            3 => self
                .constants
                .get(self.c)
                .ok_or("constante VP fora de faixa")?
                .map(f32::from_bits),
            _ => return Err("mux de entrada VP sem modelo"),
        };
        Ok(std::array::from_fn(|lane| {
            let x = value[((swizzle >> (6 - lane * 2)) & 3) as usize];
            if neg { -x } else { x }
        }))
    }
}
fn masked(dst: &mut Vector, src: Vector, mask: u32) {
    for i in 0..4 {
        if mask & (8 >> i) != 0 {
            dst[i] = src[i];
        }
    }
}
fn write_outputs(
    d3: u32,
    ilu: u32,
    result: Vector,
    temps: &mut [Vector; 16],
    outputs: &mut [Vector; 16],
    masks: &mut [u32; 16],
    temp_masks: &mut [u32; 16],
) -> Result<(), &'static str> {
    let temp = ((d3 >> 20) & 15) as usize;
    let mask = if ilu != 0 {
        (d3 >> 16) & 15
    } else {
        (d3 >> 24) & 15
    };
    masked(&mut temps[temp], result, mask);
    temp_masks[temp] |= mask;
    if temp == 12 {
        masked(&mut outputs[0], result, mask);
        masks[0] |= mask;
    }
    let omask = (d3 >> 12) & 15;
    if omask != 0 {
        if d3 & (1 << 11) == 0 || ((d3 >> 2) & 1) != u32::from(ilu != 0) || (d3 >> 3) & 0xF0 != 0 {
            return Err("saida VP constante ou mux sem modelo");
        }
        let address = ((d3 >> 3) & 15) as usize;
        masked(&mut outputs[address], result, omask);
        masks[address] |= omask;
        if address == 0 {
            masked(&mut temps[12], result, omask);
            temp_masks[12] |= omask;
        }
    }
    Ok(())
}
pub(super) fn run(state: &Render, attrs: &[Vector; 16]) -> Result<Vertex, &'static str> {
    let mut temps = [[0.0; 4]; 16];
    let mut outputs = [[0.0; 4]; 16];
    let mut temp_masks = [0; 16];
    let mut masks = [0u32; 16];
    let start = state.reg(0x1EA0) as usize;
    if start >= state.program.len() {
        return Err("inicio de VP fora de faixa");
    }
    for pc in start..state.program.len() {
        if state.program_mask[pc] != 15 {
            return Err("instrucao VP incompleta ou nao carregada");
        }
        let [d0, d1, d2, d3] = state.program[pc];
        // MAC: 1 MOV, 2 MUL, 3 ADD, 4 MAD, 5 DP3, 9 MIN, 10 MAX.
        // ILU: 2 RCP, 4 RSQ, 7 LIT (ADRs 0068 e 0089).
        let mac = (d1 >> 21) & 15;
        let ilu = (d1 >> 25) & 7;
        if d0 != 0
            || d3 & 2 != 0
            || !matches!(mac, 0..=5 | 9 | 10)
            || !matches!(ilu, 0 | 2 | 4 | 7)
            || (mac != 0 && ilu != 0)
        {
            return Err("opcode VP, indexacao relativa ou pareamento sem modelo");
        }
        let inputs = Inputs {
            attrs,
            temps: &temps,
            temp_masks: &temp_masks,
            constant_masks: &state.constant_mask,
            constants: &state.constants,
            v: ((d1 >> 9) & 15) as usize,
            c: ((d1 >> 13) & 255) as usize,
        };
        if inputs.c >= CONSTANTS && (mac != 0 || ilu != 0) {
            return Err("constante VP fora de faixa");
        }
        // Faixas usadas: escalares do ILU leem x; DP3 lê xyz; as demais operações são
        // componente a componente e usam só as faixas escritas (temporário ou saída).
        let escritas = if ilu != 0 {
            (d3 >> 16) & 15
        } else {
            ((d3 >> 24) & 15) | ((d3 >> 12) & 15)
        };
        let usadas = match (mac, ilu) {
            (5, _) => 0b1110,
            (0, _) => 0b1000,
            _ => escritas,
        };
        let a = if mac != 0 {
            inputs.source((d2 >> 26) & 3, d2 >> 28, d1 & 255, d1 & 256 != 0, usadas)?
        } else {
            [0.0; 4]
        };
        let b = if matches!(mac, 2 | 4 | 5 | 9 | 10) {
            inputs.source(
                (d2 >> 11) & 3,
                (d2 >> 13) & 15,
                (d2 >> 17) & 255,
                d2 & (1 << 25) != 0,
                usadas,
            )?
        } else {
            [0.0; 4]
        };
        let c = if matches!(mac, 3 | 4) || ilu != 0 {
            inputs.source(
                (d3 >> 28) & 3,
                ((d2 & 3) << 2) | (d3 >> 30),
                (d2 >> 2) & 255,
                d2 & (1 << 10) != 0,
                usadas,
            )?
        } else {
            [0.0; 4]
        };
        let result = calcular(mac, ilu, a, b, c)?;
        if mac != 0 || ilu != 0 {
            write_outputs(
                d3,
                ilu,
                result,
                &mut temps,
                &mut outputs,
                &mut masks,
                &mut temp_masks,
            )?;
        }
        if d3 & 1 != 0 {
            return saida(&outputs, &masks);
        }
    }
    Err("vertex program sem FINAL")
}

/// Vértice de saída no FINAL: posição o[0] e cor o[3] completas e finitas, W > 0.
fn saida(outputs: &[Vector; 16], masks: &[u32; 16]) -> Result<Vertex, &'static str> {
    if masks[0] != 15 || masks[3] != 15 {
        return Err("VP nao define posicao/cor completas");
    }
    let out = Vertex {
        position: outputs[0],
        color: outputs[3],
        texcoord: if masks[9] == 0 {
            [0.0, 0.0, 0.0, 1.0]
        } else {
            outputs[9]
        },
    };
    if out
        .position
        .into_iter()
        .chain(out.color)
        .any(|v| !v.is_finite())
        || out.position[3] <= 0.0
    {
        return Err("saida VP nao finita ou W nao positivo");
    }
    Ok(out)
}

/// Resultado de uma instrução: MAC (MOV, MUL, ADD, MAD, DP3, MIN, MAX) ou ILU
/// (RCP, RSQ, LIT) sobre os operandos já lidos.
fn calcular(mac: u32, ilu: u32, a: Vector, b: Vector, c: Vector) -> Result<Vector, &'static str> {
    Ok(match mac {
        1 => a,
        2 => std::array::from_fn(|i| a[i] * b[i]),
        3 => std::array::from_fn(|i| a[i] + c[i]),
        4 => std::array::from_fn(|i| a[i] * b[i] + c[i]),
        5 => [a[0] * b[0] + a[1] * b[1] + a[2] * b[2]; 4],
        9 => std::array::from_fn(|i| a[i].min(b[i])),
        10 => std::array::from_fn(|i| a[i].max(b[i])),
        _ if ilu == 2 => {
            if c[0] == 0.0 {
                return Err("RCP de zero fora do perfil VP");
            }
            [1.0 / c[0]; 4]
        }
        _ if ilu == 4 => {
            if c[0] == 0.0 {
                return Err("RSQ de zero fora do perfil VP");
            }
            [1.0 / c[0].abs().sqrt(); 4]
        }
        _ if ilu == 7 => lit(c),
        _ => [0.0; 4],
    })
}

/// LIT do vertex shader 1.1 (o que o perfil vp20 do cgc emite para `pow`): x = 1,
/// y = max(N·L, 0), z = N·L > 0 ? max(N·H, 0)^potência : 0, w = 1. A potência é
/// limitada a ±127,9961 como no NV2A (xemu `vsh`, mesma faixa do D3D).
fn lit(c: Vector) -> Vector {
    const POTENCIA: f32 = 127.996_09;
    let potencia = c[3].clamp(-POTENCIA, POTENCIA);
    let especular = if c[0] > 0.0 {
        pow(c[1].max(0.0), potencia)
    } else {
        0.0
    };
    [1.0, c[0].max(0.0), especular, 1.0]
}

/// `base^expoente` para `base >= 0`, em `f64` com log2/exp2 próprios: só soma,
/// multiplicação e divisão IEEE, então o resultado não depende da libm do host.
fn pow(base: f32, expoente: f32) -> f32 {
    if base == 0.0 {
        return if expoente > 0.0 {
            0.0
        } else if expoente == 0.0 {
            1.0
        } else {
            f32::INFINITY
        };
    }
    exp2(f64::from(expoente) * log2(f64::from(base))) as f32
}

/// log2 de `x > 0` normal: `x = m·2^k`, `m ∈ [√½, √2)`, série de atanh.
fn log2(x: f64) -> f64 {
    let bits = x.to_bits();
    let mut k = ((bits >> 52) & 0x7FF) as i32 - 1023;
    let mut m = f64::from_bits((bits & ((1 << 52) - 1)) | 0x3FF0_0000_0000_0000);
    if m > std::f64::consts::SQRT_2 {
        m /= 2.0;
        k += 1;
    }
    let s = (m - 1.0) / (m + 1.0);
    let s2 = s * s;
    let mut termo = s;
    let mut soma = 0.0;
    for n in (1..=23).step_by(2) {
        soma += termo / f64::from(n);
        termo *= s2;
    }
    f64::from(k) + 2.0 * soma / std::f64::consts::LN_2
}

/// 2^y: parte inteira por escala exata, fração em `[-½, ½]` por série de Taylor.
fn exp2(y: f64) -> f64 {
    if y > 1023.0 {
        return f64::INFINITY;
    }
    if y < -1074.0 {
        return 0.0;
    }
    let n = y.round_ties_even();
    let t = (y - n) * std::f64::consts::LN_2;
    let mut termo = 1.0;
    let mut soma = 1.0;
    for i in 1..=20 {
        termo *= t / f64::from(i);
        soma += termo;
    }
    soma * 2f64.powi(n as i32)
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::{lit, pow};

    #[test]
    fn pow_proprio_concorda_com_f32_em_ate_um_ulp() {
        for base in [0.001f32, 0.25, 0.5, 0.9, 0.99, 1.0, 2.0, 10.0] {
            for e in [0.5f32, 1.0, 2.0, 16.0, 64.0, -1.0, 127.9] {
                let (nosso, host) = (pow(base, e), base.powf(e));
                if host.is_finite() && host > 1e-30 {
                    assert!(
                        nosso.to_bits().abs_diff(host.to_bits()) <= 1,
                        "{base}^{e}: {nosso} × {host}"
                    );
                }
            }
        }
        assert_eq!(pow(0.0, 64.0), 0.0);
        assert_eq!(pow(0.0, 0.0), 1.0);
    }

    #[test]
    fn lit_segue_o_vertex_shader_1_1() {
        assert_eq!(lit([0.5, 0.5, 0.0, 2.0]), [1.0, 0.5, 0.25, 1.0]);
        assert_eq!(
            lit([-0.5, 0.9, 0.0, 2.0]),
            [1.0, 0.0, 0.0, 1.0],
            "N·L <= 0 zera o especular"
        );
        assert_eq!(lit([0.5, -0.5, 0.0, 2.0])[2], 0.0);
        assert_eq!(
            lit([1.0, 0.5, 0.0, 1000.0])[2],
            pow(0.5, 127.996_09),
            "potência limitada"
        );
    }
}
