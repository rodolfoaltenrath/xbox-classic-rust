//! Reserva+commit privado em RAM guest, perfil AOT v1 (ADR-0043).
mod operations;

use super::{
    Atendimento, Erro, Irql, Resultado, STATUS_ACCESS_VIOLATION, STATUS_INSUFFICIENT_RESOURCES,
    STATUS_SUCCESS, descrever_argumentos, ram_range,
};
use crate::imports::Call;
use std::collections::BTreeSet;
use xbox_core::error::SnapshotError;
use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_mem::{Acesso, Bus, GuestPhys, GuestVirt, Perm};

pub const MEM_COMMIT: u32 = 0x1000;
pub const MEM_RESERVE: u32 = 0x2000;
pub const PAGE_READWRITE: u32 = 4;
pub const STATUS_INVALID_PARAMETER: u32 = 0xC000_000D;
pub const STATUS_NO_MEMORY: u32 = 0xC000_0017;
pub const MAX_VIRTUAL_ALLOCATIONS: usize = 4096;
const PAGE: u32 = 4096;
const GRANULARITY: u32 = 65536;
// Política de endereço do perfil: abaixo da pilha/TIB/imports e das janelas físicas.
const END: u32 = crate::loader::STACK_BASE;
const MAX_SNAPSHOT_PAGES: u32 = 128 * 1024 * 1024 / PAGE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageRange {
    pub base: u32,
    pub size: u32,
}

impl ImageRange {
    fn valid(self) -> bool {
        self.size >= 0x110 && self.base.checked_add(self.size).is_some()
    }
}

/// Região privada integralmente committed, sempre `PAGE_READWRITE` neste perfil.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VirtualAllocation {
    pub base: u32,
    pub size: u32,
}

#[derive(Debug, Default)]
pub struct VirtualMemory {
    image: Option<ImageRange>,
    allocations: Vec<VirtualAllocation>,
    // Metadado transitório da leitura KRNL v1–v5, consumido pela restauração de
    // Session; não muda a execução guest nem é uma segunda fonte de alocações.
    legacy_context: bool,
}

impl VirtualMemory {
    pub(super) fn legacy() -> Self {
        Self {
            legacy_context: true,
            ..Self::default()
        }
    }

    pub(super) fn for_image(base: u32, size: u32) -> xbox_core::Result<Self> {
        let image = ImageRange { base, size };
        if !image.valid() {
            return Err(xbox_core::Error::format(
                "intervalo de imagem inválido para memória virtual",
            ));
        }
        Ok(Self {
            image: Some(image),
            ..Self::default()
        })
    }

    pub const fn image(&self) -> Option<ImageRange> {
        self.image
    }
    pub fn allocations(&self) -> &[VirtualAllocation] {
        &self.allocations
    }

    pub(super) fn restore_context(
        &mut self,
        original: Option<ImageRange>,
        bus: &Bus,
    ) -> Result<(), SnapshotError> {
        if self.legacy_context {
            self.image = original;
            self.legacy_context = false;
        }
        self.validate_bus(bus).map_err(bad)
    }

    /// RAM/MMU/bitmap continuam no Bus. O registro lógico não duplica frames.
    fn validate_bus(&self, bus: &Bus) -> Result<(), &'static str> {
        if let Some(image) = self.image
            && (!image.valid()
                || peek_word(bus, image.base) != Some(u32::from_le_bytes(*b"XBEH"))
                || peek_word(bus, image.base + 0x104) != Some(image.base)
                || peek_word(bus, image.base + 0x10C) != Some(image.size))
        {
            return Err("intervalo de imagem não corresponde aos headers XBE na RAM");
        }
        let mut frames = BTreeSet::new();
        let mut addresses = BTreeSet::new();
        for region in &self.allocations {
            if region.size / PAGE > bus.frames().total() {
                return Err("região maior que a RAM do Bus");
            }
            for va in (region.base..region.base + region.size).step_by(PAGE as usize) {
                addresses.insert(va);
                let Some((pa, perm)) = bus.mmu().consultar(GuestVirt(va)) else {
                    return Err("região committed sem mapeamento");
                };
                if perm != Perm::RW
                    || pa.0 as usize >= bus.ram()
                    || !bus.frames().em_uso(pa.frame())
                    || !frames.insert(pa.frame())
                {
                    return Err("região committed diverge de permissões, RAM ou posse dos frames");
                }
            }
        }
        if self.image.is_some() {
            for (va, pa, _) in bus.mmu().mapeamentos() {
                if (pa.0 as usize) < bus.ram() && !bus.frames().em_uso(pa.frame()) {
                    return Err("página RAM mapeada sem posse no alocador de frames");
                }
                if frames.contains(&pa.frame()) && !addresses.contains(&va.0) {
                    return Err("região privada possui alias físico externo");
                }
            }
        }
        Ok(())
    }

    pub(super) fn allocate(&mut self, bus: &mut Bus, call: &Call, irql: Irql) -> Resultado {
        let args = [
            call.argument(bus, 0)?,
            call.argument(bus, 1)?,
            call.argument(bus, 2)?,
            call.argument(bus, 3)?,
            call.argument(bus, 4)?,
        ];
        let [base_out, zero_bits, size_out, flags, protect] = args;
        if irql != Irql::Passive {
            return Err(variant(
                bus,
                call,
                "alocação fora de PASSIVE_LEVEL ainda sem modelo",
            ));
        }
        for pointer in [base_out, size_out] {
            if !ram_range(bus, pointer, 4, Acesso::Leitura)
                || !ram_range(bus, pointer, 4, Acesso::Escrita)
            {
                return Ok(Atendimento {
                    eax: STATUS_ACCESS_VIOLATION,
                });
            }
        }
        // Inclui aliases físicos e sobreposição parcial entre as duas saídas.
        let base_bytes = physical_word(bus, base_out);
        let size_bytes = physical_word(bus, size_out);
        if base_bytes.iter().any(|pa| size_bytes.contains(pa)) {
            return Ok(Atendimento {
                eax: STATUS_INVALID_PARAMETER,
            });
        }
        let requested_base = bus.ler_u32(GuestVirt(base_out))?;
        let requested_size = bus.ler_u32(GuestVirt(size_out))?;
        let Some(size) = align(requested_size, PAGE).filter(|size| *size != 0) else {
            return Ok(Atendimento {
                eax: STATUS_INVALID_PARAMETER,
            });
        };
        if self.image.is_none() {
            return Err(variant(
                bus,
                call,
                "alocador exige o intervalo da imagem XBE da sessão",
            ));
        }
        if requested_base != 0 {
            return Err(variant(
                bus,
                call,
                "alocação em endereço solicitado ainda sem modelo",
            ));
        }
        if zero_bits != 0 {
            return Err(variant(bus, call, "restrição ZeroBits ainda sem modelo"));
        }
        if flags != MEM_RESERVE | MEM_COMMIT {
            return Err(variant(
                bus,
                call,
                "somente MEM_RESERVE | MEM_COMMIT está modelado",
            ));
        }
        if protect != PAGE_READWRITE {
            return Err(variant(
                bus,
                call,
                "somente PAGE_READWRITE está modelado no perfil AOT",
            ));
        }
        self.validate_bus(bus).map_err(Erro::MemoriaVirtual)?;
        if self.allocations.len() >= MAX_VIRTUAL_ALLOCATIONS {
            return Ok(Atendimento {
                eax: STATUS_INSUFFICIENT_RESOURCES,
            });
        }
        let pages = size / PAGE;
        if pages > bus.frames().livres() {
            return Ok(Atendimento {
                eax: STATUS_NO_MEMORY,
            });
        }
        let image = self.image.expect("intervalo conferido acima");
        let Some(base) = find_base(bus, image, size) else {
            return Ok(Atendimento {
                eax: STATUS_NO_MEMORY,
            });
        };
        self.publish(bus, VirtualAllocation { base, size }, base_out, size_out)
    }

    fn publish(
        &mut self,
        bus: &mut Bus,
        region: VirtualAllocation,
        base_out: u32,
        size_out: u32,
    ) -> Resultado {
        let VirtualAllocation { base, size } = region;
        let pages = size / PAGE;
        // Planejar em cópias da MMU/bitmap. Falha não publica páginas/frames e
        // não exige copiar a arena inteira ou desfazer mapeamentos já visíveis.
        let mut mmu = bus.mmu().clone();
        let mut allocator = bus.frames().clone();
        let mut frames = Vec::with_capacity(pages as usize);
        for index in 0..pages {
            let Some(pa) = allocator.alocar() else {
                return Ok(Atendimento {
                    eax: STATUS_NO_MEMORY,
                });
            };
            mmu.mapear(GuestVirt(base + index * PAGE), pa, 1, Perm::RW)?;
            frames.push(pa);
        }
        // Todos os frames são RAM completa e as saídas já têm leitura/escrita
        // validadas. Daqui em diante não há falha guest ou dispositivo a executar.
        for pa in frames {
            bus.carregar_em_fisico(pa, &[0; PAGE as usize])
                .map_err(crate::loader::Error::from)?;
        }
        *bus.mmu_mut() = mmu;
        *bus.frames_mut() = allocator;
        if base_out != 0 {
            bus.escrever_u32(GuestVirt(base_out), base)?;
            bus.escrever_u32(GuestVirt(size_out), size)?;
        }
        let at = self
            .allocations
            .partition_point(|region| region.base < base);
        self.allocations
            .insert(at, VirtualAllocation { base, size });
        Ok(Atendimento {
            eax: STATUS_SUCCESS,
        })
    }
}

impl VirtualMemory {
    /// Recurso privado do kernel, planejado como as demais regiões committed.
    pub(super) fn allocate_resource(
        &mut self,
        bus: &mut Bus,
        size: u32,
    ) -> Result<Option<VirtualAllocation>, Erro> {
        self.validate_bus(bus).map_err(Erro::MemoriaVirtual)?;
        let Some(image) = self.image else {
            return Err(Erro::MemoriaVirtual("imagem ausente"));
        };
        if size == 0
            || !size.is_multiple_of(PAGE)
            || size / PAGE > bus.frames().livres()
            || self.allocations.len() >= MAX_VIRTUAL_ALLOCATIONS
        {
            return Ok(None);
        }
        let Some(base) = size
            .checked_add(PAGE)
            .and_then(|guarded| find_base(bus, image, guarded))
        else {
            return Ok(None);
        };
        let region = VirtualAllocation { base, size };
        let result = self.publish(bus, region, 0, 0)?;
        Ok((result.eax == STATUS_SUCCESS).then_some(region))
    }
}

// Inspeção do snapshot não marca Accessed/Dirty nem acessa MMIO.
pub(super) fn peek_word(bus: &Bus, address: u32) -> Option<u32> {
    let mut bytes = [0; 4];
    for (offset, byte) in bytes.iter_mut().enumerate() {
        let (pa, perm) = bus
            .mmu()
            .consultar(GuestVirt(address.checked_add(offset as u32)?))?;
        if !perm.permite(Acesso::Leitura) {
            return None;
        }
        *byte = *bus.dma(pa, 1).ok()?.first()?;
    }
    Some(u32::from_le_bytes(bytes))
}
fn find_base(bus: &Bus, image: ImageRange, size: u32) -> Option<u32> {
    let mut base = align(image.base + image.size, GRANULARITY)?;
    loop {
        let end = base.checked_add(size).filter(|end| *end <= END)?;
        let occupied = (base..end)
            .step_by(PAGE as usize)
            .find(|va| bus.mmu().consultar(GuestVirt(*va)).is_some());
        let Some(occupied) = occupied else {
            return Some(base);
        };
        base = align(occupied + PAGE, GRANULARITY)?;
    }
}
pub(super) fn physical_word(bus: &Bus, pointer: u32) -> [GuestPhys; 4] {
    std::array::from_fn(|i| {
        bus.mmu()
            .consultar(GuestVirt(pointer + i as u32))
            .expect("RAM validada")
            .0
    })
}
fn align(value: u32, alignment: u32) -> Option<u32> {
    value
        .checked_add(alignment - 1)
        .map(|v| v & !(alignment - 1))
}
fn variant(bus: &Bus, call: &Call, detalhe: &'static str) -> Erro {
    Erro::VarianteNaoImplementada {
        nome: call.export.name,
        ordinal: call.export.ordinal,
        chamador: call.return_pc,
        detalhe,
        argumentos: descrever_argumentos(bus, call),
    }
}
fn bad(what: &'static str) -> SnapshotError {
    SnapshotError::BadValue {
        section: "VMEM",
        what,
    }
}

impl Snapshot for VirtualMemory {
    fn save(&self, out: &mut SnapshotWriter) {
        out.section("VMEM", 1);
        out.bool(self.image.is_some());
        if let Some(image) = self.image {
            out.u32(image.base);
            out.u32(image.size);
        }
        out.u32(self.allocations.len() as u32);
        for region in &self.allocations {
            out.u32(region.base);
            out.u32(region.size);
        }
    }
    fn load(&mut self, inp: &mut SnapshotReader<'_>) -> Result<(), SnapshotError> {
        if inp.section("VMEM", 1)? != 1 {
            return Err(bad("versão de memória virtual desconhecida"));
        }
        let image = if inp.bool()? {
            let image = ImageRange {
                base: inp.u32()?,
                size: inp.u32()?,
            };
            if !image.valid() {
                return Err(bad("intervalo de imagem inválido"));
            }
            Some(image)
        } else {
            None
        };
        let count = inp.u32()? as usize;
        if count > MAX_VIRTUAL_ALLOCATIONS || (image.is_none() && count != 0) {
            return Err(bad("contagem de regiões inválida ou sem imagem"));
        }
        let mut allocations = Vec::with_capacity(count);
        let mut previous = image.and_then(|image| align(image.base + image.size, GRANULARITY));
        let mut pages = 0;
        for _ in 0..count {
            let region = VirtualAllocation {
                base: inp.u32()?,
                size: inp.u32()?,
            };
            let end = region
                .base
                .checked_add(region.size)
                .filter(|end| *end <= END);
            if !region.base.is_multiple_of(GRANULARITY)
                || region.size == 0
                || !region.size.is_multiple_of(PAGE)
                || end.is_none()
                || previous.is_none_or(|p| region.base < p)
            {
                return Err(bad("regiões desalinhadas, sobrepostas ou fora do perfil"));
            }
            pages += region.size / PAGE;
            if pages > MAX_SNAPSHOT_PAGES {
                return Err(bad("regiões excedem a RAM máxima do perfil"));
            }
            previous = end;
            allocations.push(region);
        }
        *self = Self {
            image,
            allocations,
            legacy_context: false,
        };
        Ok(())
    }
}
