//! Recursos fisicamente contíguos com janela guest restrita às páginas possuídas.
use super::{Atendimento, Erro, Irql, Resultado, descrever_argumentos};
use crate::imports::Call;
use std::collections::BTreeSet;
use xbox_core::{Snapshot, SnapshotReader, SnapshotWriter, error::SnapshotError};
use xbox_mem::{Bus, GuestPhys, GuestVirt, Perm};

pub const CONTIGUOUS_WINDOW: u32 = 0x8000_0000;
pub const PAGE_WRITECOMBINE: u32 = 0x400;
pub const MAX_CONTIGUOUS_ALLOCATIONS: usize = 4096;
const PAGE: u32 = 4096;
const MAX_RAM: u32 = 128 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContiguousAllocation {
    pub physical: GuestPhys,
    pub size: u32,
    pub alignment: u32,
    pub protect: u32,
}
impl ContiguousAllocation {
    pub const fn address(self) -> GuestVirt {
        GuestVirt(CONTIGUOUS_WINDOW | self.physical.0)
    }
    fn valid(self) -> bool {
        self.size != 0
            && self.size.is_multiple_of(PAGE)
            && self.alignment >= PAGE
            && self.alignment.is_power_of_two()
            && self.physical.0.is_multiple_of(self.alignment)
            && protection(self.protect)
            && self
                .physical
                .0
                .checked_add(self.size)
                .is_some_and(|end| end <= MAX_RAM)
    }
}
#[derive(Debug, Default)]
pub struct ContiguousMemory {
    allocations: Vec<ContiguousAllocation>,
}
impl ContiguousMemory {
    pub fn allocations(&self) -> &[ContiguousAllocation] {
        &self.allocations
    }
    pub(super) fn allocate(&mut self, bus: &mut Bus, call: &Call, irql: Irql) -> Resultado {
        let mut args = [0; 5];
        for (i, arg) in args.iter_mut().enumerate() {
            *arg = call.argument(bus, i as u32)?;
        }
        let [requested, low, high, alignment, protect] = args;
        // Alinhamento 0 é "sem exigência": o nxdk o passa (alocação de 0x20 bytes na
        // inicialização de vídeo do `triangle`) e o piso é a página. Só o zero é
        // promovido; 16, 0x3000 e outros valores abaixo de uma página ou que não são
        // potência de dois seguem recusados, porque nenhum título os usou.
        let alignment = if alignment == 0 { PAGE } else { alignment };
        if irql != Irql::Passive
            || !protection(protect)
            || alignment < PAGE
            || !alignment.is_power_of_two()
        {
            return Err(Erro::VarianteNaoImplementada {
                nome: call.export.name,
                ordinal: call.export.ordinal,
                chamador: call.return_pc,
                detalhe: "exige Passive, alinhamento potência de dois >= 4 KiB e RW com ou sem WRITECOMBINE",
                argumentos: descrever_argumentos(bus, call),
            });
        }
        self.validate_bus(bus).map_err(Erro::MemoriaContigua)?;
        let Some(size) = align(requested, PAGE).filter(|&size| size != 0) else {
            return Ok(Atendimento { eax: 0 });
        };
        if low > high
            || size / PAGE > bus.frames().livres()
            || self.allocations.len() >= MAX_CONTIGUOUS_ALLOCATIONS
        {
            return Ok(Atendimento { eax: 0 });
        }
        let end = (u64::from(high) + 1).min(bus.ram() as u64) as u32;
        let Some(base) = candidate(bus, low, end, size, alignment) else {
            return Ok(Atendimento { eax: 0 });
        };
        let allocation = ContiguousAllocation {
            physical: GuestPhys(base),
            size,
            alignment,
            protect,
        };
        self.publish(bus, allocation)?;
        Ok(Atendimento {
            eax: allocation.address().0,
        })
    }
    pub(super) fn reserve_instance(
        &mut self,
        bus: &mut Bus,
        base: u32,
        size: u32,
    ) -> Result<bool, Erro> {
        self.validate_bus(bus).map_err(Erro::MemoriaContigua)?;
        if self.allocations.len() >= MAX_CONTIGUOUS_ALLOCATIONS
            || size / PAGE > bus.frames().livres()
            || candidate(bus, base, base + size, size, PAGE) != Some(base)
        {
            return Ok(false);
        }
        self.publish(
            bus,
            ContiguousAllocation {
                physical: GuestPhys(base),
                size,
                alignment: PAGE,
                protect: 4,
            },
        )?;
        Ok(true)
    }
    fn publish(&mut self, bus: &mut Bus, allocation: ContiguousAllocation) -> Result<(), Erro> {
        let base = allocation.physical.0;
        let size = allocation.size;
        let mut frames = bus.frames().clone();
        let mut mmu = bus.mmu().clone();
        frames
            .reservar(allocation.physical, size / PAGE)
            .map_err(crate::loader::Error::from)?;
        mmu.mapear(
            allocation.address(),
            allocation.physical,
            size / PAGE,
            Perm::RW,
        )?;
        // RAM completa, frames e mapeamentos já conferidos. Zerar é política local;
        // não há dispositivo, ponteiro guest ou store de saída após o commit.
        for offset in (0..size).step_by(PAGE as usize) {
            bus.carregar_em_fisico(GuestPhys(base + offset), &[0; PAGE as usize])
                .map_err(crate::loader::Error::from)?;
        }
        *bus.frames_mut() = frames;
        *bus.mmu_mut() = mmu;
        let at = self.allocations.partition_point(|a| a.physical.0 < base);
        self.allocations.insert(at, allocation);
        Ok(())
    }
    /// Consulta sem A/D: posse física, tradução linear e ausência de aliases externos.
    pub(super) fn validate_bus(&self, bus: &Bus) -> Result<(), &'static str> {
        bus.boot_dma_page().validate(bus)?;
        let boot_reserved = bus.boot_dma_page().reserved();
        let mut owned = BTreeSet::new();
        for allocation in &self.allocations {
            if !allocation.valid()
                || (boot_reserved && allocation.physical.0 == 0)
                || u64::from(allocation.physical.0) + u64::from(allocation.size) > bus.ram() as u64
            {
                return Err("geometria de alocação contígua fora da RAM");
            }
            for offset in (0..allocation.size).step_by(PAGE as usize) {
                let pa = GuestPhys(allocation.physical.0 + offset);
                let va = GuestVirt(CONTIGUOUS_WINDOW + pa.0);
                if !bus.frames().em_uso(pa.frame())
                    || !owned.insert(pa.frame())
                    || bus.mmu().consultar(va) != Some((pa, Perm::RW))
                {
                    return Err("recurso contíguo diverge de posse, tradução ou permissões");
                }
            }
        }
        for (va, pa, _) in bus.mmu().mapeamentos() {
            if (pa.0 as usize) < bus.ram() && !bus.frames().em_uso(pa.frame()) {
                return Err("RAM mapeada sem frame reservado");
            }
            if owned.contains(&pa.frame()) && va.0 != CONTIGUOUS_WINDOW + pa.0 {
                return Err("recurso contíguo possui alias externo");
            }
            if (pa.0 as usize) < bus.ram()
                && (CONTIGUOUS_WINDOW..CONTIGUOUS_WINDOW + MAX_RAM).contains(&va.0)
                && !(boot_reserved && pa.0 == 0 && va == xbox_mem::BOOT_DMA_ADDRESS)
                && (!owned.contains(&pa.frame()) || va.0 != CONTIGUOUS_WINDOW + pa.0)
            {
                return Err("janela de RAM contígua sem recurso registrado");
            }
        }
        Ok(())
    }
}
fn candidate(bus: &Bus, low: u32, end: u32, size: u32, alignment: u32) -> Option<u32> {
    let mut base = align(low, alignment)?;
    loop {
        let limit = base.checked_add(size).filter(|&limit| limit <= end)?;
        let occupied = (base..limit).step_by(PAGE as usize).find(|&pa| {
            bus.frames().em_uso(pa / PAGE)
                || bus
                    .mmu()
                    .consultar(GuestVirt(CONTIGUOUS_WINDOW + pa))
                    .is_some()
        });
        if let Some(pa) = occupied {
            base = align(pa.checked_add(PAGE)?, alignment)?;
        } else {
            return Some(base);
        }
    }
}
fn align(value: u32, alignment: u32) -> Option<u32> {
    value
        .checked_add(alignment - 1)
        .map(|v| v & !(alignment - 1))
}
fn protection(protect: u32) -> bool {
    protect == 4 || protect == 4 | PAGE_WRITECOMBINE
}
fn bad(what: &'static str) -> SnapshotError {
    SnapshotError::BadValue {
        section: "CMEM",
        what,
    }
}
impl Snapshot for ContiguousMemory {
    fn save(&self, out: &mut SnapshotWriter) {
        out.section("CMEM", 1);
        out.u32(self.allocations.len() as u32);
        for a in &self.allocations {
            out.u32(a.physical.0);
            out.u32(a.size);
            out.u32(a.alignment);
            out.u32(a.protect);
        }
    }
    fn load(&mut self, inp: &mut SnapshotReader<'_>) -> Result<(), SnapshotError> {
        if inp.section("CMEM", 1)? != 1 {
            return Err(bad("versão desconhecida"));
        }
        let count = inp.u32()? as usize;
        if count > MAX_CONTIGUOUS_ALLOCATIONS {
            return Err(bad("limite de recursos contíguos excedido"));
        }
        let mut allocations = Vec::with_capacity(count);
        let mut previous = 0;
        for _ in 0..count {
            let a = ContiguousAllocation {
                physical: GuestPhys(inp.u32()?),
                size: inp.u32()?,
                alignment: inp.u32()?,
                protect: inp.u32()?,
            };
            if !a.valid() || a.physical.0 < previous {
                return Err(bad("geometria, ordem ou sobreposição de recursos inválida"));
            }
            previous = a.physical.0 + a.size;
            allocations.push(a);
        }
        *self = Self { allocations };
        Ok(())
    }
}
