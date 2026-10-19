//! Estado explícito e loader experimental, sem decodificação durante a execução.

use std::collections::BTreeMap;

use xbox_core::error::SnapshotError;
use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_formats::Xbe;
use xbox_formats::xbe::SectionFlags;
use xbox_mem::{Bus, GuestVirt, Perm};

use crate::Result;
use crate::ir::{Condition, Width};

/// Retorno do entry point ao chamador do host; nunca é executado como código.
pub const RETURN_SENTINEL: u32 = 0xFFFF_FFF0;
/// Índice de EAX na ordem x86 dos registradores.
pub const EAX: usize = 0;
/// Índice de ESP na ordem x86 dos registradores.
pub const ESP: usize = 4;
const STACK_BASE: u32 = 0x7000_0000;
const STACK_SIZE: u32 = 16 * 4096;

/// CPU inteira mínima. Não representa ainda x87, SSE ou o sistema de threads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    /// EAX, ECX, EDX, EBX, ESP, EBP, ESI, EDI.
    pub regs: [u32; 8],
    pub eip: u32,
    pub eflags: u32,
    /// Instruções concluídas, sem pretensão de representar ciclos do Pentium III.
    pub retired: u64,
    pub returned: bool,
}

impl State {
    pub const fn new(entry: u32) -> Self {
        Self {
            regs: [0; 8],
            eip: entry,
            eflags: 2,
            retired: 0,
            returned: false,
        }
    }
}

impl Snapshot for State {
    fn save(&self, out: &mut SnapshotWriter) {
        out.section("AOTC", 1);
        for r in self.regs {
            out.u32(r);
        }
        out.u32(self.eip);
        out.u32(self.eflags);
        out.u64(self.retired);
        out.bool(self.returned);
    }

    fn load(&mut self, inp: &mut SnapshotReader<'_>) -> std::result::Result<(), SnapshotError> {
        inp.section("AOTC", 1)?;
        for r in &mut self.regs {
            *r = inp.u32()?;
        }
        self.eip = inp.u32()?;
        self.eflags = inp.u32()?;
        self.retired = inp.u64()?;
        self.returned = inp.bool()?;
        Ok(())
    }
}

/// Saída em fronteira de bloco: nenhuma continuação fica na pilha do host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    Returned,
    Budget,
}

/// Sessão de teste: loader de seções e pilha; kernel/TLS ainda não implementados.
#[derive(Debug)]
pub struct Session {
    pub state: State,
    pub bus: Bus,
}

impl Session {
    pub fn load(bytes: &[u8]) -> Result<Self> {
        let xbe = Xbe::parse(bytes)?;
        crate::analysis::validate_layout(&xbe)?;
        if xbe.tls.is_some() || !xbe.kernel_imports()?.is_empty() {
            return Err(xbox_core::Error::not_implemented(
                "TLS e importações do kernel no loader AOT",
                "Fase 4",
            )
            .into());
        }
        let mut bus = Bus::novo()?;
        let mut pages = BTreeMap::<u32, Perm>::new();
        add_pages(
            &mut pages,
            xbe.header.base_address,
            xbe.header.size_of_headers,
            Perm::RO,
            bus.ram(),
        )?;
        for s in &xbe.sections {
            if s.raw_size > s.virtual_size {
                return Err(xbox_core::Error::format(
                    "seção AOT com raw_size maior que virtual_size",
                )
                .into());
            }
            let mut perm = Perm::RO;
            if s.flags.tem(SectionFlags::EXECUTABLE) {
                perm.0 |= Perm::EXECUTAR;
            }
            if s.flags.tem(SectionFlags::WRITABLE) {
                perm.0 |= Perm::ESCREVER;
            }
            if perm.0 & (Perm::ESCREVER | Perm::EXECUTAR) == (Perm::ESCREVER | Perm::EXECUTAR) {
                return Err(xbox_core::Error::format(
                    "AOT inicial não aceita seção executável gravável",
                )
                .into());
            }
            add_pages(
                &mut pages,
                s.virtual_address,
                s.virtual_size,
                perm,
                bus.ram(),
            )?;
        }
        add_pages(&mut pages, STACK_BASE, STACK_SIZE, Perm::RW, bus.ram())?;
        for va in pages.keys() {
            let frame = bus
                .frames_mut()
                .alocar()
                .ok_or_else(|| xbox_core::Error::format("RAM insuficiente para o XBE"))?;
            bus.mapear(GuestVirt(*va), frame, 1, Perm::RWX)?;
        }
        let headers = bytes
            .get(..xbe.header.size_of_headers as usize)
            .ok_or_else(|| xbox_core::Error::format("headers XBE truncados"))?;
        bus.escrever_bytes(GuestVirt(xbe.header.base_address), headers)?;
        for (i, s) in xbe.sections.iter().enumerate() {
            bus.escrever_bytes(GuestVirt(s.virtual_address), xbe.section_data(i)?)?;
        }
        for (va, perm) in pages {
            if perm.0 & (Perm::ESCREVER | Perm::EXECUTAR) == (Perm::ESCREVER | Perm::EXECUTAR) {
                return Err(xbox_core::Error::format(
                    "página AOT compartilhada entre dados graváveis e código",
                )
                .into());
            }
            let frame = bus.traduzir(GuestVirt(va), xbox_mem::Acesso::Leitura)?;
            bus.mapear(GuestVirt(va), frame, 1, perm)?;
        }
        let mut state = State::new(xbe.entry_point);
        state.regs[ESP] = STACK_BASE + STACK_SIZE - 4;
        bus.escrever_u32(GuestVirt(state.regs[ESP]), RETURN_SENTINEL)?;
        Ok(Self { state, bus })
    }
}

fn add_pages(
    pages: &mut BTreeMap<u32, Perm>,
    start: u32,
    size: u32,
    perm: Perm,
    ram: usize,
) -> Result<()> {
    if size == 0 {
        return Ok(());
    }
    let end = start
        .checked_add(size - 1)
        .ok_or_else(|| xbox_core::Error::format("seção XBE transborda"))?;
    if u64::from(end / 4096 - start / 4096 + 1) > (ram / 4096) as u64 {
        return Err(xbox_core::Error::format("seção XBE maior que a RAM disponível").into());
    }
    for page in start / 4096..=end / 4096 {
        pages
            .entry(page * 4096)
            .and_modify(|p| p.0 |= perm.0)
            .or_insert(perm);
        if pages.len() > ram / 4096 {
            return Err(xbox_core::Error::format("imagem XBE maior que a RAM disponível").into());
        }
    }
    Ok(())
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

/// XOR/TEST limpam CF/OF. AF indefinido é normalizado para zero.
pub fn logic_flags(old: u32, result: u32) -> u32 {
    logic_flags_width(old, result, Width::Dword)
}

pub fn logic_flags_width(old: u32, result: u32, width: Width) -> u32 {
    common_flags(old, result & width.mask(), width)
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
