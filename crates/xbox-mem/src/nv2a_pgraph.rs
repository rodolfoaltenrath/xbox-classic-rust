//! Adaptador DMA exclusivo do backend gráfico para arena/PRAMIN únicas.
use crate::{Arena, ArenaOff, Nv2aPramin};
pub(crate) struct GpuMemory<'a> {
    pub arena: &'a mut Arena,
    pub pramin: &'a Nv2aPramin,
}
impl xbox_gpu::pgraph::Memory for GpuMemory<'_> {
    fn ram_len(&self) -> u64 {
        self.arena.len() as u64
    }
    fn read_instance(&self, offset: u32) -> Result<u32, &'static str> {
        self.pramin
            .read(
                0x700000u32
                    .checked_add(offset)
                    .ok_or("instância transborda")?,
            )
            .ok_or("instância fora da PRAMIN")
    }
    fn read_ram(&self, physical: u32) -> Result<u32, &'static str> {
        if physical & 3 != 0 {
            return Err("DMA gráfico desalinhado");
        }
        let bytes = self
            .arena
            .faixa(ArenaOff(physical), 4)
            .map_err(|_| "DMA gráfico fora da RAM")?;
        Ok(u32::from_le_bytes(bytes.try_into().expect("DWORD")))
    }
    fn write_ram(&mut self, physical: u32, value: u32) -> Result<(), &'static str> {
        if physical & 3 != 0 {
            return Err("DMA gráfico desalinhado");
        }
        self.arena
            .escrever(ArenaOff(physical), &value.to_le_bytes())
            .map_err(|_| "store gráfico fora da RAM")
    }
}
