//! Estado explícito e loader experimental, sem decodificação durante a execução.

use xbox_core::Clock;
use xbox_core::error::SnapshotError;
use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_formats::Xbe;
use xbox_kernel::dispatch::{CHILD_THREAD, ThreadPhase};
use xbox_mem::{Acesso, Bus, GuestVirt};

pub use drive::{Desfecho, Limites};

use crate::Result;
use crate::ir::{Condition, DoubleShift, Rotate, Shift, Width};

mod drive;
mod interrupts;
mod msr;
mod muldiv;
pub mod sse;
mod threads;
pub mod x87;
pub use msr::CpuProfile;
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
/// Interrupt-enable flag do guest, independente das flags do processo host.
pub const INTERRUPT_FLAG: u32 = 1 << 9;
/// Política AOT v1: custo sintético por instrução ou elemento REP concluído.
/// Não representa latência física do Pentium III, nem tempo de parede do host.
pub const AOT_STEP_CYCLES: u64 = 1;
const TIMING_PROFILE: u32 = 1;
/// Valor de MXCSR após RESET/FNINIT: todas as exceções mascaradas, arredondamento ao mais próximo.
pub const MXCSR_RESET: u32 = 0x1F80;

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
    /// Relógio único da sessão AOT atual. Não copiar para contextos de threads
    /// ao introduzir o scheduler: todos devem compartilhar o mesmo tempo guest.
    pub clock: Clock,
    /// PC do HLT concluído. EIP aponta à instrução seguinte; polls não retomam
    /// a CPU. Não há entrega de IRQ/NMI neste perfil ainda.
    pub halted: Option<u32>,
    /// PC de INT3 que gerou #BP. EIP já aponta à instrução seguinte; a exceção
    /// continua pendente até existir tratamento explícito, não por novo poll.
    pub breakpoint: Option<u32>,
    /// Configuração funcional de CPU/MSRs guest; independente da CPU host.
    pub cpu_profile: CpuProfile,
    /// XMM0..XMM7 como quatro palavras de 32 bits (bits crus, sem `f32`, para igualdade e
    /// snapshot exatos, inclusive NaN). A palavra 0 é a baixa.
    pub xmm: [[u32; 4]; 8],
    /// MXCSR. Só o valor inicial é modelado (ADR-0053, P8).
    pub mxcsr: u32,
    /// Pilha x87 de 80 bits. Só carga/armazenamento são modelados (ADR-0053, P9).
    pub fpu: x87::Fpu,
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
            clock: Clock::new(),
            halted: None,
            breakpoint: None,
            cpu_profile: CpuProfile::XboxRetailV1,
            xmm: [[0; 4]; 8],
            mxcsr: MXCSR_RESET,
            fpu: x87::Fpu::new(),
        }
    }

    /// Um limite de representação é erro do runtime, antes de qualquer efeito
    /// da próxima instrução. Evita overflow/pânico ou wrap silencioso do relógio.
    pub fn check_time_step(&self) -> Result<()> {
        self.clock
            .cycles()
            .checked_add(AOT_STEP_CYCLES)
            .ok_or(crate::Error::TimeExhausted { pc: self.eip })?;
        Ok(())
    }

    pub fn halted_exit(&self) -> Option<Exit> {
        self.halted.map(|pc| Exit::Halted {
            pc,
            interrupts_enabled: self.eflags & INTERRUPT_FLAG != 0,
        })
    }

    /// Fronteiras persistentes, inclusive quando o orçamento de execução é zero.
    pub fn stopped_exit(&self) -> Option<Exit> {
        if self.returned {
            Some(Exit::Returned)
        } else if let Some(pc) = self.breakpoint {
            Some(Exit::Breakpoint { pc })
        } else {
            self.halted_exit()
        }
    }
}

impl Snapshot for State {
    fn save(&self, out: &mut SnapshotWriter) {
        out.section("AOTC", 8);
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
        out.u32(TIMING_PROFILE);
        self.clock.save(out);
        out.bool(self.halted.is_some());
        if let Some(pc) = self.halted {
            out.u32(pc);
        }
        out.bool(self.breakpoint.is_some());
        if let Some(pc) = self.breakpoint {
            out.u32(pc);
        }
        out.u32(self.cpu_profile as u32);
        out.u32(self.mxcsr);
        for lane in self.xmm.iter().flatten() {
            out.u32(*lane);
        }
        self.fpu.save(out);
    }

    fn load(&mut self, inp: &mut SnapshotReader<'_>) -> std::result::Result<(), SnapshotError> {
        let version = inp.section("AOTC", 8)?;
        if version == 0 {
            return Err(SnapshotError::BadValue {
                section: "AOTC",
                what: "versão de CPU AOT desconhecida",
            });
        }
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
        // As versões anteriores não guardavam tempo. Não inferir ciclos a partir
        // de retired: elementos REP parciais ainda não aposentaram a instrução.
        self.clock = Clock::new();
        if version >= 4 {
            if inp.u32()? != TIMING_PROFILE {
                return Err(SnapshotError::BadValue {
                    section: "AOTC",
                    what: "perfil de tempo AOT desconhecido",
                });
            }
            self.clock.load(inp)?;
        }
        self.halted = if version >= 5 && inp.bool()? {
            Some(inp.u32()?)
        } else {
            None
        };
        self.breakpoint = if version >= 6 && inp.bool()? {
            Some(inp.u32()?)
        } else {
            None
        };
        if self.breakpoint.is_some() && (self.halted.is_some() || self.returned) {
            return Err(SnapshotError::BadValue {
                section: "AOTC",
                what: "breakpoint incompatível com CPU suspensa ou retornada",
            });
        }
        // v1–v6 não liam MSRs: migração explícita para o primeiro perfil funcional.
        self.cpu_profile = if version >= 7 {
            CpuProfile::from_id(inp.u32()?)?
        } else {
            CpuProfile::XboxRetailV1
        };
        // v1–v7 não tinham SSE: migram para o estado de RESET, sem inventar valores.
        if version >= 8 {
            self.mxcsr = inp.u32()?;
            for lane in self.xmm.iter_mut().flatten() {
                *lane = inp.u32()?;
            }
            self.fpu.load(inp)?;
        } else {
            self.mxcsr = MXCSR_RESET;
            self.xmm = [[0; 4]; 8];
            self.fpu = x87::Fpu::new();
        }
        Ok(())
    }
}

/// Saída em fronteira de bloco ou entre elementos REP; sem continuação no host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    Returned,
    Budget,
    /// #BP (vetor 3) pendente. O host não entregou a exceção ao kernel guest.
    Breakpoint {
        pc: u32,
    },
    /// CPU suspensa; não equivale a retorno do título ou orçamento esgotado.
    Halted {
        pc: u32,
        interrupts_enabled: bool,
    },
    /// Fronteira explícita; o serviço ainda precisa ser executado pelo host HLE.
    KernelCall {
        ordinal: u32,
    },
}

/// Como terminou um atendimento de kernel pela sessão.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Servico {
    /// O serviço rodou e o retorno foi aplicado.
    Concluido,
    /// A espera não termina sem tempo e interrupções: o retorno **não** foi aplicado e a
    /// CPU continua na chamada pendente.
    Bloqueado,
}

/// Sessão AOT com CPU, memória e estado HLE. O snapshot da sessão inclui os três.
#[derive(Debug)]
pub struct Session {
    pub state: State,
    pub bus: Bus,
    pub kernel: xbox_kernel::dispatch::Dispatcher,
    parent: Option<threads::ThreadContext>,
    /// Vblanks já gerados pelo relógio guest (ADR-0055).
    vblanks: u64,
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
        Ok(Self {
            state,
            bus,
            parent: None,
            vblanks: 0,
            kernel: xbox_kernel::dispatch::Dispatcher::for_image(
                xbe.header.base_address,
                xbe.header.size_of_image,
            )?,
        })
    }

    /// Copia o relógio guest para `KeTickCount`, o dado que o título lê direto da memória.
    ///
    /// Chamada onde o relógio muda por ação do host (espera atendida) e a cada fim de
    /// fatia. Entre uma chamada e outra o contador fica parado — provisório, P7.
    pub fn publicar_tick_count(&mut self) -> Result<()> {
        xbox_kernel::loader::data::publish_tick_count(
            &mut self.bus,
            self.state.clock.tick_count_ms(),
        )?;
        Ok(())
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
    /// que o snapshot da sessão já exige. Assim um serviço faltante pode ser implementado
    /// e a execução retomada do snapshot, sem estado escondido no host.
    pub fn atender_kernel(&mut self) -> Result<()> {
        match self.atender_kernel_ou_bloquear()? {
            Servico::Concluido => Ok(()),
            // Quem chama aqui não entrega interrupções; seguir faria a mesma chamada
            // bloquear para sempre.
            Servico::Bloqueado => Err(crate::Error::WouldBlock { pc: self.state.eip }),
        }
    }

    /// Como [`Self::atender_kernel`], mas uma espera que só termina com tempo e
    /// interrupções devolve [`Servico::Bloqueado`] **sem aplicar o retorno**: a CPU segue
    /// na chamada pendente e o mesmo atendimento pode ser repetido depois.
    pub fn atender_kernel_ou_bloquear(&mut self) -> Result<Servico> {
        use xbox_kernel::dispatch::ORDINAL_PS_TERMINATE_SYSTEM_THREAD;
        if self.state.stopped_exit().is_some() {
            return Err(
                xbox_core::Error::format("CPU parada não pode atender novo serviço").into(),
            );
        }
        let call = self.kernel_call()?;
        let terminating = call.export.ordinal == ORDINAL_PS_TERMINATE_SYSTEM_THREAD;
        if terminating {
            if self.parent.is_none() {
                return Err(
                    xbox_kernel::dispatch::Erro::ThreadState("término sem pai suspenso").into(),
                );
            }
        } else {
            self.validate_kernel_return(&call)?;
        }
        let atendimento = self.kernel.atender_com_clock(
            &mut self.bus,
            self.state.eip,
            &self.state.regs,
            &self.state.clock,
        )?;
        // O serviço pode escrever sobre a pilha de argumentos/retorno. Usar a
        // continuação capturada, sem nova leitura falível depois de publicar efeitos.
        if terminating {
            // O kernel já validou o término; não retornar para código após noreturn.
            if let Some(parent) = self.parent.take() {
                parent.restore(&mut self.state);
            }
        } else {
            // Um serviço de espera deixa um pedido de avanço de tempo. O Clock é da
            // sessão (ADR-0025), então só aqui ele avança — e a checagem de overflow vem
            // **antes** de aplicar o retorno, para que `TimeExhausted` deixe a CPU na
            // chamada pendente, como qualquer outra recusa. O pedido é consumido mesmo na
            // falha: deixá-lo pendente faria o próximo atendimento aplicar um avanço
            // velho.
            if let Some(pedido) = self.kernel.take_time_request() {
                let avanco = self.state.clock.cycles().checked_add(pedido.ciclos);
                let Some(alvo) = avanco else {
                    return Err(crate::Error::TimeExhausted { pc: self.state.eip });
                };
                self.state.clock.set(alvo);
                self.publicar_tick_count()?;
            }
            if self.kernel.take_block_request() {
                return Ok(Servico::Bloqueado);
            }
            self.apply_kernel_return(&call, atendimento.eax);
            if let Some(child) = self.kernel.start_ready_thread() {
                self.parent = Some(threads::ThreadContext::capture(&self.state));
                self.state.regs = [0; 8];
                self.state.regs[ESP] = child.esp();
                self.state.eip = child.entry;
                self.state.eflags = 2 | INTERRUPT_FLAG;
                self.state.fs_base = child.tib();
                self.state.repeat = None;
                self.state.returned = false;
            }
        }
        Ok(Servico::Concluido)
    }

    pub fn has_parked_parent(&self) -> bool {
        self.parent.is_some()
    }

    /// RET do `SystemRoutine` é término local da filha; o main deve consultar esta
    /// fronteira antes de anunciar que o processo inteiro retornou.
    pub fn complete_thread_return(&mut self) -> Result<bool> {
        if !self.state.returned || self.parent.is_none() {
            return Ok(false);
        }
        self.kernel.terminate_running_thread(self.state.regs[EAX])?;
        if let Some(parent) = self.parent.take() {
            parent.restore(&mut self.state);
        }
        Ok(true)
    }

    /// O dispatcher chama isto somente após implementar o serviço correspondente.
    /// Cdecl preserva argumentos; stdcall/fastcall removem só argumentos de pilha.
    pub fn complete_kernel_call(&mut self, ordinal: u32, eax: u32) -> Result<()> {
        let call = self.kernel_call()?;
        if call.export.ordinal != ordinal {
            return Err(xbox_core::Error::format("resposta de outro ordinal de kernel").into());
        }
        self.validate_kernel_return(&call)?;
        self.apply_kernel_return(&call, eax);
        Ok(())
    }

    fn validate_kernel_return(&self, call: &xbox_kernel::imports::Call) -> Result<()> {
        if call.return_pc != RETURN_SENTINEL {
            self.bus
                .traduzir(GuestVirt(call.return_pc), Acesso::Execucao)?;
        }
        Ok(())
    }

    fn apply_kernel_return(&mut self, call: &xbox_kernel::imports::Call, eax: u32) {
        self.state.regs[EAX] = eax;
        self.state.regs[ESP] = call.return_esp;
        self.state.eip = call.return_pc;
        self.state.returned = call.return_pc == RETURN_SENTINEL;
    }
}

impl Snapshot for Session {
    fn save(&self, out: &mut SnapshotWriter) {
        out.section("AOTS", 3);
        self.state.save(out);
        self.bus.save(out);
        self.kernel.save(out);
        out.bool(self.parent.is_some());
        if let Some(parent) = &self.parent {
            parent.save(out);
        }
        out.u64(self.vblanks);
    }

    fn load(&mut self, inp: &mut SnapshotReader<'_>) -> std::result::Result<(), SnapshotError> {
        let version = inp.section("AOTS", 3)?;
        if !matches!(version, 1..=3) {
            return Err(SnapshotError::BadValue {
                section: "AOTS",
                what: "versão de sessão desconhecida",
            });
        }
        let original_image = self.kernel.virtual_memory().image();
        self.state.load(inp)?;
        self.bus.load(inp)?;
        self.kernel.load(inp)?;
        self.kernel
            .restore_memory_context(original_image, &self.bus)?;
        self.parent = if version >= 2 && inp.bool()? {
            let mut context = threads::ThreadContext::capture(&State::new(0));
            context.load(inp)?;
            context.validate(&self.bus)?;
            Some(context)
        } else {
            None
        };
        if (self.kernel.current_thread() == CHILD_THREAD) != self.parent.is_some()
            || self
                .kernel
                .child_thread()
                .is_some_and(|child| child.phase == ThreadPhase::Ready)
        {
            return Err(SnapshotError::BadValue {
                section: "AOTS",
                what: "CPU, thread ativa e pai suspenso incoerentes",
            });
        }
        if let Some(child) = self.kernel.child_thread().filter(|_| self.parent.is_some())
            && (self.state.fs_base != child.tib()
                || !(child.region.base..=child.stack_top()).contains(&self.state.regs[ESP]))
        {
            return Err(SnapshotError::BadValue {
                section: "AOTS",
                what: "CPU ativa diverge do TLS/pilha da filha",
            });
        }
        // v1–v2 não guardavam vblanks: parte do relógio, sem inventar atraso acumulado.
        self.vblanks = if version >= 3 {
            inp.u64()?
        } else {
            self.state.clock.cycles() / Self::vblank_period()
        };
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
