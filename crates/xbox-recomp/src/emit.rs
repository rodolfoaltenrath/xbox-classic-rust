//! Emissão de blocos Rust. O executável gerado não decodifica instruções x86.

use std::fmt::Write;

use crate::analysis::Program;
use crate::ir::{Binary, Operand, Operation, Unary, Width};

/// Gera módulo Rust com `run(state, bus, orçamento_de_blocos)`.
/// Saídas de orçamento acontecem somente em fronteiras de blocos.
pub fn rust_source(program: &Program) -> String {
    let mut out = String::from(
        "// Gerado por xbox-recomp. Artefato local.\n\
use xbox_recomp::{Error, Result};\n\
#[allow(unused_imports)]\n\
use xbox_recomp::runtime::{State, Exit, RETURN_SENTINEL, arithmetic_flags_width, logic_flags_width, sign_extend, condition_met};\n\
#[allow(unused_imports)]\n\
use xbox_recomp::ir::{Width, Condition};\n\
use xbox_mem::{Bus, GuestVirt, Contexto};\n\
#[allow(unused_variables)]\n\
pub fn run(s: &mut State, bus: &mut Bus, blocks: u64) -> Result<Exit> {\n\
    if s.returned { return Ok(Exit::Returned); }\n\
    for _ in 0..blocks {\n\
        match s.eip {\n",
    );
    for (start, instructions) in &program.blocks {
        line(&mut out, &format!("0x{start:08X} => {{"));
        for i in instructions {
            line(&mut out, &format!("s.eip = 0x{:08X};", i.pc));
            line(
                &mut out,
                "bus.definir_contexto(Contexto { pc: Some(GuestVirt(s.eip)), thread: None });",
            );
            emit_operation(&mut out, &i.operation, i.next);
            line(&mut out, "s.retired = s.retired.wrapping_add(1);");
        }
        line(&mut out, "if s.returned { return Ok(Exit::Returned); }");
        line(&mut out, "}");
    }
    out.push_str("            pc => return Err(Error::UnknownBlock(pc)),\n        }\n    }\n    Ok(Exit::Budget)\n}\n");
    out
}

fn emit_operation(out: &mut String, operation: &Operation, next: u32) {
    match operation {
        Operation::Binary(op, dst, src) => emit_binary(out, *op, dst, src),
        Operation::Unary(op, dst) => emit_unary(out, *op, dst),
        Operation::Extend { signed, dst, src } => {
            line(out, &format!("let b = {};", read(src)));
            let value = if *signed {
                format!(
                    "sign_extend(b, Width::{:?})",
                    src.width().unwrap_or(Width::Dword)
                )
            } else {
                "b".to_owned()
            };
            write_operand(out, dst, &value);
        }
        Operation::Lea {
            dst,
            address: operand,
        } => {
            line(
                out,
                &format!("let value = {};", address(operand, "s.regs[4]")),
            );
            write_operand(out, dst, "value");
        }
        Operation::Push(src, width) => {
            // PUSH ESP armazena o valor antes do decremento (Intel SDM, PUSH).
            line(out, &format!("let value = {};", read(src)));
            line(
                out,
                &format!("let address = s.regs[4].wrapping_sub({});", width.bytes()),
            );
            write_memory(out, *width, "value");
            line(out, "s.regs[4] = address;");
        }
        Operation::Pop(dst, width) => {
            line(
                out,
                &format!("let value = {};", read_memory(*width, "s.regs[4]")),
            );
            line(
                out,
                &format!("let next_esp = s.regs[4].wrapping_add({});", width.bytes()),
            );
            // POP [ESP+...] usa ESP incrementado. O estado é comprometido depois
            // da escrita; POP ESP substitui o incremento pelo valor desempilhado.
            prepare_address(out, dst, "next_esp");
            if matches!(dst, Operand::Mem { .. }) {
                write_operand(out, dst, "value");
            }
            line(out, "s.regs[4] = next_esp;");
            if !matches!(dst, Operand::Mem { .. }) {
                write_operand(out, dst, "value");
            }
        }
        Operation::Leave => {
            line(out, "let value = bus.ler_u32(GuestVirt(s.regs[5]))?;");
            line(out, "s.regs[4] = s.regs[5].wrapping_add(4);");
            line(out, "s.regs[5] = value;");
        }
        Operation::Jump(target) => {
            line(out, &format!("s.eip = 0x{target:08X};"));
            return;
        }
        Operation::Branch { condition, target } => {
            line(
                out,
                &format!(
                    "s.eip = if condition_met(Condition::{condition:?}, s.eflags) {{ 0x{target:08X} }} else {{ 0x{next:08X} }};"
                ),
            );
            return;
        }
        Operation::Call(target) => {
            line(out, "let esp = s.regs[4].wrapping_sub(4);");
            line(
                out,
                &format!("bus.escrever_u32(GuestVirt(esp), 0x{next:08X})?;"),
            );
            line(out, "s.regs[4] = esp;");
            line(out, &format!("s.eip = 0x{target:08X};"));
            return;
        }
        Operation::Ret(bytes) => {
            line(out, "let target = bus.ler_u32(GuestVirt(s.regs[4]))?;");
            line(
                out,
                &format!(
                    "s.regs[4] = s.regs[4].wrapping_add({});",
                    u32::from(*bytes) + 4
                ),
            );
            line(out, "s.eip = target;");
            line(out, "s.returned = target == RETURN_SENTINEL;");
            return;
        }
        Operation::Nop => {}
    }
    line(out, &format!("s.eip = 0x{next:08X};"));
}

fn emit_binary(out: &mut String, op: Binary, dst: &Operand, src: &Operand) {
    prepare_address(out, dst, "s.regs[4]");
    line(out, &format!("let b = {};", read(src)));
    if op == Binary::Mov {
        write_operand(out, dst, "b");
        return;
    }
    let width = dst.width().unwrap_or(Width::Dword);
    line(out, &format!("let a = {};", read(dst)));
    let expr = match op {
        Binary::Add => "a.wrapping_add(b)",
        Binary::Sub | Binary::Cmp => "a.wrapping_sub(b)",
        Binary::Xor => "a ^ b",
        Binary::And | Binary::Test => "a & b",
        Binary::Or => "a | b",
        Binary::Mov => unreachable!(),
    };
    line(
        out,
        &format!("let result = ({expr}) & 0x{:08X}u32;", width.mask()),
    );
    if !matches!(op, Binary::Cmp | Binary::Test) {
        write_operand(out, dst, "result");
    }
    if matches!(op, Binary::Xor | Binary::And | Binary::Or | Binary::Test) {
        line(
            out,
            &format!("s.eflags = logic_flags_width(s.eflags, result, Width::{width:?});"),
        );
    } else {
        line(
            out,
            &format!(
                "s.eflags = arithmetic_flags_width(s.eflags, a, b, result, {}, Width::{width:?});",
                matches!(op, Binary::Sub | Binary::Cmp)
            ),
        );
    }
}

fn emit_unary(out: &mut String, op: Unary, dst: &Operand) {
    let width = dst.width().unwrap_or(Width::Dword);
    prepare_address(out, dst, "s.regs[4]");
    line(out, &format!("let a = {};", read(dst)));
    let expr = match op {
        Unary::Inc => "a.wrapping_add(1)",
        Unary::Dec => "a.wrapping_sub(1)",
        Unary::Neg => "0u32.wrapping_sub(a)",
        Unary::Not => "!a",
    };
    line(
        out,
        &format!("let result = ({expr}) & 0x{:08X}u32;", width.mask()),
    );
    write_operand(out, dst, "result");
    match op {
        Unary::Inc | Unary::Dec => line(
            out,
            &format!(
                "s.eflags = (arithmetic_flags_width(s.eflags, a, 1, result, {}, Width::{width:?}) & !1) | (s.eflags & 1);",
                op == Unary::Dec
            ),
        ),
        Unary::Neg => line(
            out,
            &format!(
                "s.eflags = arithmetic_flags_width(s.eflags, 0, a, result, true, Width::{width:?});"
            ),
        ),
        Unary::Not => {}
    }
}

fn line(out: &mut String, text: &str) {
    // Escrever numa String é infalível; nenhum erro de dado do usuário é ocultado.
    let _ = writeln!(out, "                {text}");
}

fn read(operand: &Operand) -> String {
    match operand {
        Operand::Reg(r) => format!("s.regs[{r}]"),
        Operand::Part {
            index,
            shift,
            width,
        } => format!("(s.regs[{index}] >> {shift}) & 0x{:08X}u32", width.mask()),
        Operand::Imm(v) => format!("0x{v:08X}u32"),
        Operand::Mem { width, .. } => read_memory(*width, &address(operand, "s.regs[4]")),
    }
}

fn read_memory(width: Width, address: &str) -> String {
    match width {
        Width::Byte => format!("u32::from(bus.ler_u8(GuestVirt({address}))?)"),
        Width::Word => format!("u32::from(bus.ler_u16(GuestVirt({address}))?)"),
        Width::Dword => format!("bus.ler_u32(GuestVirt({address}))?"),
    }
}

fn address(operand: &Operand, esp: &str) -> String {
    let Operand::Mem {
        base,
        index,
        scale,
        displacement,
        ..
    } = operand
    else {
        unreachable!()
    };
    let reg = |r| {
        if r == 4 {
            esp.to_owned()
        } else {
            format!("s.regs[{r}]")
        }
    };
    let mut expr = format!("0x{displacement:08X}u32");
    if let Some(r) = base {
        let _ = write!(expr, ".wrapping_add({})", reg(*r));
    }
    if let Some(r) = index {
        let _ = write!(expr, ".wrapping_add({}.wrapping_mul({scale}))", reg(*r));
    }
    expr
}

fn prepare_address(out: &mut String, operand: &Operand, esp: &str) {
    if matches!(operand, Operand::Mem { .. }) {
        line(out, &format!("let address = {};", address(operand, esp)));
    }
}

fn write_memory(out: &mut String, width: Width, value: &str) {
    match width {
        Width::Byte => line(
            out,
            &format!("bus.escrever_u8(GuestVirt(address), ({value}) as u8)?;"),
        ),
        Width::Word => line(
            out,
            &format!("bus.escrever_u16(GuestVirt(address), ({value}) as u16)?;"),
        ),
        Width::Dword => line(
            out,
            &format!("bus.escrever_u32(GuestVirt(address), {value})?;"),
        ),
    }
}

fn write_operand(out: &mut String, operand: &Operand, value: &str) {
    match operand {
        Operand::Reg(r) => line(out, &format!("s.regs[{r}] = {value};")),
        Operand::Part {
            index,
            shift,
            width,
        } => line(
            out,
            &format!(
                "s.regs[{index}] = (s.regs[{index}] & !0x{:08X}u32) | ((({value}) & 0x{:08X}u32) << {shift});",
                width.mask() << shift,
                width.mask()
            ),
        ),
        Operand::Mem { width, .. } => write_memory(out, *width, value),
        Operand::Imm(_) => unreachable!("destinos imediatos são excluídos pelo decoder"),
    }
}
