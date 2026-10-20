//! Imagem XBE em memória, sem executar código nem resolver importações.

use std::collections::BTreeMap;
use xbox_core::{Error as CoreError, PAGE_SIZE};
use xbox_formats::{Xbe, xbe::SectionFlags};
use xbox_mem::{Acesso, Bus, GuestPhys, GuestVirt, Perm};

mod verify;
pub use verify::Report;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Format(#[from] CoreError),
    #[error(transparent)]
    Memory(#[from] xbox_mem::Falta),
}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Mapeia a imagem e o alias físico contíguo para validação, nunca execução.
    Inspect,
    /// Recusa código gravável e não oferece alias que permita modificá-lo.
    Aot,
}

#[derive(Debug)]
pub struct Image {
    pub bus: Bus,
    pub entry: u32,
    pub kernel_thunk: u32,
    pub pages: usize,
    mode: Mode,
}

impl Image {
    pub fn load(bytes: &[u8], mode: Mode) -> Result<Self> {
        let xbe = Xbe::parse(bytes)?;
        validate_layout(&xbe)?;
        let mut bus = Bus::novo()?;
        let pages = page_plan(&xbe, mode, bus.ram())?;
        for &va in pages.keys() {
            let frame = bus
                .frames_mut()
                .alocar()
                .ok_or_else(|| CoreError::format("RAM insuficiente para o XBE"))?;
            bus.mapear(GuestVirt(va), frame, 1, Perm::RW)?;
        }
        let headers = bytes
            .get(..xbe.header.size_of_headers as usize)
            .ok_or_else(|| CoreError::format("headers XBE truncados"))?;
        bus.escrever_bytes(GuestVirt(xbe.header.base_address), headers)?;
        for (index, section) in xbe.sections.iter().enumerate() {
            bus.escrever_bytes(GuestVirt(section.virtual_address), xbe.section_data(index)?)?;
        }
        for (&va, &perm) in &pages {
            let frame = bus.traduzir(GuestVirt(va), Acesso::Leitura)?;
            bus.mapear(GuestVirt(va), frame, 1, perm)?;
        }
        if mode == Mode::Inspect {
            // Alias explícito VA=0x80000000+PA. Não somar VA de seção à base:
            // os frames são alocados e não têm identidade VA=PA.
            for offset in (0..bus.ram()).step_by(xbox_core::LARGE_PAGE_SIZE) {
                bus.mmu_mut().mapear_grande(
                    GuestVirt(0x80000000 + offset as u32),
                    GuestPhys(offset as u32),
                    Perm::RW,
                )?;
            }
        }
        Ok(Self {
            bus,
            entry: xbe.entry_point,
            kernel_thunk: xbe.kernel_thunk,
            pages: pages.len(),
            mode,
        })
    }
}

/// Validação comum ao decoder AOT e ao loader. Não reserva a pilha de teste AOT.
pub fn validate_layout(xbe: &Xbe<'_>) -> xbox_core::Result<()> {
    let base = xbe.header.base_address;
    let end = base
        .checked_add(xbe.header.size_of_image)
        .ok_or_else(|| CoreError::format("imagem transborda o espaço x86"))?;
    let headers_end = base
        .checked_add(xbe.header.size_of_headers)
        .filter(|&v| v <= end)
        .ok_or_else(|| CoreError::format("headers fora da imagem"))?;
    let mut ranges = vec![(base, headers_end)];
    for section in &xbe.sections {
        if section.raw_size > section.virtual_size {
            return Err(CoreError::format(
                "raw_size maior que virtual_size não suportado",
            ));
        }
        if section.virtual_size == 0 {
            continue;
        }
        let finish = section
            .virtual_address
            .checked_add(section.virtual_size)
            .filter(|&v| v <= end && section.virtual_address >= base)
            .ok_or_else(|| CoreError::format("seção fora da imagem declarada"))?;
        ranges.push((section.virtual_address, finish));
    }
    ranges.sort_unstable();
    if ranges.windows(2).any(|pair| pair[0].1 > pair[1].0) {
        return Err(CoreError::format("seções/headers sobrepostos"));
    }
    Ok(())
}

fn page_plan(xbe: &Xbe<'_>, mode: Mode, ram: usize) -> xbox_core::Result<BTreeMap<u32, Perm>> {
    let mut pages = BTreeMap::new();
    add_pages(
        &mut pages,
        xbe.header.base_address,
        xbe.header.size_of_headers,
        Perm::RO,
        ram,
    )?;
    for section in &xbe.sections {
        let mut perm = Perm::RO;
        if section.flags.tem(SectionFlags::EXECUTABLE) {
            perm.0 |= Perm::EXECUTAR;
        }
        if section.flags.tem(SectionFlags::WRITABLE) {
            perm.0 |= Perm::ESCREVER;
        }
        add_pages(
            &mut pages,
            section.virtual_address,
            section.virtual_size,
            perm,
            ram,
        )?;
    }
    if mode == Mode::Aot && pages.values().any(|p| p.0 & 6 == 6) {
        return Err(CoreError::format(
            "AOT não aceita página executável gravável ou compartilhada com dados graváveis",
        ));
    }
    if mode == Mode::Inspect
        && pages
            .keys()
            .any(|&va| va >= 0x80000000 && u64::from(va) < 0x80000000 + ram as u64)
    {
        return Err(CoreError::format(
            "imagem ocupa a janela contígua de inspeção",
        ));
    }
    Ok(pages)
}

fn add_pages(
    pages: &mut BTreeMap<u32, Perm>,
    start: u32,
    size: u32,
    perm: Perm,
    ram: usize,
) -> xbox_core::Result<()> {
    if size == 0 {
        return Ok(());
    }
    let end = start
        .checked_add(size - 1)
        .ok_or_else(|| CoreError::format("seção XBE transborda"))?;
    if u64::from(end / 4096 - start / 4096 + 1) > (ram / PAGE_SIZE) as u64 {
        return Err(CoreError::format("seção XBE maior que a RAM disponível"));
    }
    for page in start / 4096..=end / 4096 {
        pages
            .entry(page * 4096)
            .and_modify(|p| p.0 |= perm.0)
            .or_insert(perm);
        if pages.len() > ram / PAGE_SIZE {
            return Err(CoreError::format("imagem XBE maior que a RAM disponível"));
        }
    }
    Ok(())
}
