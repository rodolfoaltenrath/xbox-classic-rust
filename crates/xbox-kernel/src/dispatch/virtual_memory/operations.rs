//! Consulta e liberação integral do perfil privado (ADR-0095).
use super::{PAGE, PAGE_READWRITE, VirtualAllocation, VirtualMemory, physical_word, variant};
use crate::dispatch::{
    Atendimento, Erro, Irql, Resultado, STATUS_ACCESS_VIOLATION, STATUS_INVALID_PARAMETER,
    STATUS_SUCCESS,
};
use crate::imports::Call;
use xbox_mem::{Acesso, Bus, GuestVirt};

const MEM_RELEASE: u32 = 0x8000;
const MEM_COMMIT: u32 = 0x1000;
const MEM_PRIVATE: u32 = 0x20000;

impl VirtualMemory {
    pub(crate) fn free(
        &mut self,
        bus: &mut Bus,
        call: &Call,
        irql: Irql,
        thread_region: Option<VirtualAllocation>,
        stack: u32,
    ) -> Resultado {
        let base_out = call.argument(bus, 0)?;
        let size_out = call.argument(bus, 1)?;
        let kind = call.argument(bus, 2)?;
        if irql != Irql::Passive {
            return Err(variant(bus, call, "liberação virtual exige Passive"));
        }
        for pointer in [base_out, size_out] {
            if !ram_range(bus, pointer, 4, Acesso::Leitura)
                || !ram_range(bus, pointer, 4, Acesso::Escrita)
            {
                return Ok(status(STATUS_ACCESS_VIOLATION));
            }
        }
        let base_bytes = physical_word(bus, base_out);
        let size_bytes = physical_word(bus, size_out);
        if base_bytes.iter().any(|pa| size_bytes.contains(pa)) {
            return Ok(status(STATUS_INVALID_PARAMETER));
        }
        let base = bus.ler_u32(GuestVirt(base_out))?;
        let size = bus.ler_u32(GuestVirt(size_out))?;
        if kind != MEM_RELEASE || size != 0 {
            return Err(variant(
                bus,
                call,
                "perfil libera somente MEM_RELEASE com RegionSize zero",
            ));
        }
        self.validate_bus(bus).map_err(Erro::MemoriaVirtual)?;
        let Some(index) = self
            .allocations
            .iter()
            .position(|region| region.base == base)
        else {
            return Err(variant(
                bus,
                call,
                "liberação exige base exata de região privada conhecida",
            ));
        };
        let region = self.allocations[index];
        let range = region.base..region.base + region.size;
        if thread_region == Some(region)
            || (u64::from(stack) < u64::from(range.end)
                && u64::from(stack) + 16 > u64::from(range.start))
            || [base_out, size_out]
                .into_iter()
                .any(|pointer| (0..4).any(|offset| range.contains(&(pointer + offset))))
        {
            return Err(variant(
                bus,
                call,
                "região contém recurso de thread, pilha ativa ou saída da própria liberação",
            ));
        }
        // validate_bus já conferiu posse integral e ausência de aliases externos.
        // Planejar em cópias: não há falta de página depois do primeiro unmap.
        let mut mmu = bus.mmu().clone();
        let mut frames = bus.frames().clone();
        for va in range.step_by(PAGE as usize) {
            let pa = bus
                .mmu()
                .consultar(GuestVirt(va))
                .expect("região validada")
                .0;
            mmu.desmapear(GuestVirt(va), 1);
            frames.liberar(pa, 1);
        }
        *bus.mmu_mut() = mmu;
        *bus.frames_mut() = frames;
        self.allocations.remove(index);
        bus.escrever_u32(GuestVirt(base_out), base)?;
        bus.escrever_u32(GuestVirt(size_out), region.size)?;
        Ok(status(STATUS_SUCCESS))
    }

    pub(crate) fn query(&self, bus: &mut Bus, call: &Call, irql: Irql) -> Resultado {
        let address = call.argument(bus, 0)?;
        let output = call.argument(bus, 1)?;
        if irql != Irql::Passive {
            return Err(variant(bus, call, "consulta virtual exige Passive"));
        }
        if !ram_range(bus, output, 28, Acesso::Escrita) {
            return Ok(status(STATUS_ACCESS_VIOLATION));
        }
        self.validate_bus(bus).map_err(Erro::MemoriaVirtual)?;
        let Some(region) = self
            .allocations
            .iter()
            .find(|region| (region.base..region.base + region.size).contains(&address))
        else {
            return Err(variant(
                bus,
                call,
                "consulta somente de região privada committed conhecida; imagem, MMIO e intervalos livres fora do perfil",
            ));
        };
        let page = address & !(PAGE - 1);
        let words = [
            page,
            region.base,
            PAGE_READWRITE,
            region.base + region.size - page,
            MEM_COMMIT,
            PAGE_READWRITE,
            MEM_PRIVATE,
        ];
        let mut bytes = [0; 28];
        for (chunk, word) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(words) {
            chunk.copy_from_slice(&word.to_le_bytes());
        }
        // ABI Xbox: só dois argumentos; sete ULONGs, sem layout/padding de host.
        bus.escrever_bytes(GuestVirt(output), &bytes)?;
        Ok(status(STATUS_SUCCESS))
    }
}
fn status(eax: u32) -> Atendimento {
    Atendimento { eax }
}

// Preflight sem tradução com efeitos A/D: falha ao fim de um buffer não altera
// nem páginas anteriores nem metadados de uma janela MMIO que seria recusada.
fn ram_range(bus: &Bus, address: u32, size: u32, access: Acesso) -> bool {
    address != 0
        && (0..size).all(|offset| {
            address
                .checked_add(offset)
                .and_then(|va| bus.mmu().consultar(GuestVirt(va)))
                .is_some_and(|(pa, perm)| perm.permite(access) && (pa.0 as usize) < bus.ram())
        })
}
