//! Fluxo possível de endereços de código que escapam para memória.
//!
//! Este passe não afirma um valor constante de RAM gravável. Mantém candidatos
//! finitos de endereços presentes no código alcançado e um pool de possíveis
//! aliases de memória, para compilar callbacks que podem chegar a chamadas
//! indiretas. A execução continua lendo RAM e validando `UnknownTarget`. Tabelas
//! indexadas readonly só são lidas pelo passe separado com prova de limites.
use super::{Binary, Insn, Operand, Operation, Width};
use std::collections::{BTreeMap, BTreeSet};
use xbox_formats::Xbe;
const MAX_CANDIDATES: usize = 4096;
#[derive(Default)]
struct MemoryFlow {
    all: BTreeSet<u32>,
    unknown: BTreeSet<u32>,
    absolute: BTreeMap<u32, BTreeSet<u32>>,
}
impl MemoryFlow {
    fn store(&mut self, dst: Option<u32>, values: BTreeSet<u32>) {
        self.all.extend(&values);
        if let Some(at) = dst {
            self.absolute.entry(at).or_default().extend(values);
        } else {
            self.unknown.extend(values);
        }
    }
    fn read(&self, src: Option<u32>) -> BTreeSet<u32> {
        if let Some(at) = src {
            let mut values = self.unknown.clone();
            if let Some(known) = self.absolute.get(&at) {
                values.extend(known);
            }
            values
        } else {
            self.all.clone()
        }
    }
}
fn absolute(operand: &Operand) -> Option<u32> {
    if let Operand::Mem {
        fs: false,
        base: None,
        index: None,
        displacement,
        ..
    } = operand
    {
        Some(*displacement)
    } else {
        None
    }
}

pub(super) fn discover(xbe: &Xbe<'_>, blocks: &BTreeMap<u32, Vec<Insn>>) -> BTreeSet<u32> {
    let mut pool = MemoryFlow::default();
    let mut targets = BTreeSet::new();
    // O primeiro passe coleta escapes concretos; o segundo propaga os candidatos
    // através das cargas de memória. Nenhum caminho é eliminado por este domínio.
    for _ in 0..2 {
        for block in blocks.values() {
            let mut regs: [BTreeSet<u32>; 8] = std::array::from_fn(|_| BTreeSet::new());
            for i in block {
                match &i.operation {
                    Operation::Binary(Binary::Mov, dst, src) => {
                        let candidates = source(xbe, src, &regs, &pool);
                        store(dst, candidates, &mut regs, &mut pool);
                    }
                    Operation::Lea { dst, address } => {
                        let candidates = match address {
                            Operand::Mem {
                                base: None,
                                index: None,
                                displacement,
                                ..
                            } if super::executable(xbe, *displacement) => {
                                BTreeSet::from([*displacement])
                            }
                            _ => BTreeSet::new(),
                        };
                        store(dst, candidates, &mut regs, &mut pool);
                    }
                    Operation::Push(src, Width::Dword) => {
                        let values = source(xbe, src, &regs, &pool);
                        pool.store(None, values);
                    }
                    Operation::Indirect { target, call } => {
                        if *call && !pool.all.is_empty() {
                            targets.extend(source(xbe, target, &regs, &pool));
                        }
                        regs.iter_mut().for_each(BTreeSet::clear);
                    }
                    Operation::Binary(Binary::Cmp | Binary::Test, ..)
                    | Operation::Nop
                    | Operation::Direction(_)
                    | Operation::Jump(_)
                    | Operation::Branch { .. } => (),
                    Operation::Binary(_, Operand::Reg(dst), _)
                    | Operation::Unary(_, Operand::Reg(dst)) => regs[*dst].clear(),
                    _ => regs.iter_mut().for_each(BTreeSet::clear),
                }
                if pool.all.len() > MAX_CANDIDATES || targets.len() > MAX_CANDIDATES {
                    return BTreeSet::new();
                }
            }
        }
    }
    targets
}
fn source(
    xbe: &Xbe<'_>,
    operand: &Operand,
    regs: &[BTreeSet<u32>; 8],
    pool: &MemoryFlow,
) -> BTreeSet<u32> {
    match operand {
        Operand::Imm(v) if super::executable(xbe, *v) => BTreeSet::from([*v]),
        Operand::Reg(i) => regs[*i].clone(),
        Operand::Mem {
            fs: false,
            base: None,
            index: None,
            displacement,
            width: Width::Dword,
            ..
        } if super::read_only_pointer(xbe, *displacement).is_some() => {
            let v = super::read_only_pointer(xbe, *displacement).expect("readonly");
            if super::executable(xbe, v) {
                BTreeSet::from([v])
            } else {
                BTreeSet::new()
            }
        }
        Operand::Mem {
            fs: false,
            width: Width::Dword,
            ..
        } => pool.read(absolute(operand)),
        _ => BTreeSet::new(),
    }
}
fn store(
    dst: &Operand,
    values: BTreeSet<u32>,
    regs: &mut [BTreeSet<u32>; 8],
    pool: &mut MemoryFlow,
) {
    match dst {
        Operand::Reg(i) => regs[*i] = values,
        Operand::Mem {
            width: Width::Dword,
            ..
        } => pool.store(absolute(dst), values),
        Operand::Part { index, .. } => regs[*index].clear(),
        _ => (),
    }
}
