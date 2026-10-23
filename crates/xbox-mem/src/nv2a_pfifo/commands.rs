//! Pusher síncrono limitado por palavras; memória vem das visões únicas do Bus.
use super::{DMA_GET, DMA_PUT, Nv2aPfifo};
use crate::{Arena, Nv2aPramin, nv2a_pgraph::GpuMemory};
use xbox_gpu::pgraph::Nv2aPgraph;
use xbox_gpu::pushbuffer::{Header, decode};

pub(super) const MAX_METHOD_RECORDS: usize = 4096;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GpuFault {
    pub get: u32,
    pub instance: u32,
    pub detail: &'static str,
}
impl std::fmt::Display for GpuFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "PFIFO GET=0x{:08X}, instância=0x{:X}: {}",
            self.get, self.instance, self.detail
        )
    }
}
impl std::error::Error for GpuFault {}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StepReport {
    pub words: u32,
    pub drained: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MethodRecord {
    pub class: u32,
    pub subchannel: u8,
    pub method: u16,
    pub count: u64,
    pub first: u32,
    pub last: u32,
    pub first_resolved: u32,
    pub last_resolved: u32,
}
impl MethodRecord {
    pub(super) fn key(self) -> (u32, u8, u16) {
        (self.class, self.subchannel, self.method)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Binding {
    pub handle: u32,
    pub instance: u32,
    pub class: u32,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct CommandState {
    pub bindings: [Option<Binding>; 8],
    pub methods: Vec<MethodRecord>,
}
impl Nv2aPfifo {
    fn value(&self, off: u32) -> u32 {
        self.register(off).expect("controle do pusher")
    }
    fn set(&mut self, off: u32, v: u32) {
        let i = super::REGISTERS
            .binary_search(&off)
            .expect("controle do pusher");
        self.registers.as_mut().expect("perfil ativo")[i] = v;
    }
    pub fn methods(&self) -> &[MethodRecord] {
        self.commands.as_ref().map_or(&[], |s| s.methods.as_slice())
    }
    fn fault(&self, detail: &'static str) -> GpuFault {
        GpuFault {
            get: self.value(DMA_GET),
            instance: self.value(0x322C),
            detail,
        }
    }
    fn enabled(&self) -> bool {
        self.commands.is_some()
            && self.value(0x3200) & 1 != 0
            && self.value(0x3220) & 0x1001 == 1
            && self.value(0x3250) & 1 != 0
            && self.value(0x3204) & 0x100 != 0
    }
    pub(crate) fn run(
        &mut self,
        arena: &mut Arena,
        pramin: &Nv2aPramin,
        graph: &mut Nv2aPgraph,
        budget: u32,
    ) -> Result<StepReport, GpuFault> {
        if !self.enabled() {
            return Ok(StepReport::default());
        }
        if self.value(0x3204) & 0x1F != 0 || self.value(0x2504) & 1 == 0 {
            return Err(self.fault("consumidor suporta apenas canal DMA zero"));
        }
        let mut report = StepReport::default();
        let mut memory = GpuMemory { arena, pramin };
        while report.words < budget && self.value(DMA_GET) != self.value(DMA_PUT) && graph.ready() {
            if self.value(DMA_PUT) & 3 != 0 || self.value(DMA_GET) & 3 != 0 {
                return Err(self.fault("GET/PUT desalinhados"));
            }
            let word = self
                .fetch(memory.arena.bytes(), pramin)
                .map_err(|d| self.fault(d))?;
            let next = self
                .value(DMA_GET)
                .checked_add(4)
                .ok_or_else(|| self.fault("GET transborda"))?;
            let state = self.value(0x3228);
            if state & !0x1FFC_FFFD != 0 {
                return Err(self.fault("DMA_STATE contém campos sem modelo"));
            }
            if (state >> 18) & 0x7FF != 0 {
                self.parameter(pramin, word, state, graph, &mut memory)
                    .map_err(|d| self.fault(d))?;
                self.set(DMA_GET, next);
            } else {
                self.header(word, next).map_err(|d| self.fault(d))?;
            }
            report.words += 1;
        }
        report.drained =
            self.value(DMA_GET) == self.value(DMA_PUT) && self.value(0x3228) & 0x1FFC0000 == 0;
        Ok(report)
    }
    fn fetch(&self, ram: &[u8], pramin: &Nv2aPramin) -> Result<u32, &'static str> {
        let inst = self.value(0x322C);
        if inst > 0xFFFF {
            return Err("instância DMA fora da PRAMIN");
        }
        let base = inst << 4;
        let flags = pramin
            .read(0x700000 + base)
            .ok_or("descritor DMA ausente")?;
        let limit = pramin.read(0x700004 + base).ok_or("limite DMA ausente")?;
        let frame = pramin.read(0x700008 + base).ok_or("frame DMA ausente")?;
        if !matches!(flags & 0xFFF, 2 | 0x3D) || !matches!((flags >> 16) & 3, 0 | 2) {
            return Err("classe ou alvo DMA sem modelo para leitura linear UMA");
        }
        let address = (frame & 0xFFFFF000) | (flags >> 20);
        let get = self.value(DMA_GET);
        if u64::from(get) + 3 > u64::from(limit) {
            return Err("GET excede limite inclusivo do descritor DMA");
        }
        let physical = u64::from(address) + u64::from(get);
        if physical & 3 != 0 {
            return Err("endereço físico DMA desalinhado");
        }
        let at = usize::try_from(physical).map_err(|_| "endereço físico DMA excede host")?;
        let bytes = ram
            .get(at..at.checked_add(4).ok_or("endereço DMA transborda")?)
            .ok_or("leitura DMA fora da RAM")?;
        Ok(u32::from_le_bytes(bytes.try_into().expect("DWORD")))
    }
    fn header(&mut self, word: u32, next: u32) -> Result<(), &'static str> {
        match decode(word)? {
            Header::Jump(target) => self.set(DMA_GET, target),
            Header::Call(target) => {
                if self.value(0x324C) & 1 != 0 {
                    return Err("call DMA aninhado não permitido");
                }
                if next & !0x1FFF_FFFC != 0 {
                    return Err("retorno DMA fora do campo de endereço");
                }
                self.set(0x324C, next | 1);
                self.set(DMA_GET, target);
            }
            Header::Return => {
                let sub = self.value(0x324C);
                if sub & 1 == 0 {
                    return Err("return DMA sem call ativo");
                }
                self.set(DMA_GET, sub & 0x1FFF_FFFC);
                self.set(0x324C, 0);
            }
            Header::Methods {
                method,
                subchannel,
                count,
                non_incrementing,
            } => {
                self.set(
                    0x3228,
                    u32::from(method)
                        | (u32::from(subchannel) << 13)
                        | (u32::from(count) << 18)
                        | u32::from(non_incrementing),
                );
                self.set(DMA_GET, next);
            }
        }
        Ok(())
    }
    fn parameter(
        &mut self,
        pramin: &Nv2aPramin,
        word: u32,
        state: u32,
        graph: &mut Nv2aPgraph,
        memory: &mut GpuMemory<'_>,
    ) -> Result<(), &'static str> {
        let method = (state & 0x1FFC) as u16;
        let sub = ((state >> 13) & 7) as u8;
        let count = (state >> 18) & 0x7FF;
        let binding = if method == 0 {
            self.lookup(pramin, word)?
        } else {
            self.commands.as_ref().expect("perfil").bindings[sub as usize]
                .ok_or("método sem objeto ligado ao subcanal")?
        };
        let resolved = if (0x180..0x200).contains(&method) {
            self.lookup(pramin, word)?.instance
        } else {
            word
        };
        let next_method = if state & 1 != 0 {
            method
        } else {
            method.wrapping_add(4)
        };
        if count > 1 && next_method > 0x1FFC {
            return Err("pacote ultrapassa espaço de métodos");
        }
        self.preflight_observe(binding.class, sub, method)?;
        graph.execute(binding.class, sub, method, word, resolved, memory)?;
        self.observe(binding.class, sub, method, word, resolved)?;
        if method == 0 {
            self.commands.as_mut().expect("perfil").bindings[sub as usize] = Some(binding);
        }
        self.set(
            0x3228,
            (state & !0x1FFC_1FFC) | u32::from(next_method & 0x1FFC) | ((count - 1) << 18),
        );
        Ok(())
    }
    fn lookup(&self, pramin: &Nv2aPramin, handle: u32) -> Result<Binding, &'static str> {
        let ramht = self.value(0x2210);
        let size = 4096u32 << ((ramht >> 16) & 3);
        let bits = 11 + ((ramht >> 16) & 3);
        let mut hash = 0;
        let mut rest = handle;
        while rest != 0 {
            hash ^= rest & ((1 << bits) - 1);
            rest >>= bits;
        }
        let offset = ((ramht & 0x1F0) << 8) + hash * 8;
        if hash * 8 + 8 > size {
            return Err("hash RAMHT fora da tabela");
        }
        let entry = pramin
            .read(0x700000 + offset)
            .ok_or("entrada RAMHT fora da PRAMIN")?;
        let context = pramin
            .read(0x700004 + offset)
            .ok_or("contexto RAMHT fora da PRAMIN")?;
        if entry != handle
            || context & 0x80000000 == 0
            || context & 0x1F000000 != 0
            || (context >> 16) & 3 > 1
        {
            return Err("handle RAMHT inválido, canal ou engine sem modelo");
        }
        let instance = (context & 0xFFFF) << 4;
        let class = pramin
            .read(0x700000 + instance)
            .ok_or("objeto RAMHT fora da PRAMIN")?
            & 0xFFF;
        Ok(Binding {
            handle,
            instance,
            class,
        })
    }
    fn preflight_observe(&self, class: u32, sub: u8, method: u16) -> Result<(), &'static str> {
        let records = &self.commands.as_ref().expect("perfil").methods;
        match records.binary_search_by_key(&(class, sub, method), |r| r.key()) {
            Ok(i) if records[i].count == u64::MAX => Err("contador de métodos esgotado"),
            Err(_) if records.len() >= MAX_METHOD_RECORDS => {
                Err("limite de métodos distintos atingido")
            }
            _ => Ok(()),
        }
    }
    fn observe(
        &mut self,
        class: u32,
        sub: u8,
        method: u16,
        raw: u32,
        resolved: u32,
    ) -> Result<(), &'static str> {
        let records = &mut self.commands.as_mut().expect("perfil").methods;
        match records.binary_search_by_key(&(class, sub, method), |r| r.key()) {
            Ok(i) => {
                let r = &mut records[i];
                r.count = r
                    .count
                    .checked_add(1)
                    .ok_or("contador de métodos esgotado")?;
                r.last = raw;
                r.last_resolved = resolved;
            }
            Err(i) => {
                if records.len() >= MAX_METHOD_RECORDS {
                    return Err("limite de métodos distintos atingido");
                }
                records.insert(
                    i,
                    MethodRecord {
                        class,
                        subchannel: sub,
                        method,
                        count: 1,
                        first: raw,
                        last: raw,
                        first_resolved: resolved,
                        last_resolved: resolved,
                    },
                );
            }
        }
        Ok(())
    }
}
