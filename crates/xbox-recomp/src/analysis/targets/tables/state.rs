use super::{Insn, MAX_CHOICES, Operation};
use crate::analysis::{Binary, Condition, Operand, Width};
use std::collections::{BTreeMap, BTreeSet};
use xbox_formats::{Xbe, xbe::SectionFlags};
#[derive(Debug, Clone)]
enum Kind {
    Unknown(u32),
    Values(BTreeSet<u32>),
    Stack(u8, i64),
}
#[derive(Debug, Clone)]
pub(super) struct Value {
    kind: Kind,
    pub table: bool,
}
impl Value {
    fn constant(v: u32) -> Self {
        Self {
            kind: Kind::Values(BTreeSet::from([v])),
            table: false,
        }
    }
    fn single(&self) -> Option<u32> {
        if let Kind::Values(v) = &self.kind
            && v.len() == 1
        {
            return v.first().copied();
        }
        None
    }
}
#[derive(Debug)]
pub(super) struct State {
    regs: [Value; 8],
    slots: BTreeMap<(u8, i64), Value>,
    ranges: BTreeMap<u32, (u32, u32)>,
    next: u32,
    compare: Option<(Value, u32)>,
}
impl State {
    pub(super) fn new() -> Self {
        let mut state = Self {
            regs: std::array::from_fn(|i| Value {
                kind: Kind::Unknown(i as u32),
                table: false,
            }),
            slots: BTreeMap::new(),
            ranges: BTreeMap::new(),
            next: 8,
            compare: None,
        };
        state.regs[4] = Value {
            kind: Kind::Stack(0, 0),
            table: false,
        };
        state.regs[5] = Value {
            kind: Kind::Stack(1, 0),
            table: false,
        };
        state
    }
    fn unknown(&mut self) -> Value {
        let id = self.next;
        self.next += 1;
        Value {
            kind: Kind::Unknown(id),
            table: false,
        }
    }
    fn bounded(&mut self, lo: u32, hi: u32) -> Value {
        let value = self.unknown();
        if let Kind::Unknown(id) = value.kind {
            self.ranges.insert(id, (lo, hi));
        }
        value
    }
    pub(super) fn choices(&self, value: &Value) -> Option<BTreeSet<u32>> {
        match &value.kind {
            Kind::Values(values) => Some(values.clone()),
            Kind::Unknown(id) => {
                let &(lo, hi) = self.ranges.get(id)?;
                if u64::from(hi) - u64::from(lo) >= MAX_CHOICES as u64 {
                    return None;
                }
                Some((lo..=hi).collect())
            }
            Kind::Stack(..) => None,
        }
    }
    fn stack_address(&self, operand: &Operand) -> Option<(u8, i64)> {
        let Operand::Mem {
            base: Some(base),
            index: None,
            displacement,
            ..
        } = operand
        else {
            return None;
        };
        if let Kind::Stack(anchor, offset) = self.regs[*base].kind {
            Some((anchor, offset + i64::from(displacement.cast_signed())))
        } else {
            None
        }
    }
    fn addresses(&self, operand: &Operand) -> Option<BTreeSet<u32>> {
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
        let bases = if let Some(i) = base {
            self.choices(&self.regs[*i])?
        } else {
            BTreeSet::from([0])
        };
        let indices = if let Some(i) = index {
            self.choices(&self.regs[*i])?
        } else {
            BTreeSet::from([0])
        };
        if bases.len().checked_mul(indices.len())? > MAX_CHOICES {
            return None;
        }
        let mut addresses = BTreeSet::new();
        for base in bases {
            for &index in &indices {
                addresses.insert(
                    base.checked_add(index.checked_mul(*scale)?)?
                        .checked_add(*displacement)?,
                );
            }
        }
        Some(addresses)
    }
    pub(super) fn value(&mut self, xbe: &Xbe<'_>, operand: &Operand) -> Value {
        match operand {
            Operand::Reg(i) => self.regs[*i].clone(),
            Operand::Imm(v) => Value::constant(*v),
            Operand::Part {
                index,
                shift,
                width,
            } => {
                let values = self.choices(&self.regs[*index]);
                if let Some(values) = values {
                    Value {
                        kind: Kind::Values(
                            values
                                .into_iter()
                                .map(|v| (v >> shift) & width.mask())
                                .collect(),
                        ),
                        table: self.regs[*index].table,
                    }
                } else {
                    self.bounded(0, width.mask())
                }
            }
            Operand::Mem {
                fs: false, width, ..
            } => {
                if let Some(key) = self.stack_address(operand)
                    && *width == Width::Dword
                {
                    if let Some(value) = self.slots.get(&key) {
                        return value.clone();
                    }
                    let value = self.unknown();
                    self.slots.insert(key, value.clone());
                    return value;
                }
                if let Some(addresses) = self.addresses(operand) {
                    let values: Option<BTreeSet<_>> = addresses
                        .into_iter()
                        .map(|at| read_immutable(xbe, at, *width))
                        .collect();
                    if let Some(values) = values {
                        return Value {
                            kind: Kind::Values(values),
                            table: true,
                        };
                    }
                }
                self.unknown()
            }
            Operand::Mem { .. } => self.unknown(),
        }
    }
    fn store(&mut self, dst: &Operand, value: Value) {
        match dst {
            Operand::Reg(i) => self.regs[*i] = value,
            Operand::Part { index, .. } => self.regs[*index] = self.unknown(),
            Operand::Mem {
                fs: false, width, ..
            } => {
                if let Some((anchor, offset)) = self.stack_address(dst) {
                    self.slots.retain(|&(a, o), _| {
                        a == anchor && (o + 4 <= offset || o >= offset + i64::from(width.bytes()))
                    });
                    if *width == Width::Dword {
                        self.slots.insert((anchor, offset), value);
                    }
                } else {
                    self.slots.clear();
                }
            }
            Operand::Mem { .. } => self.slots.clear(),
            Operand::Imm(_) => (),
        }
    }
    fn arithmetic(&mut self, op: Binary, left: &Value, right: &Value) -> Value {
        if let (Kind::Stack(anchor, offset), Some(value)) = (&left.kind, right.single())
            && matches!(op, Binary::Add | Binary::Sub)
        {
            let delta = i64::from(value.cast_signed());
            return Value {
                kind: Kind::Stack(
                    *anchor,
                    if op == Binary::Add {
                        offset + delta
                    } else {
                        offset - delta
                    },
                ),
                table: false,
            };
        }
        if let (Some(a), Some(b)) = (self.choices(left), self.choices(right))
            && a.len()
                .checked_mul(b.len())
                .is_some_and(|n| n <= MAX_CHOICES)
        {
            let mut values = BTreeSet::new();
            for a in a {
                for &b in &b {
                    values.insert(match op {
                        Binary::Add => a.wrapping_add(b),
                        Binary::Sub => a.wrapping_sub(b),
                        Binary::And => a & b,
                        Binary::Or => a | b,
                        Binary::Xor => a ^ b,
                        _ => return self.unknown(),
                    });
                }
            }
            return Value {
                kind: Kind::Values(values),
                table: left.table || right.table,
            };
        }
        if op == Binary::And
            && let Some(mask) = right.single()
        {
            return self.bounded(0, mask);
        }
        self.unknown()
    }
    pub(super) fn step(&mut self, xbe: &Xbe<'_>, i: &Insn) {
        match &i.operation {
            Operation::Binary(Binary::Mov, dst, src) => {
                let v = self.value(xbe, src);
                self.store(dst, v);
            }
            Operation::Binary(op, dst, src) => {
                let left = self.value(xbe, dst);
                let right = self.value(xbe, src);
                self.compare = if matches!(op, Binary::Cmp | Binary::Sub)
                    && dst.width() == Some(Width::Dword)
                {
                    right.single().map(|n| (left.clone(), n))
                } else {
                    None
                };
                if !matches!(op, Binary::Cmp | Binary::Test) {
                    let v = if *op == Binary::Xor && dst == src {
                        Value::constant(0)
                    } else {
                        self.arithmetic(*op, &left, &right)
                    };
                    self.store(dst, v);
                }
            }
            Operation::Lea { dst, address } => {
                let value = if let Some((a, o)) = self.stack_address(address) {
                    Value {
                        kind: Kind::Stack(a, o),
                        table: false,
                    }
                } else if let Some(values) = self.addresses(address) {
                    Value {
                        kind: Kind::Values(values),
                        table: false,
                    }
                } else {
                    self.unknown()
                };
                self.store(dst, value);
            }
            Operation::Extend { signed, dst, src } => {
                let source = self.value(xbe, src);
                let width = src.width().unwrap_or(Width::Dword);
                let value = if let Some(values) = self.choices(&source) {
                    let values = values
                        .into_iter()
                        .map(|v| {
                            if *signed {
                                match width {
                                    Width::Byte => {
                                        i32::from((v as u8).cast_signed()).cast_unsigned()
                                    }
                                    Width::Word => {
                                        i32::from((v as u16).cast_signed()).cast_unsigned()
                                    }
                                    Width::Dword => v,
                                }
                            } else {
                                v & width.mask()
                            }
                        })
                        .collect();
                    Value {
                        kind: Kind::Values(values),
                        table: source.table,
                    }
                } else if !signed {
                    self.bounded(0, width.mask())
                } else {
                    self.unknown()
                };
                self.store(dst, value);
            }
            Operation::Push(src, Width::Dword) => {
                let value = self.value(xbe, src);
                let esp = self.regs[4].clone();
                self.regs[4] = self.arithmetic(Binary::Sub, &esp, &Value::constant(4));
                let dst = Operand::Mem {
                    fs: false,
                    base: Some(4),
                    index: None,
                    scale: 1,
                    displacement: 0,
                    width: Width::Dword,
                };
                self.store(&dst, value);
            }
            Operation::Nop
            | Operation::Direction(_)
            | Operation::Branch { .. }
            | Operation::Jump(_) => (),
            // Uma operação não descrita invalida inclusive aliases e flags. Não se
            // pressupõe ABI, preservação de registradores ou efeitos de memória novos.
            _ => {
                *self = Self::new();
            }
        }
    }
    pub(super) fn refine(&mut self, condition: Condition, taken: bool) -> bool {
        let Some((left, bound)) = self.compare.clone() else {
            return true;
        };
        let limits = match (condition, taken) {
            (Condition::Above, false) | (Condition::BelowEqual, true) => Some((0, bound)),
            (Condition::Above, true) | (Condition::BelowEqual, false) => {
                bound.checked_add(1).map(|n| (n, u32::MAX))
            }
            (Condition::Below, true) | (Condition::AboveEqual, false) => {
                bound.checked_sub(1).map(|n| (0, n))
            }
            (Condition::Below, false) | (Condition::AboveEqual, true) => Some((bound, u32::MAX)),
            (Condition::Equal, true) | (Condition::NotEqual, false) => Some((bound, bound)),
            _ => return true,
        };
        let Some((lo, hi)) = limits else {
            return false;
        };
        match left.kind {
            Kind::Unknown(id) => {
                let old = self.ranges.get(&id).copied().unwrap_or((0, u32::MAX));
                let range = (lo.max(old.0), hi.min(old.1));
                if range.0 > range.1 {
                    return false;
                }
                self.ranges.insert(id, range);
            }
            Kind::Values(values) => {
                let filtered: BTreeSet<_> = values
                    .iter()
                    .copied()
                    .filter(|&v| lo <= v && v <= hi)
                    .collect();
                if filtered.is_empty() {
                    return false;
                }
                // Conjuntos finitos iguais podem ter origens independentes.
                // Sem identidade simbólica, não restringir seus aliases.
            }
            Kind::Stack(..) => (),
        }
        true
    }
}
fn read_immutable(xbe: &Xbe<'_>, at: u32, width: Width) -> Option<u32> {
    for (i, section) in xbe.sections.iter().enumerate() {
        if section.flags.tem(SectionFlags::WRITABLE) {
            continue;
        }
        let Some(offset) = at.checked_sub(section.virtual_address) else {
            continue;
        };
        if u64::from(offset) + u64::from(width.bytes())
            > u64::from(section.raw_size.min(section.virtual_size))
        {
            continue;
        }
        let data = xbe
            .section_data(i)
            .ok()?
            .get(offset as usize..offset as usize + width.bytes() as usize)?;
        return Some(
            data.iter()
                .enumerate()
                .fold(0, |v, (lane, byte)| v | (u32::from(*byte) << (lane * 8))),
        );
    }
    None
}
