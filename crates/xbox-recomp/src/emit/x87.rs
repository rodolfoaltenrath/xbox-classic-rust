//! Emissão de x87: carga e armazenamento sobre `s.fpu` (ADR-0053).
//!
//! A leitura de memória precede qualquer efeito na pilha, e a escrita precede o `pop`:
//! uma falta de página ou um sobre/subfluxo não deixa efeito parcial.

use super::{address, line};
use crate::ir::{X87, X87Constant};

pub(super) fn emit(out: &mut String, x87: &X87) {
    match x87 {
        X87::LoadInteger { source, bytes } => {
            load(out, &address(source, "s.regs[4]"), *bytes);
            let signed = match bytes {
                2 => "i64::from(i16::from_le_bytes([raw[0], raw[1]]))".to_owned(),
                4 => "i64::from(i32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))".to_owned(),
                _ => "i64::from_le_bytes(raw)".to_owned(),
            };
            line(
                out,
                &format!(
                    "s.fpu.push(xbox_recomp::runtime::x87::Extended::from_i64({signed})).map_err(Error::from)?;"
                ),
            );
        }
        X87::LoadFloat { source, bytes } => {
            load(out, &address(source, "s.regs[4]"), *bytes);
            let value = match bytes {
                4 => "from_f32_bits(u32::from_le_bytes(raw))".to_owned(),
                8 => "from_f64_bits(u64::from_le_bytes(raw))".to_owned(),
                _ => "from_bytes(raw)".to_owned(),
            };
            line(
                out,
                &format!(
                    "s.fpu.push(xbox_recomp::runtime::x87::Extended::{value}).map_err(Error::from)?;"
                ),
            );
        }
        X87::LoadConstant(constant) => {
            let name = match constant {
                X87Constant::Zero => "ZERO",
                X87Constant::One => "ONE",
            };
            line(
                out,
                &format!(
                    "s.fpu.push(xbox_recomp::runtime::x87::Extended::{name}).map_err(Error::from)?;"
                ),
            );
        }
        X87::Store { dst, bytes, pop } => {
            line(out, "let top = s.fpu.peek().map_err(Error::from)?;");
            line(
                out,
                &format!("let address = {};", address(dst, "s.regs[4]")),
            );
            let raw = match bytes {
                4 => "top.to_f32_bits().to_le_bytes().to_vec()",
                8 => "top.to_f64_bits().to_le_bytes().to_vec()",
                _ => "top.to_bytes().to_vec()",
            };
            line(out, &format!("let raw = {raw};"));
            line(out, "bus.escrever_bytes(GuestVirt(address), &raw)?;");
            if *pop {
                line(out, "s.fpu.pop().map_err(Error::from)?;");
            }
        }
    }
}

fn load(out: &mut String, address: &str, bytes: u32) {
    line(out, &format!("let address = {address};"));
    line(out, &format!("let mut raw = [0u8; {bytes}];"));
    line(out, "bus.ler_bytes(GuestVirt(address), &mut raw)?;");
}
