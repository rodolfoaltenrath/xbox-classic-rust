//! Reserva de boot da página D3D, com posse explícita (ADR-0061).
use crate::{Bus, GuestPhys, GuestVirt, Perm};
use xbox_core::{Error, Result, SnapshotReader, SnapshotWriter, error::SnapshotError};

pub const BOOT_DMA_ADDRESS: GuestVirt = GuestVirt(0x8000_0000);
const PHYSICAL: GuestPhys = GuestPhys(0);

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct BootDmaPage {
    reserved: bool,
}
impl BootDmaPage {
    pub const fn reserved(self) -> bool {
        self.reserved
    }
    pub(crate) fn install(bus: &mut Bus) -> Result<Self> {
        if bus.ram() < 4096
            || bus.frames().em_uso(0)
            || bus.mmu().consultar(BOOT_DMA_ADDRESS).is_some()
            || bus.mmu().mapeamentos().any(|(_, pa, _)| pa == PHYSICAL)
        {
            return Err(Error::format(
                "página de boot DMA indisponível; instalar antes da imagem",
            ));
        }
        let mut frames = bus.frames().clone();
        let mut mmu = bus.mmu().clone();
        frames.reservar(PHYSICAL, 1)?;
        mmu.mapear(BOOT_DMA_ADDRESS, PHYSICAL, 1, Perm::RW)
            .map_err(|e| Error::format(e.to_string()))?;
        bus.carregar_em_fisico(PHYSICAL, &[0; 4096])?;
        *bus.frames_mut() = frames;
        *bus.mmu_mut() = mmu;
        Ok(Self { reserved: true })
    }
    /// Sem bits A/D: a reserva tem um único alias RW, sem execução.
    pub fn validate(self, bus: &Bus) -> std::result::Result<(), &'static str> {
        if self.reserved
            && (bus.ram() < 4096
                || !bus.frames().em_uso(0)
                || bus.mmu().consultar(BOOT_DMA_ADDRESS) != Some((PHYSICAL, Perm::RW))
                || bus
                    .mmu()
                    .mapeamentos()
                    .any(|(va, pa, _)| pa == PHYSICAL && va != BOOT_DMA_ADDRESS))
        {
            return Err("reserva de boot DMA diverge de frame, mapeamento, permissões ou aliases");
        }
        Ok(())
    }
    pub(crate) fn save(self, out: &mut SnapshotWriter) {
        out.section("BTPG", 1);
        out.bool(self.reserved);
    }
    pub(crate) fn load(
        &mut self,
        inp: &mut SnapshotReader<'_>,
    ) -> std::result::Result<(), SnapshotError> {
        let bad = |what| SnapshotError::BadValue {
            section: "BTPG",
            what,
        };
        if inp.section("BTPG", 1)? != 1 {
            return Err(bad("versão desconhecida"));
        }
        let reserved = match inp.u8()? {
            0 => false,
            1 => true,
            _ => return Err(bad("marcador de reserva inválido")),
        };
        self.reserved = reserved;
        Ok(())
    }
}
