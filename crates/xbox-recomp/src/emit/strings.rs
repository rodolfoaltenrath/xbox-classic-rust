//! Código especializado por operação/largura, sem decodificação no runtime.

use super::{
    Repeat, StringOp, Width, accumulator, line, read, read_memory, write_memory, write_operand,
};

pub(super) fn emit(
    out: &mut String,
    kind: StringOp,
    width: Width,
    address: Width,
    repeat: Repeat,
    next: u32,
) {
    let counter = accumulator(1, address);
    let compare = matches!(kind, StringOp::Cmps | StringOp::Scas);
    let repeated = repeat != Repeat::None;
    if repeated {
        line(
            out,
            &format!(
                "if ({}) == 0 {{ s.repeat = None; s.eip = 0x{next:08X}; }} else {{",
                read(&counter)
            ),
        );
        if compare {
            line(
                out,
                "if s.repeat.is_none_or(|r| r.pc != s.eip) { s.repeat = Some(RepeatState { pc: s.eip, eflags: s.eflags }); }",
            );
        }
    }
    emit_element(out, kind, width, address);
    if repeated && compare {
        line(
            out,
            "if let Err(error) = element { s.eflags = s.repeat.expect(\"comparação REP iniciada\").eflags; return Err(error); }",
        );
    } else {
        line(out, "element?;");
    }
    if repeated {
        write_operand(
            out,
            &counter,
            &format!("({}).wrapping_sub(1)", read(&counter)),
        );
        let condition = match repeat {
            Repeat::Always => "true",
            Repeat::Equal => "s.eflags & 0x40 != 0",
            Repeat::NotEqual => "s.eflags & 0x40 == 0",
            Repeat::None => unreachable!(),
        };
        line(
            out,
            &format!(
                "if ({}) == 0 || !({condition}) {{ s.repeat = None; s.eip = 0x{next:08X}; }}",
                read(&counter)
            ),
        );
        line(out, "}");
    } else {
        line(out, &format!("s.eip = 0x{next:08X};"));
    }
}

fn emit_element(out: &mut String, kind: StringOp, width: Width, address: Width) {
    let src = accumulator(6, address);
    let dst = accumulator(7, address);
    // Nenhum índice/contador/FLAGS muda antes de todos os acessos do elemento.
    // A closure permite restaurar FLAGS originais de CMPS/SCAS em qualquer falta.
    line(out, "let element: Result<()> = (|| {");
    match kind {
        StringOp::Movs | StringOp::Lods | StringOp::Cmps => {
            line(
                out,
                &format!("let source = {};", read_memory(width, &read(&src))),
            );
        }
        _ => {}
    }
    match kind {
        StringOp::Movs | StringOp::Stos => {
            line(out, &format!("let address = {};", read(&dst)));
            let value = if kind == StringOp::Movs {
                "source".to_owned()
            } else {
                read(&accumulator(0, width))
            };
            write_memory(out, width, &value);
        }
        StringOp::Lods => write_operand(out, &accumulator(0, width), "source"),
        StringOp::Cmps | StringOp::Scas => {
            line(
                out,
                &format!("let destination = {};", read_memory(width, &read(&dst))),
            );
            if kind == StringOp::Scas {
                line(
                    out,
                    &format!("let source = {};", read(&accumulator(0, width))),
                );
            }
            line(
                out,
                &format!(
                    "let result = source.wrapping_sub(destination) & 0x{:08X}u32;",
                    width.mask()
                ),
            );
            line(
                out,
                &format!(
                    "s.eflags = arithmetic_flags_width(s.eflags, source, destination, result, true, Width::{width:?});"
                ),
            );
        }
    }
    line(
        out,
        &format!(
            "let delta = if s.eflags & 0x400 == 0 {{ {}u32 }} else {{ 0u32.wrapping_sub({}) }};",
            width.bytes(),
            width.bytes()
        ),
    );
    for index in [6, 7] {
        let used = if index == 6 {
            matches!(kind, StringOp::Movs | StringOp::Lods | StringOp::Cmps)
        } else {
            kind != StringOp::Lods
        };
        if used {
            let operand = accumulator(index, address);
            write_operand(
                out,
                &operand,
                &format!("({}).wrapping_add(delta)", read(&operand)),
            );
        }
    }
    line(out, "Ok(()) })();");
}
