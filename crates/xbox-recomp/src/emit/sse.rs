//! Emissão de SSE escalar de precisão simples.
//!
//! Registradores XMM vivem em `s.xmm[n]` como quatro palavras `u32` (bits crus). Toda
//! leitura de memória acontece **antes** de qualquer escrita em registrador ou memória:
//! uma falta de página não deixa efeito parcial. Escritas de 128 bits usam
//! `escrever_bytes`, que valida o intervalo inteiro antes de tocar a RAM.

use super::{address, line, read, write_operand};
use crate::ir::{Sse, SseArith, SseLogic, XmmRm};

pub(super) fn emit(out: &mut String, sse: &Sse) {
    match sse {
        Sse::MoveScalar { dst, src } => move_scalar(out, dst, src),
        Sse::MovePacked { dst, src, aligned } => {
            load_lanes(out, src, *aligned);
            store_lanes(out, dst, *aligned);
        }
        Sse::Logic { op, dst, src } => {
            // Memória de 128 bits exige alinhamento nas versões legadas (ANDPS/XORPS...).
            load_lanes(out, src, true);
            let expression = match op {
                SseLogic::And => "s.xmm[{d}][k] & lanes[k]",
                SseLogic::AndNot => "!s.xmm[{d}][k] & lanes[k]",
                SseLogic::Or => "s.xmm[{d}][k] | lanes[k]",
                SseLogic::Xor => "s.xmm[{d}][k] ^ lanes[k]",
            }
            .replace("{d}", &dst.to_string());
            line(
                out,
                &format!("for k in 0..4 {{ s.xmm[{dst}][k] = {expression}; }}"),
            );
        }
        Sse::Arith { op, dst, src } => {
            line(out, &format!("let b = f32::from_bits({});", read_u32(src)));
            line(out, &format!("let a = f32::from_bits(s.xmm[{dst}][0]);"));
            let result = match op {
                SseArith::Add => "a + b",
                SseArith::Sub => "a - b",
                SseArith::Mul => "a * b",
                SseArith::Div => "a / b",
                SseArith::Sqrt => "b.sqrt()",
                SseArith::Min => "xbox_recomp::runtime::sse::minss(a, b)",
                SseArith::Max => "xbox_recomp::runtime::sse::maxss(a, b)",
            };
            line(out, &format!("s.xmm[{dst}][0] = ({result}).to_bits();"));
        }
        Sse::FromInt { dst, src } => {
            line(out, &format!("let value = {}.cast_signed();", read(src)));
            line(out, &format!("s.xmm[{dst}][0] = (value as f32).to_bits();"));
        }
        Sse::ToInt { dst, src, truncate } => {
            line(
                out,
                &format!("let value = f32::from_bits({});", read_u32(src)),
            );
            let convert = if *truncate { "cvttss2si" } else { "cvtss2si" };
            line(
                out,
                &format!("let result = xbox_recomp::runtime::sse::{convert}(value);"),
            );
            write_operand(out, dst, "result");
        }
        Sse::Compare { left, right } => {
            line(
                out,
                &format!("let b = f32::from_bits({});", read_u32(right)),
            );
            line(out, &format!("let a = f32::from_bits(s.xmm[{left}][0]);"));
            line(
                out,
                "s.eflags = xbox_recomp::runtime::sse::compare_flags(s.eflags, a, b);",
            );
        }
    }
}

/// Expressão `u32` da palavra baixa de um registrador XMM ou de 4 bytes de memória.
fn read_u32(src: &XmmRm) -> String {
    match src {
        XmmRm::Xmm(r) => format!("s.xmm[{r}][0]"),
        XmmRm::Mem(operand) => {
            format!("bus.ler_u32(GuestVirt({}))?", address(operand, "s.regs[4]"))
        }
    }
}

fn move_scalar(out: &mut String, dst: &XmmRm, src: &XmmRm) {
    match (dst, src) {
        // Registrador→registrador preserva as três palavras altas do destino.
        (XmmRm::Xmm(d), XmmRm::Xmm(r)) => line(out, &format!("s.xmm[{d}][0] = s.xmm[{r}][0];")),
        // Carga da memória zera as palavras altas.
        (XmmRm::Xmm(d), XmmRm::Mem(_)) => {
            line(out, &format!("let value = {};", read_u32(src)));
            line(out, &format!("s.xmm[{d}] = [value, 0, 0, 0];"));
        }
        (XmmRm::Mem(operand), XmmRm::Xmm(r)) => {
            line(out, &format!("let value = s.xmm[{r}][0];"));
            line(
                out,
                &format!("let address = {};", address(operand, "s.regs[4]")),
            );
            line(out, "bus.escrever_u32(GuestVirt(address), value)?;");
        }
        (XmmRm::Mem(_), XmmRm::Mem(_)) => unreachable!("MOVSS mem,mem não existe"),
    }
}

/// Deixa `lanes: [u32; 4]` com o conteúdo de 128 bits da origem.
fn load_lanes(out: &mut String, src: &XmmRm, aligned: bool) {
    match src {
        XmmRm::Xmm(r) => line(out, &format!("let lanes = s.xmm[{r}];")),
        XmmRm::Mem(operand) => {
            line(
                out,
                &format!("let address = {};", address(operand, "s.regs[4]")),
            );
            if aligned {
                line(
                    out,
                    "xbox_recomp::runtime::sse::check_aligned(s.eip, address)?;",
                );
            }
            line(out, "let mut raw = [0u8; 16];");
            line(out, "bus.ler_bytes(GuestVirt(address), &mut raw)?;");
            line(
                out,
                "let lanes = [0, 4, 8, 12].map(|i| u32::from_le_bytes([raw[i], raw[i + 1], raw[i + 2], raw[i + 3]]));",
            );
        }
    }
}

/// Grava `lanes` em registrador XMM ou, de forma atômica, em 16 bytes de memória.
fn store_lanes(out: &mut String, dst: &XmmRm, aligned: bool) {
    match dst {
        XmmRm::Xmm(d) => line(out, &format!("s.xmm[{d}] = lanes;")),
        XmmRm::Mem(operand) => {
            line(
                out,
                &format!("let address = {};", address(operand, "s.regs[4]")),
            );
            if aligned {
                line(
                    out,
                    "xbox_recomp::runtime::sse::check_aligned(s.eip, address)?;",
                );
            }
            line(out, "let mut raw = [0u8; 16];");
            line(
                out,
                "for (i, lane) in lanes.iter().enumerate() { raw[i * 4..i * 4 + 4].copy_from_slice(&lane.to_le_bytes()); }",
            );
            line(out, "bus.escrever_bytes(GuestVirt(address), &raw)?;");
        }
    }
}
