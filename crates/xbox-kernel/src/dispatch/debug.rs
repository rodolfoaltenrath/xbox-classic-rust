//! `DbgPrint`: o `printf` do kernel, escrito num transcrito do host (ADR-0100).
//!
//! O guest chama `ULONG DbgPrint(PCSTR Format, ...)` (cdecl, variádica; o nxdk fixado
//! documenta o retorno como "um código de status"). O texto formatado fica num transcrito
//! limitado que o host consulta; ele é **diagnóstico**, não estado do console, e por isso
//! não entra em snapshot: restaurar um snapshot não reproduz o que foi impresso antes.
//!
//! O formatador cobre as conversões de 32 bits que os títulos usam e recusa o resto como
//! variante, em vez de imprimir algo errado: `%d %i %u %x %X %c %s %p %%`, com `-`, `0`,
//! largura e o modificador `l`. Não há ponto flutuante, 64 bits (`%I64`, `%ll`), `%n` nem
//! strings largas.

use xbox_mem::{Bus, GuestVirt};

use super::threads::variant;
use super::{Atendimento, Resultado, STATUS_SUCCESS, ram_range};
use crate::imports::Call;

/// Bytes do transcrito; o que passar disso é descartado e contado.
pub const MAX_DEBUG_OUTPUT: usize = 64 * 1024;
/// Maior formato aceito, com o NUL.
const MAX_FORMAT: usize = 512;
/// Maior string `%s` aceita, sem o NUL.
const MAX_STRING: usize = 1024;

#[derive(Debug, Default)]
pub(super) struct DebugOutput {
    bytes: Vec<u8>,
    dropped: u64,
}

impl DebugOutput {
    fn append(&mut self, text: &[u8]) {
        let room = MAX_DEBUG_OUTPUT.saturating_sub(self.bytes.len());
        let kept = text.len().min(room);
        self.bytes.extend_from_slice(&text[..kept]);
        self.dropped += (text.len() - kept) as u64;
    }
}

fn read_c_string(bus: &Bus, address: u32, limit: usize) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    for offset in 0..=limit {
        let at = address.checked_add(u32::try_from(offset).ok()?)?;
        if !ram_range(bus, at, 1, xbox_mem::Acesso::Leitura) {
            return None;
        }
        match bus.ler_u8(GuestVirt(at)).ok()? {
            0 => return Some(out),
            byte => out.push(byte),
        }
    }
    None
}

fn pad(out: &mut Vec<u8>, digits: &[u8], width: usize, left: bool, zero: bool) {
    let fill = width.saturating_sub(digits.len());
    if left {
        out.extend_from_slice(digits);
        out.extend(std::iter::repeat_n(b' ', fill));
    } else if zero {
        // O zero de preenchimento vem depois do sinal.
        let (sign, rest) = match digits.first() {
            Some(b'-') => (&digits[..1], &digits[1..]),
            _ => (&digits[..0], digits),
        };
        out.extend_from_slice(sign);
        out.extend(std::iter::repeat_n(b'0', fill));
        out.extend_from_slice(rest);
    } else {
        out.extend(std::iter::repeat_n(b' ', fill));
        out.extend_from_slice(digits);
    }
}

impl super::Dispatcher {
    /// `ULONG DbgPrint(PCSTR Format, ...)`
    pub(super) fn dbg_print(&mut self, bus: &mut Bus, call: &Call) -> Resultado {
        let format_at = call.argument(bus, 0)?;
        let Some(format) = read_c_string(bus, format_at, MAX_FORMAT - 1) else {
            return Err(variant(
                bus,
                call,
                "formato fora da RAM ou sem NUL em 512 bytes",
            ));
        };
        let mut out = Vec::new();
        let mut next_argument = 1u32;
        let mut i = 0;
        while i < format.len() {
            if format[i] != b'%' {
                out.push(format[i]);
                i += 1;
                continue;
            }
            i += 1;
            let (mut left, mut zero) = (false, false);
            while let Some(&flag) = format.get(i) {
                match flag {
                    b'-' => left = true,
                    b'0' => zero = true,
                    _ => break,
                }
                i += 1;
            }
            let mut width = 0usize;
            while let Some(digit) = format.get(i).filter(|b| b.is_ascii_digit()) {
                width = width * 10 + usize::from(digit - b'0');
                if width > 512 {
                    return Err(variant(bus, call, "largura de campo acima de 512"));
                }
                i += 1;
            }
            if format.get(i) == Some(&b'l') {
                i += 1;
            }
            let Some(&conversion) = format.get(i) else {
                return Err(variant(bus, call, "conversão incompleta no fim do formato"));
            };
            i += 1;
            if conversion == b'%' {
                out.push(b'%');
                continue;
            }
            let value = call.argument(bus, next_argument)?;
            next_argument += 1;
            let text: Vec<u8> = match conversion {
                b'd' | b'i' => value.cast_signed().to_string().into_bytes(),
                b'u' => value.to_string().into_bytes(),
                b'x' => format!("{value:x}").into_bytes(),
                b'X' => format!("{value:X}").into_bytes(),
                // `%p` em x86 de 32 bits: oito dígitos hexadecimais maiúsculos.
                b'p' => format!("{value:08X}").into_bytes(),
                b'c' => vec![value as u8],
                b's' => {
                    let Some(string) = (value != 0)
                        .then(|| read_c_string(bus, value, MAX_STRING))
                        .flatten()
                    else {
                        return Err(variant(
                            bus,
                            call,
                            "%s com ponteiro nulo, fora da RAM ou sem NUL em 1024 bytes",
                        ));
                    };
                    string
                }
                _ => {
                    return Err(variant(
                        bus,
                        call,
                        "conversão do formato fora do perfil (aceitas: d i u x X c s p %)",
                    ));
                }
            };
            pad(
                &mut out,
                &text,
                width,
                left,
                zero && !left && conversion != b's',
            );
        }
        self.debug.append(&out);
        Ok(Atendimento {
            eax: STATUS_SUCCESS,
        })
    }

    /// Tudo que o guest imprimiu com `DbgPrint` (até [`MAX_DEBUG_OUTPUT`] bytes).
    pub fn debug_output(&self) -> &[u8] {
        &self.debug.bytes
    }

    /// Bytes que passaram do limite do transcrito e foram descartados.
    pub fn debug_output_dropped(&self) -> u64 {
        self.debug.dropped
    }
}
