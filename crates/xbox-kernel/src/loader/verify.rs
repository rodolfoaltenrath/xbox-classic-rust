//! T14: arquivo → seções → frames → MMU → bus, incluindo alias e imports.

use super::{CoreError, Image, Mode, Result};
use xbox_formats::Xbe;
use xbox_mem::{Acesso, GuestVirt};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub sections: usize,
    pub pages: usize,
    pub checked_bytes: u64,
    pub entry: u32,
    pub entry_word: u32,
    pub imports: Vec<u32>,
    pub unmapped_probe: u32,
}

impl Image {
    /// Compara bytes/BSS, alias por PA, entry, imports e uma página ausente.
    pub fn verify(&self, bytes: &[u8]) -> Result<Report> {
        if self.mode != Mode::Inspect {
            return Err(CoreError::format("verificação T14 exige modo de inspeção").into());
        }
        let xbe = Xbe::parse(bytes)?;
        let headers = bytes
            .get(..xbe.header.size_of_headers as usize)
            .ok_or_else(|| CoreError::format("headers truncados"))?;
        self.compare(xbe.header.base_address, headers)?;
        let mut checked_bytes = headers.len() as u64;
        for (index, section) in xbe.sections.iter().enumerate() {
            self.compare(section.virtual_address, xbe.section_data(index)?)?;
            let mut offset = section.raw_size;
            let zeros = [0; 4096];
            while offset < section.virtual_size {
                let count = (section.virtual_size - offset).min(4096) as usize;
                self.compare(section.virtual_address + offset, &zeros[..count])?;
                offset += count as u32;
            }
            checked_bytes += u64::from(section.virtual_size);
        }
        let entry_word = self.bus.ler_u32(GuestVirt(xbe.entry_point))?;
        let mut expected = [0; 4];
        for (index, byte) in expected.iter_mut().enumerate() {
            let va = xbe
                .entry_point
                .checked_add(index as u32)
                .ok_or_else(|| CoreError::format("entry transborda"))?;
            let offset = xbe
                .va_to_offset(va)
                .ok_or_else(|| CoreError::format("entry sem bytes no arquivo"))?;
            *byte = *bytes
                .get(offset)
                .ok_or_else(|| CoreError::format("entry truncado"))?;
        }
        if entry_word != u32::from_le_bytes(expected) {
            return Err(CoreError::format("entry diverge do arquivo").into());
        }
        self.bus
            .traduzir(GuestVirt(xbe.entry_point), Acesso::Execucao)?;
        let imports = self.imports_from_memory()?;
        if imports != xbe.kernel_imports()? {
            return Err(CoreError::format("ordinais no bus divergem do arquivo").into());
        }
        let unmapped_probe = (0..=u32::MAX / 4096)
            .map(|page| page * 4096)
            .find(|&va| self.bus.mmu().consultar(GuestVirt(va)).is_none())
            .ok_or_else(|| CoreError::format("não há página ausente para verificar"))?;
        if self.bus.ler_u8(GuestVirt(unmapped_probe)).is_ok() {
            return Err(CoreError::format("página ausente retornou dados").into());
        }
        Ok(Report {
            sections: xbe.sections.len(),
            pages: self.pages,
            checked_bytes,
            entry: xbe.entry_point,
            entry_word,
            imports,
            unmapped_probe,
        })
    }

    pub fn imports_from_memory(&self) -> Result<Vec<u32>> {
        if self.kernel_thunk == 0 {
            return Ok(Vec::new());
        }
        let mut imports = Vec::new();
        for index in 0..512u32 {
            let va = self
                .kernel_thunk
                .checked_add(index * 4)
                .ok_or_else(|| CoreError::format("thunk transborda"))?;
            let raw = self.bus.ler_u32(GuestVirt(va))?;
            if raw == 0 {
                return Ok(imports);
            }
            if raw & 0x80000000 == 0 {
                return Err(CoreError::format("thunk no bus sem bit de ordinal").into());
            }
            imports.push(raw & 0xFFFF);
        }
        Err(CoreError::format("thunk no bus sem terminador em 512 entradas").into())
    }

    fn compare(&self, mut va: u32, mut expected: &[u8]) -> Result<()> {
        while !expected.is_empty() {
            let size = expected.len().min(4096 - (va & 4095) as usize);
            let mut direct = [0; 4096];
            let mut alias = [0; 4096];
            self.bus.ler_bytes(GuestVirt(va), &mut direct[..size])?;
            let physical = self.bus.traduzir(GuestVirt(va), Acesso::Leitura)?;
            self.bus
                .ler_bytes(GuestVirt(0x80000000 + physical.0), &mut alias[..size])?;
            if direct[..size] != expected[..size] || alias[..size] != expected[..size] {
                return Err(CoreError::format(format!(
                    "bytes do bus/alias divergem em 0x{va:08X}"
                ))
                .into());
            }
            expected = &expected[size..];
            if !expected.is_empty() {
                va = va
                    .checked_add(size as u32)
                    .ok_or_else(|| CoreError::format("comparação transborda"))?;
            }
        }
        Ok(())
    }
}
