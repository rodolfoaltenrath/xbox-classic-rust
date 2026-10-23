//! Tabelas limitadas por guardas em caminhos sem joins; nenhum dado é varrido.
mod state;
mod walks;
use super::{Insn, Operation};
use std::collections::{BTreeMap, BTreeSet};
use xbox_formats::Xbe;
const MAX_PATH_BLOCKS: usize = 64;
const MAX_PATH_INSTRUCTIONS: usize = 512;
const MAX_CHOICES: usize = 4096;
pub(super) fn discover(xbe: &Xbe<'_>, blocks: &BTreeMap<u32, Vec<Insn>>) -> BTreeSet<u32> {
    let mut predecessors: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
    for (&head, block) in blocks {
        if let Some(last) = block.last() {
            for next in successors(last) {
                if blocks.contains_key(&next) {
                    predecessors.entry(next).or_default().insert(head);
                }
            }
        }
    }
    let mut targets = BTreeSet::new();
    for (&head, block) in blocks {
        if !block
            .iter()
            .any(|i| matches!(i.operation, Operation::Indirect { .. }))
        {
            continue;
        }
        let mut path = vec![head];
        let mut visited = BTreeSet::from([head]);
        let mut size = block.len();
        while path.len() < MAX_PATH_BLOCKS {
            let Some(preds) = predecessors.get(path.last().unwrap()) else {
                break;
            };
            if preds.len() != 1 {
                break;
            }
            let prev = *preds.first().unwrap();
            if !visited.insert(prev) || size + blocks[&prev].len() > MAX_PATH_INSTRUCTIONS {
                break;
            }
            size += blocks[&prev].len();
            path.push(prev);
        }
        path.reverse();
        let mut state = state::State::new();
        let mut possible = true;
        let mut inferred = BTreeSet::new();
        for (at, &pc) in path.iter().enumerate() {
            for i in &blocks[&pc] {
                if let Operation::Indirect { target, .. } = &i.operation {
                    let value = state.value(xbe, target);
                    if value.table
                        && let Some(values) = state.choices(&value)
                    {
                        inferred.extend(
                            values
                                .into_iter()
                                .filter(|&target| super::executable(xbe, target)),
                        );
                    }
                }
                state.step(xbe, i);
            }
            if let Some(&next) = path.get(at + 1)
                && let Some(last) = blocks[&pc].last()
                && let Operation::Branch { condition, target } = last.operation
                && target != last.next
                && !state.refine(condition, next == target)
            {
                possible = false;
                break;
            }
        }
        // Só publicar o caminho após conferir todas as suas guardas.
        if possible {
            targets.extend(inferred);
        }
    }
    targets.extend(walks::discover(xbe, blocks));
    targets
}
fn successors(i: &Insn) -> Vec<u32> {
    match i.operation {
        Operation::Jump(pc) => vec![pc],
        Operation::Branch { target, .. } | Operation::CountBranch { target, .. } => {
            vec![target, i.next]
        }
        Operation::Call(_)
        | Operation::Indirect { .. }
        | Operation::Ret(_)
        | Operation::Breakpoint
        | Operation::Halt => vec![],
        _ => vec![i.next],
    }
}
