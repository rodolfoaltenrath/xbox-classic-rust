//! Descoberta conservadora por fluxo alcançável, com rejeição de código ambíguo.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use iced_x86::{
    Code, ConditionCode, Decoder, DecoderOptions, Instruction, Mnemonic, OpKind, Register,
};
use xbox_formats::{Xbe, xbe::SectionFlags};

pub use crate::ir::{
    Binary, Condition, CountBranch, DoubleShift, Operand, Operation, Repeat, Rotate, Shift,
    StringOp, Unary, Width,
};
use crate::{Error, Result};

mod targets;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Insn {
    pub pc: u32,
    pub next: u32,
    pub operation: Operation,
}

#[derive(Debug)]
pub struct Program {
    pub entry: u32,
    pub instructions: BTreeMap<u32, Insn>,
    pub blocks: BTreeMap<u32, Vec<Insn>>,
    pub kernel_imports: Vec<xbox_kernel::imports::Export>,
}

fn fail(pc: u32, detail: impl Into<String>) -> Error {
    Error::Analysis {
        pc,
        detail: detail.into(),
    }
}

/// Analisa fluxo direto e indireto inferível, sem varrer dados como código.
pub fn analyze(bytes: &[u8]) -> Result<Program> {
    analyze_with_entries(bytes, &[])
}

/// Entradas adicionais são raízes explícitas, sujeitas à mesma validação do entry.
pub fn analyze_with_entries(bytes: &[u8], entries: &[u32]) -> Result<Program> {
    let xbe = Xbe::parse(bytes)?;
    validate_layout(&xbe)?;
    let kernel_imports = xbox_kernel::imports::plan(&xbe)?
        .into_iter()
        .map(|e| (e.ordinal, e))
        .collect::<BTreeMap<_, _>>()
        .into_values()
        .collect();
    if entries.len() > 65_536 {
        return Err(fail(xbe.entry_point, "limite de 65536 entradas adicionais"));
    }
    let mut graph = Discovery::default();
    graph.enqueue(xbe.entry_point);
    for &entry in entries {
        graph.enqueue(entry);
    }
    let mut rounds = 0;
    loop {
        if rounds == 256 {
            return Err(fail(
                xbe.entry_point,
                "limite de 256 rodadas de descoberta indireta; informe entradas adicionais",
            ));
        }
        rounds += 1;
        graph.decode_pending(&xbe)?;
        let blocks = build_blocks(&graph.instructions, &graph.leaders)?;
        let inferred = targets::discover(&xbe, &blocks);
        for target in inferred {
            if !graph.leaders.contains(&target) {
                graph.enqueue(target);
            }
        }
        if graph.pending.is_empty() {
            break;
        }
    }
    // Um destino dinâmico pode apontar para qualquer início já decodificado.
    // Só programas com controle indireto pagam pelo despacho por instrução.
    if graph
        .instructions
        .values()
        .any(|i| matches!(i.operation, Operation::Indirect { .. }))
    {
        graph.leaders.extend(graph.instructions.keys());
    }
    let blocks = build_blocks(&graph.instructions, &graph.leaders)?;
    Ok(Program {
        entry: xbe.entry_point,
        instructions: graph.instructions,
        blocks,
        kernel_imports,
    })
}

#[derive(Debug, Default)]
struct Discovery {
    instructions: BTreeMap<u32, Insn>,
    pending: VecDeque<u32>,
    owners: BTreeMap<u32, u32>,
    leaders: BTreeSet<u32>,
}

impl Discovery {
    fn enqueue(&mut self, pc: u32) {
        self.leaders.insert(pc);
        self.pending.push_back(pc);
    }

    fn decode_pending(&mut self, xbe: &Xbe<'_>) -> Result<()> {
        while let Some(pc) = self.pending.pop_front() {
            if self.instructions.contains_key(&pc) {
                continue;
            }
            if self.instructions.len() >= 65_536 {
                return Err(fail(pc, "limite de 65536 instruções alcançáveis"));
            }
            if self.owners.contains_key(&pc) {
                return Err(fail(pc, "alvo no meio de outra instrução"));
            }
            let i = decode(xbe, pc)?;
            for address in pc..i.next {
                if self.owners.insert(address, pc).is_some() {
                    return Err(fail(pc, "instruções sobrepostas"));
                }
            }
            match i.operation {
                Operation::Jump(target) => self.enqueue(target),
                Operation::Branch { target, .. }
                | Operation::CountBranch { target, .. }
                | Operation::Call(target) => {
                    self.enqueue(target);
                    self.enqueue(i.next);
                }
                // HLT pode ser retomado por um evento futuro. Traduzir também
                // a continuação; parar a descoberta aqui esconderia código.
                Operation::Indirect { call: true, .. } | Operation::Halt => self.enqueue(i.next),
                // INT3 transfere para uma exceção, sem fallthrough automático.
                // Uma continuação futura deve ser raiz explícita (--entry).
                Operation::Ret(_)
                | Operation::Indirect { call: false, .. }
                | Operation::Breakpoint => {}
                Operation::String { repeat, .. } if repeat != Repeat::None => {
                    self.leaders.insert(pc);
                    self.enqueue(i.next);
                }
                _ => self.pending.push_back(i.next),
            }
            self.instructions.insert(pc, i);
        }
        Ok(())
    }
}

fn decode(xbe: &Xbe<'_>, pc: u32) -> Result<Insn> {
    let (index, section) = xbe
        .sections
        .iter()
        .enumerate()
        .find(|(_, s)| {
            s.flags.tem(SectionFlags::EXECUTABLE)
                && pc >= s.virtual_address
                && u64::from(pc)
                    < u64::from(s.virtual_address) + u64::from(s.raw_size.min(s.virtual_size))
        })
        .ok_or_else(|| fail(pc, "fluxo fora dos bytes de uma seção executável"))?;
    if section.flags.tem(SectionFlags::WRITABLE) {
        return Err(fail(
            pc,
            "código gravável exige invalidação AOT ainda não implementada",
        ));
    }
    let data = xbe.section_data(index)?;
    let offset = (pc - section.virtual_address) as usize;
    let available = section.raw_size.min(section.virtual_size) as usize;
    let mut decoder = Decoder::with_ip(
        32,
        &data[offset..available],
        u64::from(pc),
        DecoderOptions::NONE,
    );
    let i = decoder.decode();
    if i.is_invalid() {
        return Err(fail(pc, "instrução inválida ou truncada"));
    }
    let next = pc
        .checked_add(i.len() as u32)
        .ok_or_else(|| fail(pc, "instrução transborda o espaço x86"))?;
    Ok(Insn {
        pc,
        next,
        operation: lower(&i)?,
    })
}

fn build_blocks(
    instructions: &BTreeMap<u32, Insn>,
    leaders: &BTreeSet<u32>,
) -> Result<BTreeMap<u32, Vec<Insn>>> {
    let mut blocks = BTreeMap::new();
    for start in leaders {
        let mut block = Vec::new();
        let mut pc = *start;
        loop {
            let i = instructions
                .get(&pc)
                .ok_or_else(|| fail(pc, "bloco sem instrução"))?;
            block.push(i.clone());
            pc = i.next;
            if matches!(
                i.operation,
                Operation::Jump(_)
                    | Operation::Branch { .. }
                    | Operation::CountBranch { .. }
                    | Operation::Call(_)
                    | Operation::Indirect { .. }
                    | Operation::Ret(_)
                    | Operation::Halt
                    | Operation::Breakpoint
            ) || matches!(i.operation, Operation::String { repeat, .. } if repeat != Repeat::None)
                || leaders.contains(&pc)
            {
                break;
            }
        }
        blocks.insert(*start, block);
    }
    Ok(blocks)
}

fn lower(i: &Instruction) -> Result<Operation> {
    let pc = i.ip32();
    if !matches!(i.segment_prefix(), Register::None | Register::FS) {
        return Err(fail(pc, "prefixo LOCK/segmento ainda não suportado"));
    }
    if i.has_lock_prefix() && !(i.mnemonic() == Mnemonic::Cmpxchg && i.op0_kind() == OpKind::Memory)
    {
        return Err(fail(
            pc,
            "LOCK exige CMPXCHG com destino de memória neste perfil",
        ));
    }
    if let Some(op) = lower_string(i)? {
        return Ok(op);
    }
    if i.has_rep_prefix() || i.has_repne_prefix() {
        return Err(fail(pc, "REP exige operação de string suportada"));
    }
    if let Some(op) = lower_exchange(i)? {
        return Ok(op);
    }
    let binary = match i.mnemonic() {
        Mnemonic::Mov => Some(Binary::Mov),
        Mnemonic::Add => Some(Binary::Add),
        Mnemonic::Adc => Some(Binary::Adc),
        Mnemonic::Sub => Some(Binary::Sub),
        Mnemonic::Sbb => Some(Binary::Sbb),
        Mnemonic::Xor => Some(Binary::Xor),
        Mnemonic::And => Some(Binary::And),
        Mnemonic::Or => Some(Binary::Or),
        Mnemonic::Cmp => Some(Binary::Cmp),
        Mnemonic::Test => Some(Binary::Test),
        _ => None,
    };
    if let Some(op) = binary {
        let dst = operand(i, 0)?;
        let src = operand(i, 1)?;
        let width = dst
            .width()
            .ok_or_else(|| fail(pc, "destino imediato inválido"))?;
        if src.width().is_some_and(|w| w != width) {
            return Err(fail(pc, "larguras de operandos incompatíveis"));
        }
        return Ok(Operation::Binary(op, dst, src));
    }
    let unary = match i.mnemonic() {
        Mnemonic::Inc => Some(Unary::Inc),
        Mnemonic::Dec => Some(Unary::Dec),
        Mnemonic::Neg => Some(Unary::Neg),
        Mnemonic::Not => Some(Unary::Not),
        _ => None,
    };
    if let Some(op) = unary {
        return Ok(Operation::Unary(op, operand(i, 0)?));
    }
    if let Some(op) = lower_bit_motion(i)? {
        return Ok(op);
    }
    if let Some(op) = lower_multiply_divide(i)? {
        return Ok(op);
    }
    if matches!(i.mnemonic(), Mnemonic::Movsx | Mnemonic::Movzx) {
        let dst = operand(i, 0)?;
        let src = operand(i, 1)?;
        if dst
            .width()
            .zip(src.width())
            .is_none_or(|(d, s)| d.bits() <= s.bits())
        {
            return Err(fail(pc, "larguras de extensão incompatíveis"));
        }
        return Ok(Operation::Extend {
            signed: i.mnemonic() == Mnemonic::Movsx,
            dst,
            src,
        });
    }
    if i.mnemonic() == Mnemonic::Lea {
        return Ok(Operation::Lea {
            dst: operand(i, 0)?,
            address: memory(i, Width::Dword)?,
        });
    }
    if i.is_jcc_short_or_near() && i.op0_kind() == OpKind::NearBranch32 {
        return Ok(Operation::Branch {
            condition: lower_condition(i)?,
            target: i.near_branch32(),
        });
    }
    lower_stack_and_flow(i)
}

fn lower_exchange(i: &Instruction) -> Result<Option<Operation>> {
    if i.mnemonic() == Mnemonic::Cmpxchg {
        return Ok(Some(Operation::CompareExchange {
            dst: operand(i, 0)?,
            src: operand(i, 1)?,
            locked: i.has_lock_prefix(),
        }));
    }
    if matches!(
        i.mnemonic(),
        Mnemonic::Seto
            | Mnemonic::Setno
            | Mnemonic::Setb
            | Mnemonic::Setae
            | Mnemonic::Sete
            | Mnemonic::Setne
            | Mnemonic::Setbe
            | Mnemonic::Seta
            | Mnemonic::Sets
            | Mnemonic::Setns
            | Mnemonic::Setp
            | Mnemonic::Setnp
            | Mnemonic::Setl
            | Mnemonic::Setge
            | Mnemonic::Setle
            | Mnemonic::Setg
    ) {
        return Ok(Some(Operation::SetCondition {
            condition: lower_condition(i)?,
            dst: operand(i, 0)?,
        }));
    }
    Ok(None)
}

fn lower_string(i: &Instruction) -> Result<Option<Operation>> {
    let (kind, width) = match i.code() {
        Code::Movsb_m8_m8 => (StringOp::Movs, Width::Byte),
        Code::Movsw_m16_m16 => (StringOp::Movs, Width::Word),
        Code::Movsd_m32_m32 => (StringOp::Movs, Width::Dword),
        Code::Stosb_m8_AL => (StringOp::Stos, Width::Byte),
        Code::Stosw_m16_AX => (StringOp::Stos, Width::Word),
        Code::Stosd_m32_EAX => (StringOp::Stos, Width::Dword),
        Code::Lodsb_AL_m8 => (StringOp::Lods, Width::Byte),
        Code::Lodsw_AX_m16 => (StringOp::Lods, Width::Word),
        Code::Lodsd_EAX_m32 => (StringOp::Lods, Width::Dword),
        Code::Cmpsb_m8_m8 => (StringOp::Cmps, Width::Byte),
        Code::Cmpsw_m16_m16 => (StringOp::Cmps, Width::Word),
        Code::Cmpsd_m32_m32 => (StringOp::Cmps, Width::Dword),
        Code::Scasb_AL_m8 => (StringOp::Scas, Width::Byte),
        Code::Scasw_AX_m16 => (StringOp::Scas, Width::Word),
        Code::Scasd_EAX_m32 => (StringOp::Scas, Width::Dword),
        _ => return Ok(None),
    };
    if i.segment_prefix() != Register::None {
        return Err(fail(i.ip32(), "segmento em string ainda não suportado"));
    }
    let address = if i
        .op_kinds()
        .any(|k| matches!(k, OpKind::MemorySegSI | OpKind::MemoryESDI))
    {
        Width::Word
    } else {
        Width::Dword
    };
    let compare = matches!(kind, StringOp::Cmps | StringOp::Scas);
    let repeat = if i.has_repne_prefix() {
        if !compare {
            return Err(fail(i.ip32(), "REPNE somente em CMPS/SCAS"));
        }
        Repeat::NotEqual
    } else if i.has_rep_prefix() {
        if compare {
            Repeat::Equal
        } else {
            Repeat::Always
        }
    } else {
        Repeat::None
    };
    Ok(Some(Operation::String {
        kind,
        width,
        address,
        repeat,
    }))
}

fn lower_multiply_divide(i: &Instruction) -> Result<Option<Operation>> {
    Ok(Some(match i.mnemonic() {
        Mnemonic::Mul => Operation::Multiply {
            signed: false,
            src: operand(i, 0)?,
        },
        Mnemonic::Imul if i.op_count() == 1 => Operation::Multiply {
            signed: true,
            src: operand(i, 0)?,
        },
        Mnemonic::Imul => {
            let dst = operand(i, 0)?;
            let (left, right) = if i.op_count() == 2 {
                (dst.clone(), operand(i, 1)?)
            } else {
                (operand(i, 1)?, operand(i, 2)?)
            };
            Operation::Imul { dst, left, right }
        }
        Mnemonic::Div => Operation::Divide {
            signed: false,
            src: operand(i, 0)?,
        },
        Mnemonic::Idiv => Operation::Divide {
            signed: true,
            src: operand(i, 0)?,
        },
        Mnemonic::Cbw => Operation::SignExtend {
            source: Width::Byte,
            pair: false,
        },
        Mnemonic::Cwde => Operation::SignExtend {
            source: Width::Word,
            pair: false,
        },
        Mnemonic::Cwd => Operation::SignExtend {
            source: Width::Word,
            pair: true,
        },
        Mnemonic::Cdq => Operation::SignExtend {
            source: Width::Dword,
            pair: true,
        },
        _ => return Ok(None),
    }))
}

fn lower_bit_motion(i: &Instruction) -> Result<Option<Operation>> {
    let shift = match i.mnemonic() {
        Mnemonic::Shl => Some(Shift::Shl),
        Mnemonic::Shr => Some(Shift::Shr),
        Mnemonic::Sar => Some(Shift::Sar),
        _ => None,
    };
    if let Some(op) = shift {
        return Ok(Some(Operation::Shift(op, operand(i, 0)?, operand(i, 1)?)));
    }
    let rotate = match i.mnemonic() {
        Mnemonic::Rol => Some(Rotate::Rol),
        Mnemonic::Ror => Some(Rotate::Ror),
        Mnemonic::Rcl => Some(Rotate::Rcl),
        Mnemonic::Rcr => Some(Rotate::Rcr),
        _ => None,
    };
    if let Some(op) = rotate {
        return Ok(Some(Operation::Rotate(op, operand(i, 0)?, operand(i, 1)?)));
    }
    let double = match i.mnemonic() {
        Mnemonic::Shld => Some(DoubleShift::Shld),
        Mnemonic::Shrd => Some(DoubleShift::Shrd),
        _ => None,
    };
    if let Some(op) = double {
        return Ok(Some(Operation::DoubleShift(
            op,
            operand(i, 0)?,
            operand(i, 1)?,
            operand(i, 2)?,
        )));
    }
    Ok(None)
}

fn lower_stack_and_flow(i: &Instruction) -> Result<Operation> {
    let pc = i.ip32();
    let count_branch = match i.code() {
        Code::Jcxz_rel8_32 => Some((CountBranch::Zero, Width::Word)),
        Code::Jecxz_rel8_32 => Some((CountBranch::Zero, Width::Dword)),
        Code::Loop_rel8_32_CX => Some((CountBranch::Loop, Width::Word)),
        Code::Loop_rel8_32_ECX => Some((CountBranch::Loop, Width::Dword)),
        Code::Loope_rel8_32_CX => Some((CountBranch::LoopEqual, Width::Word)),
        Code::Loope_rel8_32_ECX => Some((CountBranch::LoopEqual, Width::Dword)),
        Code::Loopne_rel8_32_CX => Some((CountBranch::LoopNotEqual, Width::Word)),
        Code::Loopne_rel8_32_ECX => Some((CountBranch::LoopNotEqual, Width::Dword)),
        _ => None,
    };
    if let Some((kind, counter)) = count_branch {
        return Ok(Operation::CountBranch {
            kind,
            counter,
            target: i.near_branch32(),
        });
    }
    Ok(match i.code() {
        Code::Cld => Operation::Direction(false),
        Code::Std => Operation::Direction(true),
        Code::Nopd | Code::Nopw => Operation::Nop,
        Code::Push_r16 | Code::Push_rm16 | Code::Push_imm16 | Code::Pushw_imm8 => {
            Operation::Push(operand(i, 0)?, Width::Word)
        }
        Code::Push_r32 | Code::Push_rm32 | Code::Pushd_imm32 | Code::Pushd_imm8 => {
            Operation::Push(operand(i, 0)?, Width::Dword)
        }
        Code::Pop_r16 | Code::Pop_rm16 => Operation::Pop(operand(i, 0)?, Width::Word),
        Code::Pop_r32 | Code::Pop_rm32 => Operation::Pop(operand(i, 0)?, Width::Dword),
        Code::Leaved => Operation::Leave,
        Code::Jmp_rel8_32 | Code::Jmp_rel32_32 => Operation::Jump(i.near_branch32()),
        Code::Call_rel32_32 => Operation::Call(i.near_branch32()),
        Code::Call_rm32 => Operation::Indirect {
            call: true,
            target: operand(i, 0)?,
        },
        Code::Jmp_rm32 => Operation::Indirect {
            call: false,
            target: operand(i, 0)?,
        },
        Code::Retnd => Operation::Ret(0),
        Code::Retnd_imm16 => Operation::Ret(i.immediate16()),
        Code::Rdtsc => Operation::ReadTimestamp,
        Code::Cli => Operation::ClearInterrupts,
        Code::Hlt => Operation::Halt,
        Code::Int3 => Operation::Breakpoint,
        _ => {
            return Err(fail(
                pc,
                format!("instrução ainda não suportada: {:?}", i.code()),
            ));
        }
    })
}

fn lower_condition(i: &Instruction) -> Result<Condition> {
    Ok(match i.condition_code() {
        ConditionCode::o => Condition::Overflow,
        ConditionCode::no => Condition::NotOverflow,
        ConditionCode::b => Condition::Below,
        ConditionCode::ae => Condition::AboveEqual,
        ConditionCode::e => Condition::Equal,
        ConditionCode::ne => Condition::NotEqual,
        ConditionCode::be => Condition::BelowEqual,
        ConditionCode::a => Condition::Above,
        ConditionCode::s => Condition::Sign,
        ConditionCode::ns => Condition::NotSign,
        ConditionCode::p => Condition::Parity,
        ConditionCode::np => Condition::NotParity,
        ConditionCode::l => Condition::Less,
        ConditionCode::ge => Condition::GreaterEqual,
        ConditionCode::le => Condition::LessEqual,
        ConditionCode::g => Condition::Greater,
        ConditionCode::None => return Err(fail(i.ip32(), "condição inválida")),
    })
}

fn register(r: Register, pc: u32) -> Result<usize> {
    match r {
        Register::EAX => Ok(0),
        Register::ECX => Ok(1),
        Register::EDX => Ok(2),
        Register::EBX => Ok(3),
        Register::ESP => Ok(4),
        Register::EBP => Ok(5),
        Register::ESI => Ok(6),
        Register::EDI => Ok(7),
        _ => Err(fail(pc, format!("registrador não suportado: {r:?}"))),
    }
}

fn operand(i: &Instruction, n: u32) -> Result<Operand> {
    let pc = i.ip32();
    Ok(match i.op_kind(n) {
        OpKind::Register => {
            let r = i.op_register(n);
            if let Ok(index) = register(r, pc) {
                Operand::Reg(index)
            } else {
                let (index, shift, width) = match r {
                    Register::AL => (0, 0, Width::Byte),
                    Register::CL => (1, 0, Width::Byte),
                    Register::DL => (2, 0, Width::Byte),
                    Register::BL => (3, 0, Width::Byte),
                    Register::AH => (0, 8, Width::Byte),
                    Register::CH => (1, 8, Width::Byte),
                    Register::DH => (2, 8, Width::Byte),
                    Register::BH => (3, 8, Width::Byte),
                    Register::AX => (0, 0, Width::Word),
                    Register::CX => (1, 0, Width::Word),
                    Register::DX => (2, 0, Width::Word),
                    Register::BX => (3, 0, Width::Word),
                    Register::SP => (4, 0, Width::Word),
                    Register::BP => (5, 0, Width::Word),
                    Register::SI => (6, 0, Width::Word),
                    Register::DI => (7, 0, Width::Word),
                    _ => return Err(fail(pc, format!("registrador não suportado: {r:?}"))),
                };
                Operand::Part {
                    index,
                    shift,
                    width,
                }
            }
        }
        OpKind::Immediate8 => Operand::Imm(u32::from(i.immediate8())),
        OpKind::Immediate16 => Operand::Imm(u32::from(i.immediate16())),
        OpKind::Immediate32 => Operand::Imm(i.immediate32()),
        OpKind::Immediate8to16 => Operand::Imm(u32::from(i.immediate8to16().cast_unsigned())),
        OpKind::Immediate8to32 => Operand::Imm(i.immediate8to32().cast_unsigned()),
        OpKind::Memory => {
            let width = match i.memory_size().size() {
                1 => Width::Byte,
                2 => Width::Word,
                4 => Width::Dword,
                _ => return Err(fail(pc, "largura de memória não suportada")),
            };
            memory(i, width)?
        }
        kind => return Err(fail(pc, format!("operando não suportado: {kind:?}"))),
    })
}

fn memory(i: &Instruction, width: Width) -> Result<Operand> {
    let pc = i.ip32();
    if i.memory_displ_size() == 2 {
        return Err(fail(pc, "endereçamento de 16 bits não suportado"));
    }
    let reg = |r| {
        if r == Register::None {
            Ok(None)
        } else {
            register(r, pc).map(Some)
        }
    };
    Ok(Operand::Mem {
        fs: i.segment_prefix() == Register::FS,
        base: reg(i.memory_base())?,
        index: reg(i.memory_index())?,
        scale: i.memory_index_scale(),
        displacement: i.memory_displacement32(),
        width,
    })
}

/// O mesmo layout deve ser usado na análise e no carregamento. Seções sobrepostas
/// poderiam fazer o código compilado divergir dos bytes efetivamente carregados.
pub(crate) fn validate_layout(xbe: &Xbe<'_>) -> Result<()> {
    xbox_kernel::loader::validate_startup_layout(xbe)?;
    Ok(())
}
