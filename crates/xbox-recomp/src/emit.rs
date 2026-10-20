//! Emissão de blocos Rust. O executável gerado não decodifica instruções x86.

use std::fmt::Write;

use crate::analysis::Program;
use crate::ir::{
    Binary, CountBranch, DoubleShift, Operand, Operation, Repeat, Rotate, Shift, StringOp, Unary,
    Width,
};

mod strings;

/// Gera módulo Rust com `run(state, bus, orçamento_de_blocos)`.
/// Cada elemento REP é uma passagem limitada de seu próprio bloco.
pub fn rust_source(program: &Program) -> String {
    let mut out = String::from(
        "// Gerado por xbox-recomp. Artefato local.\n\
use xbox_recomp::{Error, Result};\n\
#[allow(unused_imports)]\n\
use xbox_recomp::runtime::{State, RepeatState, Exit, RETURN_SENTINEL, arithmetic_flags_width, arithmetic_carry_flags_width, logic_flags_width, shift_flags, rotate_flags, double_shift_flags, multiply_values, divide_values, sign_extend, condition_met};\n\
#[allow(unused_imports)]\n\
use xbox_recomp::ir::{Width, Condition, Shift, Rotate, DoubleShift};\n\
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
            if matches!(i.operation, Operation::String { repeat, .. } if repeat != Repeat::None) {
                line(
                    &mut out,
                    &format!(
                        "if s.eip != 0x{:08X} {{ s.retired = s.retired.wrapping_add(1); }}",
                        i.pc
                    ),
                );
            } else {
                line(&mut out, "s.retired = s.retired.wrapping_add(1);");
            }
        }
        line(&mut out, "if s.returned { return Ok(Exit::Returned); }");
        line(&mut out, "}");
    }
    out.push_str("            pc => return Err(Error::UnknownBlock(pc)),\n        }\n    }\n    Ok(Exit::Budget)\n}\n");
    if program
        .instructions
        .values()
        .any(|i| matches!(i.operation, Operation::Indirect { .. } | Operation::Ret(_)))
    {
        emit_target_check(&mut out, program);
    }
    out
}

fn emit_target_check(out: &mut String, program: &Program) {
    out.push_str("\nfn checked_target(s: &State, bus: &Bus, target: u32) -> Result<()> {\n    if !matches!(target, ");
    for (index, pc) in program.blocks.keys().enumerate() {
        if index != 0 {
            out.push_str(" | ");
        }
        let _ = write!(out, "0x{pc:08X}");
    }
    out.push_str(") { return Err(Error::UnknownTarget { pc: s.eip, target }); }\n    bus.traduzir(GuestVirt(target), xbox_mem::Acesso::Execucao)?;\n    Ok(())\n}\n");
}

fn emit_operation(out: &mut String, operation: &Operation, next: u32) {
    match operation {
        Operation::Binary(op, dst, src) => emit_binary(out, *op, dst, src),
        Operation::Unary(op, dst) => emit_unary(out, *op, dst),
        Operation::Shift(op, dst, count) => emit_shift(out, *op, dst, count),
        Operation::Rotate(op, dst, count) => emit_rotate(out, *op, dst, count),
        Operation::DoubleShift(op, dst, src, count) => emit_double_shift(out, *op, dst, src, count),
        Operation::Multiply { signed, src } => emit_multiply(out, *signed, src),
        Operation::Divide { signed, src } => emit_divide(out, *signed, src),
        Operation::Imul { dst, left, right } => emit_imul(out, dst, left, right),
        Operation::SignExtend { source, pair } => emit_sign_extension(out, *source, *pair),
        Operation::Extend { signed, dst, src } => emit_extend(out, *signed, dst, src),
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
        Operation::Push(..) | Operation::Pop(..) | Operation::Leave => emit_stack(out, operation),
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
        Operation::CountBranch {
            kind,
            counter,
            target,
        } => {
            emit_count_branch(out, *kind, *counter, *target, next);
            return;
        }
        Operation::String {
            kind,
            width,
            address,
            repeat,
        } => {
            strings::emit(out, *kind, *width, *address, *repeat, next);
            return;
        }
        Operation::Direction(set) => {
            line(
                out,
                if *set {
                    "s.eflags |= 0x400;"
                } else {
                    "s.eflags &= !0x400;"
                },
            );
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
        Operation::Indirect { call, target } => {
            emit_indirect(out, *call, target, next);
            return;
        }
        Operation::Ret(bytes) => {
            line(out, "let target = bus.ler_u32(GuestVirt(s.regs[4]))?;");
            line(
                out,
                "if target != RETURN_SENTINEL { checked_target(s, bus, target)?; }",
            );
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

fn emit_count_branch(out: &mut String, kind: CountBranch, counter: Width, target: u32, next: u32) {
    let operand = accumulator(1, counter);
    if kind == CountBranch::Zero {
        line(out, &format!("let count = {};", read(&operand)));
    } else {
        line(
            out,
            &format!(
                "let count = ({}).wrapping_sub(1) & 0x{:08X}u32;",
                read(&operand),
                counter.mask()
            ),
        );
        write_operand(out, &operand, "count");
    }
    let condition = match kind {
        CountBranch::Zero => "count == 0",
        CountBranch::Loop => "count != 0",
        CountBranch::LoopEqual => "count != 0 && s.eflags & 0x40 != 0",
        CountBranch::LoopNotEqual => "count != 0 && s.eflags & 0x40 == 0",
    };
    line(
        out,
        &format!("s.eip = if {condition} {{ 0x{target:08X} }} else {{ 0x{next:08X} }};"),
    );
}

fn emit_stack(out: &mut String, operation: &Operation) {
    match operation {
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
        _ => unreachable!("operação de pilha"),
    }
}

fn emit_extend(out: &mut String, signed: bool, dst: &Operand, src: &Operand) {
    line(out, &format!("let b = {};", read(src)));
    let value = if signed {
        format!(
            "sign_extend(b, Width::{:?})",
            src.width().unwrap_or(Width::Dword)
        )
    } else {
        "b".to_owned()
    };
    write_operand(out, dst, &value);
}

fn accumulator(index: usize, width: Width) -> Operand {
    if width == Width::Dword {
        Operand::Reg(index)
    } else {
        Operand::Part {
            index,
            shift: 0,
            width,
        }
    }
}

fn emit_multiply(out: &mut String, signed: bool, src: &Operand) {
    let width = src.width().unwrap_or(Width::Dword);
    line(out, &format!("let value = {};", read(src)));
    line(
        out,
        &format!(
            "let (product, overflow) = multiply_values(s.regs[0], value, Width::{width:?}, {signed});"
        ),
    );
    if width == Width::Byte {
        write_operand(out, &accumulator(0, Width::Word), "product as u32");
    } else {
        write_operand(out, &accumulator(0, width), "product as u32");
        write_operand(
            out,
            &accumulator(2, width),
            &format!("(product >> {}) as u32", width.bits()),
        );
    }
    line(
        out,
        "s.eflags = (s.eflags & !0x8D5) | if overflow { 0x801 } else { 0 };",
    );
}

fn emit_imul(out: &mut String, dst: &Operand, left: &Operand, right: &Operand) {
    let width = dst.width().unwrap_or(Width::Dword);
    line(out, &format!("let a = {};", read(left)));
    line(out, &format!("let b = {};", read(right)));
    line(
        out,
        &format!("let (product, overflow) = multiply_values(a, b, Width::{width:?}, true);"),
    );
    write_operand(out, dst, "product as u32");
    line(
        out,
        "s.eflags = (s.eflags & !0x8D5) | if overflow { 0x801 } else { 0 };",
    );
}

fn emit_divide(out: &mut String, signed: bool, src: &Operand) {
    let width = src.width().unwrap_or(Width::Dword);
    line(out, &format!("let value = {};", read(src)));
    line(
        out,
        &format!(
            "let (quotient, remainder) = divide_values(s.regs[0], s.regs[2], value, Width::{width:?}, {signed}).map_err(|reason| Error::Divide {{ pc: s.eip, reason }})?;"
        ),
    );
    if width == Width::Byte {
        write_operand(
            out,
            &accumulator(0, Width::Word),
            "quotient | (remainder << 8)",
        );
    } else {
        write_operand(out, &accumulator(0, width), "quotient");
        write_operand(out, &accumulator(2, width), "remainder");
    }
    line(out, "s.eflags &= !0x8D5;");
}

fn emit_sign_extension(out: &mut String, source: Width, pair: bool) {
    if pair {
        let sign = 1u32 << (source.bits() - 1);
        line(
            out,
            &format!(
                "let value = if s.regs[0] & 0x{sign:08X} != 0 {{ 0x{:08X}u32 }} else {{ 0 }};",
                source.mask()
            ),
        );
        write_operand(out, &accumulator(2, source), "value");
    } else {
        let target = if source == Width::Byte {
            Width::Word
        } else {
            Width::Dword
        };
        line(
            out,
            &format!("let value = sign_extend(s.regs[0], Width::{source:?});"),
        );
        write_operand(out, &accumulator(0, target), "value");
    }
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
        Binary::Adc => "a.wrapping_add(b).wrapping_add(s.eflags & 1)",
        Binary::Sub | Binary::Cmp => "a.wrapping_sub(b)",
        Binary::Sbb => "a.wrapping_sub(b).wrapping_sub(s.eflags & 1)",
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
        let flags = if matches!(op, Binary::Adc | Binary::Sbb) {
            "arithmetic_carry_flags_width"
        } else {
            "arithmetic_flags_width"
        };
        line(
            out,
            &format!(
                "s.eflags = {flags}(s.eflags, a, b, result, {}, Width::{width:?});",
                matches!(op, Binary::Sub | Binary::Sbb | Binary::Cmp)
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

fn emit_shift(out: &mut String, op: Shift, dst: &Operand, count: &Operand) {
    let width = dst.width().unwrap_or(Width::Dword);
    prepare_address(out, dst, "s.regs[4]");
    // Capturar CL antes da escrita cobre também SHL ECX,CL e SHR CL,CL.
    line(out, &format!("let count = ({}) & 31;", read(count)));
    line(out, &format!("let a = {};", read(dst)));
    let expr = match op {
        Shift::Shl => "a << count".to_owned(),
        Shift::Shr => "a >> count".to_owned(),
        Shift::Sar => {
            format!("(sign_extend(a, Width::{width:?}).cast_signed() >> count).cast_unsigned()")
        }
    };
    line(
        out,
        &format!("let result = ({expr}) & 0x{:08X}u32;", width.mask()),
    );
    // Memória é read-modify-write mesmo com contagem efetiva zero. Falha de
    // acesso não deve comprometer FLAGS ou registradores da CPU.
    write_operand(out, dst, "result");
    line(
        out,
        &format!(
            "s.eflags = shift_flags(s.eflags, a, result, count, Shift::{op:?}, Width::{width:?});"
        ),
    );
}

fn emit_rotate(out: &mut String, op: Rotate, dst: &Operand, count: &Operand) {
    let width = dst.width().unwrap_or(Width::Dword);
    let bits = width.bits();
    let through_carry = matches!(op, Rotate::Rcl | Rotate::Rcr);
    let ring_bits = bits + u32::from(through_carry);
    let ring_mask = (1u64 << ring_bits) - 1;
    prepare_address(out, dst, "s.regs[4]");
    line(out, &format!("let count = ({}) & 31;", read(count)));
    line(out, &format!("let a = {};", read(dst)));
    let ring = if through_carry {
        format!("u64::from(a) | (u64::from(s.eflags & 1) << {bits})")
    } else {
        "u64::from(a)".to_owned()
    };
    line(out, &format!("let ring = {ring};"));
    line(out, &format!("let n = count % {ring_bits};"));
    let rotated = if matches!(op, Rotate::Rol | Rotate::Rcl) {
        format!("(ring << n) | (ring >> ({ring_bits} - n))")
    } else {
        format!("(ring >> n) | (ring << ({ring_bits} - n))")
    };
    line(
        out,
        &format!("let rotated = ({rotated}) & 0x{ring_mask:X}u64;"),
    );
    line(
        out,
        &format!("let result = rotated as u32 & 0x{:08X}u32;", width.mask()),
    );
    let carry = match op {
        Rotate::Rol => "result & 1".to_owned(),
        Rotate::Ror => format!("result >> {}", bits - 1),
        Rotate::Rcl | Rotate::Rcr => format!("(rotated >> {bits}) as u32"),
    };
    line(out, &format!("let carry = {carry};"));
    write_operand(out, dst, "result");
    line(
        out,
        &format!(
            "s.eflags = rotate_flags(s.eflags, result, carry, count, Rotate::{op:?}, Width::{width:?});"
        ),
    );
}

fn emit_double_shift(
    out: &mut String,
    op: DoubleShift,
    dst: &Operand,
    src: &Operand,
    count: &Operand,
) {
    let width = dst.width().unwrap_or(Width::Dword);
    let bits = width.bits();
    line(out, &format!("let count = ({}) & 31;", read(count)));
    // A ISA não define resultado/flags para count > largura. Recusar antes de
    // qualquer acesso à memória ou escrita de estado, com diagnóstico do PC.
    line(
        out,
        &format!(
            "if count > {bits} {{ return Err(Error::UndefinedDoubleShift {{ pc: s.eip, count, bits: {bits} }}); }}"
        ),
    );
    prepare_address(out, dst, "s.regs[4]");
    line(out, &format!("let a = {};", read(dst)));
    line(out, &format!("let b = {};", read(src)));
    let expr = match op {
        DoubleShift::Shld => {
            format!("(u64::from(a) << count) | (u64::from(b) >> ({bits} - count))")
        }
        DoubleShift::Shrd => {
            format!("(u64::from(a) >> count) | (u64::from(b) << ({bits} - count))")
        }
    };
    line(
        out,
        &format!("let result = ({expr}) as u32 & 0x{:08X}u32;", width.mask()),
    );
    write_operand(out, dst, "result");
    line(
        out,
        &format!(
            "s.eflags = double_shift_flags(s.eflags, a, result, count, DoubleShift::{op:?}, Width::{width:?});"
        ),
    );
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

fn emit_indirect(out: &mut String, call: bool, target: &Operand, next: u32) {
    // Capturar antes de PUSH cobre CALL ESP, [ESP] e [ESP-4].
    line(out, &format!("let target = {};", read(target)));
    line(out, "checked_target(s, bus, target)?;");
    if call {
        line(out, "let esp = s.regs[4].wrapping_sub(4);");
        line(
            out,
            &format!("bus.escrever_u32(GuestVirt(esp), 0x{next:08X})?;"),
        );
        line(out, "s.regs[4] = esp;");
    }
    line(out, "s.eip = target;");
}
