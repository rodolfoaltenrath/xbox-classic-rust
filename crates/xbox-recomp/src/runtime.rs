//! Estado explícito e loader experimental, sem decodificação durante a execução.

use xbox_core::error::SnapshotError;
use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_formats::Xbe;
use xbox_mem::{Acesso, Bus, GuestVirt};

use crate::Result;
use crate::ir::{Condition, DoubleShift, Rotate, Shift, Width};

mod muldiv;
pub use muldiv::{divide_values, multiply_values};
pub use xbox_kernel::imports::entry as kernel_entry;

/// RMW é indivisível sob a posse exclusiva de Bus; não há yield dentro da
/// instrução. LOCK não autoriza compartilhar a arena entre threads host.
/// MMIO exige protocolo próprio e é recusado antes de qualquer leitura.
pub fn validate_cmpxchg_ram(bus: &Bus, address: u32, width: Width) -> Result<()> {
    for offset in 0..width.bytes() {
        let va = address
            .checked_add(offset)
            .ok_or_else(|| xbox_core::Error::format("CMPXCHG transborda o endereço guest"))?;
        // CMPXCHG é uma operação de escrita inclusive no caso desigual. Uma
        // página ausente deve reportar a intenção de escrita da operação RMW.
        for access in [Acesso::Escrita, Acesso::Leitura] {
            let physical = bus.traduzir(GuestVirt(va), access)?;
            if physical.0 as usize >= bus.ram() {
                return Err(xbox_core::Error::not_implemented("CMPXCHG em MMIO", "Fase 5").into());
            }
        }
    }
    Ok(())
}

/// Retorno do entry point ao chamador do host; nunca é executado como código.
pub use xbox_kernel::loader::RETURN_SENTINEL;
/// Índice de EAX na ordem x86 dos registradores.
pub const EAX: usize = 0;
/// Índice de ESP na ordem x86 dos registradores.
pub const ESP: usize = 4;

/// Flags de entrada da instrução de comparação REP em andamento.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RepeatState {
    pub pc: u32,
    /// REPE/REPNE CMPS/SCAS restauram estas flags se houver falta (Intel SDM).
    pub eflags: u32,
}

/// CPU inteira mínima, com continuação serializável de comparação REP.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    /// EAX, ECX, EDX, EBX, ESP, EBP, ESI, EDI.
    pub regs: [u32; 8],
    pub eip: u32,
    pub eflags: u32,
    /// Instruções concluídas, sem pretensão de representar ciclos do Pentium III.
    pub retired: u64,
    pub returned: bool,
    pub repeat: Option<RepeatState>,
    /// Base do segmento FS no espaço de endereços guest, sem ponteiro host.
    pub fs_base: u32,
}

impl State {
    pub const fn new(entry: u32) -> Self {
        Self {
            regs: [0; 8],
            eip: entry,
            eflags: 2,
            retired: 0,
            returned: false,
            repeat: None,
            fs_base: 0,
        }
    }
}

impl Snapshot for State {
    fn save(&self, out: &mut SnapshotWriter) {
        out.section("AOTC", 3);
        for r in self.regs {
            out.u32(r);
        }
        out.u32(self.eip);
        out.u32(self.eflags);
        out.u64(self.retired);
        out.bool(self.returned);
        out.bool(self.repeat.is_some());
        if let Some(repeat) = self.repeat {
            out.u32(repeat.pc);
            out.u32(repeat.eflags);
        }
        out.u32(self.fs_base);
    }

    fn load(&mut self, inp: &mut SnapshotReader<'_>) -> std::result::Result<(), SnapshotError> {
        let version = inp.section("AOTC", 3)?;
        for r in &mut self.regs {
            *r = inp.u32()?;
        }
        self.eip = inp.u32()?;
        self.eflags = inp.u32()?;
        self.retired = inp.u64()?;
        self.returned = inp.bool()?;
        self.repeat = if version >= 2 && inp.bool()? {
            Some(RepeatState {
                pc: inp.u32()?,
                eflags: inp.u32()?,
            })
        } else {
            None
        };
        self.fs_base = if version >= 3 { inp.u32()? } else { 0 };
        Ok(())
    }
}

/// Saída em fronteira de bloco ou entre elementos REP; sem continuação no host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    Returned,
    Budget,
    /// Fronteira explícita; o serviço ainda precisa ser executado pelo host HLE.
    KernelCall {
        ordinal: u32,
    },
}

/// Sessão AOT com TLS inicial e fronteira ABI; serviços HLE são uma etapa separada.
#[derive(Debug)]
pub struct Session {
    pub state: State,
    pub bus: Bus,
}

impl Session {
    pub fn load(bytes: &[u8]) -> Result<Self> {
        let xbe = Xbe::parse(bytes)?;
        crate::analysis::validate_layout(&xbe)?;
        let startup = xbox_kernel::loader::Image::prepare_aot(bytes)?;
        let bus = startup.image.bus;
        let mut state = State::new(xbe.entry_point);
        state.fs_base = startup.fs_base;
        state.regs[ESP] = startup.esp;
        Ok(Self { state, bus })
    }

    pub fn kernel_call(&self) -> Result<xbox_kernel::imports::Call> {
        Ok(xbox_kernel::imports::Call::read(
            &self.bus,
            self.state.eip,
            &self.state.regs,
        )?)
    }

    /// Atende a chamada de kernel pendente e retoma a execução do guest.
    ///
    /// Em caso de erro a CPU fica **intacta**: `EIP` continua no trampolim e a chamada
    /// pendente segue reconstruível por `EIP`, registradores e RAM — o mesmo contrato
    /// que o snapshot `AOTC` já exige. Assim um serviço faltante pode ser implementado
    /// e a execução retomada do snapshot, sem estado escondido no host.
    pub fn atender_kernel(
        &mut self,
        dispatcher: &mut xbox_kernel::dispatch::Dispatcher,
    ) -> Result<()> {
        let ordinal = self.kernel_call()?.export.ordinal;
        let atendimento = dispatcher.atender(&mut self.bus, self.state.eip, &self.state.regs)?;
        self.complete_kernel_call(ordinal, atendimento.eax)
    }

    /// O dispatcher chama isto somente após implementar o serviço correspondente.
    /// Cdecl preserva argumentos; stdcall/fastcall removem só argumentos de pilha.
    pub fn complete_kernel_call(&mut self, ordinal: u32, eax: u32) -> Result<()> {
        let call = self.kernel_call()?;
        if call.export.ordinal != ordinal {
            return Err(xbox_core::Error::format("resposta de outro ordinal de kernel").into());
        }
        if call.return_pc != RETURN_SENTINEL {
            self.bus
                .traduzir(GuestVirt(call.return_pc), Acesso::Execucao)?;
        }
        self.state.regs[EAX] = eax;
        self.state.regs[ESP] = call.return_esp;
        self.state.eip = call.return_pc;
        self.state.returned = call.return_pc == RETURN_SENTINEL;
        Ok(())
    }
}

/// Flags CF/PF/AF/ZF/SF/OF de ADD/SUB de 32 bits (Intel SDM, vol. 2).
pub fn arithmetic_flags(old: u32, a: u32, b: u32, result: u32, subtract: bool) -> u32 {
    arithmetic_flags_width(old, a, b, result, subtract, Width::Dword)
}

/// Flags de ADD/SUB/CMP/NEG/INC/DEC no tamanho efetivo do operando.
pub fn arithmetic_flags_width(
    old: u32,
    a: u32,
    b: u32,
    result: u32,
    subtract: bool,
    width: Width,
) -> u32 {
    let a = a & width.mask();
    let b = b & width.mask();
    let result = result & width.mask();
    let sign = 1u32 << (width.bits() - 1);
    let carry = if subtract { a < b } else { result < a };
    let overflow = if subtract {
        ((a ^ b) & (a ^ result)) & sign != 0
    } else {
        (!(a ^ b) & (a ^ result)) & sign != 0
    };
    common_flags(old, result, width)
        | u32::from(carry)
        | (u32::from((a ^ b ^ result) & 0x10 != 0) << 4)
        | (u32::from(overflow) << 11)
}

/// Flags de ADC/SBB usando CF de entrada em `old`. OF/AF usam os operandos
/// originais e o resultado final; CF precisa da soma/subtração sem truncamento.
pub fn arithmetic_carry_flags_width(
    old: u32,
    a: u32,
    b: u32,
    result: u32,
    subtract: bool,
    width: Width,
) -> u32 {
    let lhs = u64::from(a & width.mask());
    let rhs = u64::from(b & width.mask()) + u64::from(old & 1);
    let carry = if subtract {
        lhs < rhs
    } else {
        lhs + rhs > u64::from(width.mask())
    };
    (arithmetic_flags_width(old, a, b, result, subtract, width) & !1) | u32::from(carry)
}

/// XOR/TEST limpam CF/OF. AF indefinido é normalizado para zero.
pub fn logic_flags(old: u32, result: u32) -> u32 {
    logic_flags_width(old, result, Width::Dword)
}

pub fn logic_flags_width(old: u32, result: u32, width: Width) -> u32 {
    common_flags(old, result & width.mask(), width)
}

/// Flags de SHL/SHR/SAR (Intel SDM vol. 2, ADR-0015). Contagem mascarada
/// zero preserva todas as flags. Flags indefinidas são normalizadas para zero.
pub fn shift_flags(old: u32, a: u32, result: u32, count: u32, op: Shift, width: Width) -> u32 {
    let count = count & 31;
    if count == 0 {
        return old;
    }
    let bits = width.bits();
    let a = a & width.mask();
    let result = result & width.mask();
    let carry = match op {
        Shift::Shl if count < bits => (a >> (bits - count)) & 1,
        Shift::Shr if count < bits => (a >> (count - 1)) & 1,
        Shift::Sar => (a >> (count.min(bits) - 1)) & 1,
        // CF de SHL/SHR é indefinido com count >= bits.
        Shift::Shl | Shift::Shr => 0,
    };
    let overflow = if count == 1 {
        match op {
            Shift::Shl => (result >> (bits - 1)) ^ carry,
            Shift::Shr => a >> (bits - 1),
            Shift::Sar => 0,
        }
    } else {
        0
    };
    common_flags(old, result, width) | carry | (overflow << 11)
}

/// Rotações preservam SF/ZF/AF/PF. OF indefinido é zero (ADR-0016).
/// `carry` é o bit extraído do anel rotacionado pelo emissor.
pub fn rotate_flags(
    old: u32,
    result: u32,
    carry: u32,
    count: u32,
    op: Rotate,
    width: Width,
) -> u32 {
    let count = count & 31;
    if count == 0 {
        return old;
    }
    let sign = width.bits() - 1;
    let carry = carry & 1;
    let overflow = if count == 1 {
        match op {
            Rotate::Rol | Rotate::Rcl => ((result >> sign) & 1) ^ carry,
            Rotate::Ror | Rotate::Rcr => ((result >> sign) ^ (result >> (sign - 1))) & 1,
        }
    } else {
        0
    };
    (old & !0x801) | carry | (overflow << 11)
}

/// Flags de SHLD/SHRD. O emissor recusa contagens efetivas maiores que a largura
/// antes de chamar este helper. AF e OF indefinidos são zero (ADR-0016).
pub fn double_shift_flags(
    old: u32,
    a: u32,
    result: u32,
    count: u32,
    op: DoubleShift,
    width: Width,
) -> u32 {
    let count = count & 31;
    if count == 0 {
        return old;
    }
    let a = a & width.mask();
    let result = result & width.mask();
    let carry = match op {
        DoubleShift::Shld => ((u64::from(a) << count) >> width.bits()) as u32 & 1,
        DoubleShift::Shrd => (a >> (count - 1)) & 1,
    };
    let overflow = if count == 1 {
        ((a ^ result) >> (width.bits() - 1)) & 1
    } else {
        0
    };
    common_flags(old, result, width) | carry | (overflow << 11)
}

fn common_flags(old: u32, result: u32, width: Width) -> u32 {
    (old & !0x8D5)
        | (u32::from((result as u8).count_ones().is_multiple_of(2)) << 2)
        | (u32::from(result == 0) << 6)
        | (u32::from(result & (1 << (width.bits() - 1)) != 0) << 7)
}

/// Extensão com sinal via aritmética de complemento de dois, sem casts assinados.
pub fn sign_extend(value: u32, width: Width) -> u32 {
    let sign = 1 << (width.bits() - 1);
    ((value & width.mask()) ^ sign).wrapping_sub(sign)
}

/// Predicados Jcc conforme Intel SDM, vol. 2, instrução Jcc.
pub const fn condition_met(condition: Condition, flags: u32) -> bool {
    let cf = flags & 1 != 0;
    let pf = flags & 4 != 0;
    let zf = flags & 0x40 != 0;
    let sf = flags & 0x80 != 0;
    let of = flags & 0x800 != 0;
    match condition {
        Condition::Overflow => of,
        Condition::NotOverflow => !of,
        Condition::Below => cf,
        Condition::AboveEqual => !cf,
        Condition::Equal => zf,
        Condition::NotEqual => !zf,
        Condition::BelowEqual => cf || zf,
        Condition::Above => !cf && !zf,
        Condition::Sign => sf,
        Condition::NotSign => !sf,
        Condition::Parity => pf,
        Condition::NotParity => !pf,
        Condition::Less => sf != of,
        Condition::GreaterEqual => sf == of,
        Condition::LessEqual => zf || sf != of,
        Condition::Greater => !zf && sf == of,
    }
}
