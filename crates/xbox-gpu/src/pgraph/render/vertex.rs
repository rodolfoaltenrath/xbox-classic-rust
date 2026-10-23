use super::{CONSTANTS, Render};
use crate::pgraph::{Memory, clear};
#[derive(Debug, Clone, Copy)]
pub(super) struct Vertex {
    pub position: [f32; 4],
    pub color: [f32; 4],
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
    fn source(&self, mux: u32, temp: u32, swizzle: u32, neg: bool) -> Result<Vector, &'static str> {
        if mux == 1
            && (0..4).any(|lane| {
                self.temp_masks[temp as usize] & (8 >> ((swizzle >> (6 - lane * 2)) & 3)) == 0
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
        let mac = (d1 >> 21) & 15;
        let ilu = (d1 >> 25) & 7;
        if d0 != 0 || d3 & 2 != 0 || mac > 4 || !matches!(ilu, 0 | 2) || (mac != 0 && ilu != 0) {
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
        let a = if mac != 0 {
            inputs.source((d2 >> 26) & 3, d2 >> 28, d1 & 255, d1 & 256 != 0)?
        } else {
            [0.0; 4]
        };
        let b = if matches!(mac, 2 | 4) {
            inputs.source(
                (d2 >> 11) & 3,
                (d2 >> 13) & 15,
                (d2 >> 17) & 255,
                d2 & (1 << 25) != 0,
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
            )?
        } else {
            [0.0; 4]
        };
        let result: Vector = match mac {
            1 => a,
            2 => std::array::from_fn(|i| a[i] * b[i]),
            3 => std::array::from_fn(|i| a[i] + c[i]),
            4 => std::array::from_fn(|i| a[i] * b[i] + c[i]),
            _ if ilu == 2 => {
                if c[0] == 0.0 {
                    return Err("RCP de zero fora do perfil VP");
                }
                [1.0 / c[0]; 4]
            }
            _ => [0.0; 4],
        };
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
            if masks[0] != 15 || masks[3] != 15 {
                return Err("VP nao define posicao/cor completas");
            }
            let out = Vertex {
                position: outputs[0],
                color: outputs[3],
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
            return Ok(out);
        }
    }
    Err("vertex program sem FINAL")
}
