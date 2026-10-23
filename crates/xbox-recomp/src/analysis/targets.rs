//! Constantes locais, tabelas com limites provados e candidatos de callbacks.
//! Nenhum passe substitui cargas de RAM gravável por constantes na execução.

use super::{Binary, Insn, Operand, Operation, Width};
mod escapes;
mod tables;
use std::collections::{BTreeMap, BTreeSet};
use xbox_formats::{Xbe, xbe::SectionFlags};

pub(super) fn discover(xbe: &Xbe<'_>, blocks: &BTreeMap<u32, Vec<Insn>>) -> BTreeSet<u32> {
    let mut targets = BTreeSet::new();
    for block in blocks.values() {
        let mut regs = [None; 8];
        for i in block {
            match &i.operation {
                Operation::Indirect { target, .. } => {
                    if let Some(target) =
                        value(xbe, target, &regs).filter(|&pc| executable(xbe, pc))
                    {
                        targets.insert(target);
                    }
                }
                Operation::Binary(Binary::Mov, Operand::Reg(dst), src) => {
                    regs[*dst] = value(xbe, src, &regs);
                }
                Operation::Lea {
                    dst: Operand::Reg(dst),
                    address: operand,
                } => {
                    regs[*dst] = address(operand, &regs);
                }
                // CLC/STC/CMC não tocam registrador: entram aqui para não invalidar
                // o rastreamento e com isso perder destinos descobertos.
                Operation::Nop | Operation::Direction(_) | Operation::Carry(_) => {}
                // Invalidar tudo é deliberadamente conservador. Não há pressuposto
                // de preservação de registradores por ABI ou por operações novas.
                _ => regs.fill(None),
            }
        }
    }
    targets.extend(tables::discover(xbe, blocks));
    targets.extend(escapes::discover(xbe, blocks));
    targets
}

fn value(xbe: &Xbe<'_>, operand: &Operand, regs: &[Option<u32>; 8]) -> Option<u32> {
    match operand {
        Operand::Imm(value) => Some(*value),
        Operand::Reg(index) => regs[*index],
        Operand::Mem {
            fs: false,
            width: Width::Dword,
            ..
        } => read_only_pointer(xbe, address(operand, regs)?),
        _ => None,
    }
}

fn address(operand: &Operand, regs: &[Option<u32>; 8]) -> Option<u32> {
    let Operand::Mem {
        base,
        index,
        scale,
        displacement,
        ..
    } = operand
    else {
        return None;
    };
    let base = if let Some(index) = base {
        regs[*index]?
    } else {
        0
    };
    let index = if let Some(index) = index {
        regs[*index]?
    } else {
        0
    };
    Some(
        displacement
            .wrapping_add(base)
            .wrapping_add(index.wrapping_mul(*scale)),
    )
}

fn read_only_pointer(xbe: &Xbe<'_>, va: u32) -> Option<u32> {
    // Exigir os quatro bytes no raw/virtual de uma mesma seção imutável.
    // Headers e padding não são fontes de ponteiros inferidos nesta versão.
    for (index, section) in xbe.sections.iter().enumerate() {
        if section.flags.tem(SectionFlags::WRITABLE) {
            continue;
        }
        let Some(offset) = va.checked_sub(section.virtual_address) else {
            continue;
        };
        if u64::from(offset) + 4 > u64::from(section.raw_size.min(section.virtual_size)) {
            continue;
        }
        let bytes = xbe
            .section_data(index)
            .ok()?
            .get(offset as usize..offset as usize + 4)?;
        return Some(u32::from_le_bytes(bytes.try_into().ok()?));
    }
    None
}

fn executable(xbe: &Xbe<'_>, pc: u32) -> bool {
    xbe.sections.iter().any(|s| {
        s.flags.tem(SectionFlags::EXECUTABLE)
            && !s.flags.tem(SectionFlags::WRITABLE)
            && pc >= s.virtual_address
            && u64::from(pc)
                < u64::from(s.virtual_address) + u64::from(s.raw_size.min(s.virtual_size))
    })
}
