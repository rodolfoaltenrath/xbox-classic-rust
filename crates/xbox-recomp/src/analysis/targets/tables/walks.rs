//! Extensões finitas de tabelas de callbacks percorridas por cursor local.
//! Não substitui a RAM do cursor por constante: só acrescenta os candidatos
//! readonly da extensão explícita (início/fim/passo) ao conjunto AOT.
use super::{Insn, MAX_CHOICES, Operation, state::State};
use crate::analysis::{Binary, Condition, Operand, Width};
use std::collections::{BTreeMap, BTreeSet};
use xbox_formats::Xbe;
const MAX_LOOP_INSTRUCTIONS: usize = 512;
fn successors(i: &Insn) -> Vec<u32> {
    match i.operation {
        Operation::Jump(pc) => vec![pc],
        Operation::Branch { target, .. } | Operation::CountBranch { target, .. } => {
            vec![target, i.next]
        }
        Operation::Ret(_)
        | Operation::Breakpoint
        | Operation::Halt
        | Operation::Indirect { call: false, .. } => vec![],
        _ => vec![i.next],
    }
}
fn concrete(state: &mut State, xbe: &Xbe<'_>, operand: &Operand) -> Option<u32> {
    let value = state.value(xbe, operand);
    let choices = state.choices(&value)?;
    if choices.len() == 1 {
        choices.first().copied()
    } else {
        None
    }
}
fn local(operand: &Operand) -> bool {
    matches!(
        operand,
        Operand::Mem {
            fs: false,
            base: Some(4 | 5),
            index: None,
            ..
        }
    )
}
struct Guard {
    cursor: Operand,
    end: u32,
    target: u32,
    zero_flag: Option<Operand>,
}
fn guard(xbe: &Xbe<'_>, block: &[Insn]) -> Option<Guard> {
    let mut state = State::new();
    let mut compare = None;
    let mut zero_flag = None;
    for i in block {
        match &i.operation {
            Operation::Binary(Binary::Cmp, dst, src)
                if local(dst) && dst.width() == Some(Width::Dword) =>
            {
                compare = concrete(&mut state, xbe, src).map(|end| (dst.clone(), end));
            }
            Operation::Binary(Binary::Mov, dst, src) => {
                if zero_flag.as_ref() == Some(dst) {
                    zero_flag = None;
                }
                if local(dst)
                    && dst.width() == Some(Width::Byte)
                    && concrete(&mut state, xbe, src) == Some(0)
                {
                    zero_flag = Some(dst.clone());
                }
            }
            Operation::Branch {
                condition: Condition::AboveEqual,
                target,
            } => {
                let (cursor, end) = compare?;
                return Some(Guard {
                    cursor,
                    end,
                    target: *target,
                    zero_flag,
                });
            }
            Operation::Lea { .. } | Operation::Nop => (),
            _ => compare = None,
        }
        state.step(xbe, i);
    }
    None
}
fn upper_exits(guard: &Guard, region: &BTreeSet<u32>, blocks: &BTreeMap<u32, Vec<Insn>>) -> bool {
    if !region.contains(&guard.target) {
        return true;
    }
    // Forma booleana do CRT: o caminho cursor>=fim chega com flag=0;
    // TEST flag,1 / JNE corpo só deixa entrar quando a guarda foi satisfeita.
    let Some(flag) = &guard.zero_flag else {
        return false;
    };
    let Some(block) = blocks.get(&guard.target) else {
        return false;
    };
    let [load, test, branch] = block.as_slice() else {
        return false;
    };
    let Operation::Binary(Binary::Mov, dst, src) = &load.operation else {
        return false;
    };
    if src != flag || dst.width() != Some(Width::Byte) {
        return false;
    }
    if !matches!(&test.operation,Operation::Binary(Binary::Test,left,Operand::Imm(1)) if left==dst)
    {
        return false;
    }
    if !matches!(branch.operation,Operation::Branch{condition:Condition::NotEqual,target} if region.contains(&target))
    {
        return false;
    }
    let Some(out) = blocks.get(&branch.next) else {
        return false;
    };
    matches!(out.as_slice(),[Insn{operation:Operation::Jump(exit),..}] if !region.contains(exit))
}
fn increments_only(cursor: &Operand, instructions: &[&Insn], tail: u32) -> bool {
    let mut count = 0;
    for (i, insn) in instructions.iter().enumerate() {
        // O cursor não é passado por endereço, nem pode ser alterado por um
        // store explícito de alias desconhecido. Chamadas mantêm semântica de
        // RAM dinâmica: a extensão encontrada é um conjunto de candidatos.
        if let Operation::Unary(_, dst) = &insn.operation
            && matches!(dst, Operand::Mem { .. })
        {
            return false;
        }
        if let Operation::Binary(op, dst, ..) = &insn.operation
            && !matches!(op, Binary::Cmp | Binary::Test)
            && let Operand::Mem {
                base,
                index,
                displacement,
                width,
                fs,
                ..
            } = dst
        {
            let Operand::Mem {
                base: cursor_base,
                displacement: cursor_at,
                ..
            } = cursor
            else {
                return false;
            };
            if *fs || index.is_some() || base != cursor_base {
                return false;
            }
            let start = i64::from(displacement.cast_signed());
            let at = i64::from(cursor_at.cast_signed());
            if start < at + 4 && at < start + i64::from(width.bytes()) && dst != cursor {
                return false;
            }
        }

        if let Operation::Lea { address, .. } = &insn.operation
            && address == cursor
        {
            return false;
        }
        if let Operation::Binary(op, dst, ..) = &insn.operation
            && !matches!(op, Binary::Cmp | Binary::Test)
            && dst == cursor
        {
            let Operation::Binary(Binary::Mov, _, Operand::Reg(reg)) = &insn.operation else {
                return false;
            };
            if insn.pc < tail
                || i < 2
                || !matches!(&instructions[i-2].operation,Operation::Binary(Binary::Mov,Operand::Reg(r),src) if r==reg&&src==cursor)
                || !matches!(&instructions[i-1].operation,Operation::Binary(Binary::Add,Operand::Reg(r),Operand::Imm(4)) if r==reg)
            {
                return false;
            }
            count += 1;
        }
    }
    count == 1
}
fn consumes(cursor: &Operand, block: &[Insn]) -> bool {
    let mut pointers = [false; 8];
    let mut loaded = [false; 8];
    for i in block {
        match &i.operation {
            Operation::Binary(Binary::Mov, Operand::Reg(dst), src) => {
                let direct = src == cursor;
                let read = matches!(src,Operand::Mem{fs:false,base:Some(base),index:None,displacement:0,width:Width::Dword,..} if pointers[*base]);
                pointers[*dst] = direct;
                loaded[*dst] = read;
            }
            Operation::Indirect { call: true, target } => {
                return match target {
                    Operand::Reg(i) => loaded[*i],
                    Operand::Mem {
                        fs: false,
                        base: Some(i),
                        index: None,
                        displacement: 0,
                        width: Width::Dword,
                        ..
                    } => pointers[*i],
                    _ => false,
                };
            }
            Operation::Binary(Binary::Cmp | Binary::Test, ..)
            | Operation::Nop
            | Operation::Branch { .. } => (),
            _ => {
                pointers.fill(false);
                loaded.fill(false);
            }
        }
    }
    false
}
pub(super) fn discover(xbe: &Xbe<'_>, blocks: &BTreeMap<u32, Vec<Insn>>) -> BTreeSet<u32> {
    let mut predecessors: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
    for (&pc, block) in blocks {
        if let Some(last) = block.last() {
            for next in successors(last) {
                predecessors.entry(next).or_default().insert(pc);
            }
        }
    }
    let mut targets = BTreeSet::new();
    for (&tail, body) in blocks {
        let Some(Insn {
            operation: Operation::Jump(head),
            ..
        }) = body.last()
        else {
            continue;
        };
        if *head >= tail {
            continue;
        }
        let Some(g) = blocks.get(head).and_then(|b| guard(xbe, b)) else {
            continue;
        };
        let mut region = BTreeSet::new();
        let mut pending = vec![*head];
        let mut total = 0;
        while let Some(pc) = pending.pop() {
            if pc < *head || pc > tail || !region.insert(pc) {
                continue;
            }
            let Some(block) = blocks.get(&pc) else {
                continue;
            };
            total += block.len();
            if total > MAX_LOOP_INSTRUCTIONS {
                break;
            }
            if let Some(last) = block.last() {
                pending.extend(successors(last));
            }
        }
        if total > MAX_LOOP_INSTRUCTIONS
            || !region.contains(&tail)
            || !upper_exits(&g, &region, blocks)
        {
            continue;
        }
        let mut incoming = BTreeSet::new();
        let mut invalid_entry = false;
        for pc in &region {
            for &pred in predecessors.get(pc).into_iter().flatten() {
                if !region.contains(&pred) {
                    if pc != head {
                        invalid_entry = true;
                    }
                    incoming.insert(pred);
                }
            }
        }
        if invalid_entry || incoming.len() != 1 {
            continue;
        }
        let preheader = &blocks[incoming.first().unwrap()];
        let mut state = State::new();
        for i in preheader {
            state.step(xbe, i);
        }
        let Some(begin) = concrete(&mut state, xbe, &g.cursor) else {
            continue;
        };
        if begin >= g.end
            || !begin.is_multiple_of(4)
            || !g.end.is_multiple_of(4)
            || (g.end - begin) / 4 > MAX_CHOICES as u32
        {
            continue;
        }
        let instructions: Vec<_> = region.iter().flat_map(|pc| blocks[pc].iter()).collect();
        if !increments_only(&g.cursor, &instructions, tail)
            || !region.iter().any(|pc| consumes(&g.cursor, &blocks[pc]))
        {
            continue;
        }
        let values: Option<Vec<_>> = (begin..g.end)
            .step_by(4)
            .map(|at| super::super::read_only_pointer(xbe, at))
            .collect();
        if let Some(values) = values
            && values
                .iter()
                .all(|&pc| pc == 0 || super::super::executable(xbe, pc))
        {
            targets.extend(values.into_iter().filter(|&v| v != 0));
        }
    }
    targets
}
