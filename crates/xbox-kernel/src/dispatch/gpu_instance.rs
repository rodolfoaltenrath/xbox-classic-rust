//! Consulta de instância GPU, bootstrap retail 64 MiB (ADR-0059/P13).
//! Não interpreta fisicamente PRAMIN ou permite redimensionamento.
use super::{
    Atendimento, ContiguousMemory, Erro, Irql, Resultado, descrever_argumentos, ram_range,
};
use crate::imports::Call;
use xbox_core::{Snapshot, SnapshotReader, SnapshotWriter, error::SnapshotError};
use xbox_mem::{Acesso, Bus, GuestVirt};

const RETAIL_RAM: usize = 64 << 20;
const POOL_BASE: u32 = 0x03FE_0000;
const POOL_SIZE: u32 = 0x0002_0000;
const PADDING: u32 = 0x0001_0000;
const BOUNDARY: u32 = 0x83FF_0000;

#[derive(Debug, Default)]
pub struct GpuInstanceMemory {
    provisioned: bool,
}
impl GpuInstanceMemory {
    pub const fn provisioned(&self) -> bool {
        self.provisioned
    }
    pub(super) fn query(
        &mut self,
        bus: &mut Bus,
        call: &Call,
        irql: Irql,
        contiguous: &mut ContiguousMemory,
    ) -> Resultado {
        let requested = call.argument(bus, 0)?;
        let output = call.argument(bus, 1)?;
        if requested != u32::MAX || irql != Irql::Passive || bus.ram() != RETAIL_RAM {
            return Err(Erro::VarianteNaoImplementada {
                nome: call.export.name,
                ordinal: call.export.ordinal,
                chamador: call.return_pc,
                detalhe: "perfil retail 64 MiB: somente consulta 0xFFFFFFFF em Passive; resize e devkit não modelados",
                argumentos: descrever_argumentos(bus, call),
            });
        }
        if !ram_range(bus, output, 4, Acesso::Escrita) {
            return Err(Erro::ArgumentoInvalido {
                nome: call.export.name,
                ordinal: call.export.ordinal,
                detalhe: "NumberOfPaddingBytes exige quatro bytes em RAM guest gravável".to_owned(),
            });
        }
        contiguous
            .validate_bus(bus)
            .map_err(Erro::MemoriaContigua)?;
        self.validate(contiguous, Some(bus))
            .map_err(|_| Erro::MemoriaContigua("perfil de instância diverge de CMEM/RAM"))?;
        if !self.provisioned {
            if !contiguous.reserve_instance(bus, POOL_BASE, POOL_SIZE)? {
                return Ok(Atendimento { eax: 0 });
            }
            self.provisioned = true;
        }
        bus.escrever_u32(GuestVirt(output), PADDING)?;
        Ok(Atendimento { eax: BOUNDARY })
    }
    pub(super) fn validate(
        &self,
        contiguous: &ContiguousMemory,
        bus: Option<&Bus>,
    ) -> Result<(), SnapshotError> {
        if self.provisioned
            && (bus.is_some_and(|bus| bus.ram() != RETAIL_RAM)
                || !contiguous.allocations().iter().any(|a| {
                    a.physical.0 == POOL_BASE
                        && a.size == POOL_SIZE
                        && a.alignment == 4096
                        && a.protect == 4
                }))
        {
            return Err(bad("perfil de instância sem sua reserva física exata"));
        }
        Ok(())
    }
}
fn bad(what: &'static str) -> SnapshotError {
    SnapshotError::BadValue {
        section: "GINS",
        what,
    }
}
impl Snapshot for GpuInstanceMemory {
    fn save(&self, out: &mut SnapshotWriter) {
        out.section("GINS", 1);
        out.bool(self.provisioned);
    }
    fn load(&mut self, inp: &mut SnapshotReader<'_>) -> Result<(), SnapshotError> {
        if inp.section("GINS", 1)? != 1 {
            return Err(bad("versão de instância GPU desconhecida"));
        }
        let provisioned = match inp.u8()? {
            0 => false,
            1 => true,
            _ => return Err(bad("marcador de provisionamento inválido")),
        };
        self.provisioned = provisioned;
        Ok(())
    }
}
