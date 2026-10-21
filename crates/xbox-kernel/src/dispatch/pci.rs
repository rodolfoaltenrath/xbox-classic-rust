//! Configuração PCI parcial: registro de clock DRAM 0:0:3, 0x6C (ADR-0039).
//! Nenhuma leitura/escrita de portas PCI host; áreas desconhecidas não são zeros.

use xbox_core::error::SnapshotError;
use xbox_core::hardware::RetailClockV1;
use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_mem::{Acesso, Bus, GuestVirt};

use super::{Atendimento, Erro, Resultado, descrever_argumentos, ram_range};
use crate::imports::Call;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum PciProfile {
    /// Somente os quatro bytes do PLL documentado, imutáveis nesta entrega.
    RetailClockV1 = 1,
}

/// Configuração autoritativa do perfil, sem tabela fictícia de dispositivos ausentes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PciConfig {
    profile: PciProfile,
}

impl Default for PciConfig {
    fn default() -> Self {
        Self {
            profile: PciProfile::RetailClockV1,
        }
    }
}

impl PciConfig {
    pub const fn profile(&self) -> PciProfile {
        self.profile
    }

    pub(super) fn transfer(&self, bus: &mut Bus, call: &Call) -> Resultado {
        let number = call.argument(bus, 0)?;
        let slot = call.argument(bus, 1)?;
        let register = call.argument(bus, 2)?;
        let buffer = call.argument(bus, 3)?;
        let length = call.argument(bus, 4)?;
        let write = call.argument(bus, 5)? as u8 != 0;

        // PCI convencional: 256 buses, 5 bits de device + 3 de function no slot
        // NT, 256 bytes por função. Não mascarar bits reservados ou dar wrap.
        if number > 255 || slot > 255 || register > 256 || length > 256 - register {
            return Err(invalid(
                call,
                "bus/slot ou intervalo fora do espaço PCI convencional",
            ));
        }
        // Slot NT: device em [4:0], function em [7:5]. 0x60 => 0:0:3.
        if number != 0 || slot != 0x60 {
            return Err(variant(
                bus,
                call,
                "função PCI ainda sem configuração guest modelada",
            ));
        }
        if write {
            return Err(variant(
                bus,
                call,
                "escrita PCI exige efeitos de dispositivo e clock ainda não modelados",
            ));
        }
        let end = register + length; // validado acima, sem overflow.
        if register < 0x6C || end > 0x70 {
            return Err(variant(
                bus,
                call,
                "intervalo PCI fora do registro PLL 0x6C..0x70 modelado",
            ));
        }
        if length == 0 {
            return Ok(Atendimento { eax: 0 }); // faixa vazia; não acessar Buffer.
        }
        if !ram_range(bus, buffer, length, Acesso::Escrita) {
            return Err(invalid(
                call,
                "Buffer deve cobrir toda a saída em RAM guest gravável",
            ));
        }
        let config = match self.profile {
            PciProfile::RetailClockV1 => RetailClockV1::FSB_PLL_CONFIG.to_le_bytes(),
        };
        let start = (register - 0x6C) as usize;
        let bytes = &config[start..start + length as usize];
        bus.escrever_bytes(GuestVirt(buffer), bytes)?;
        Ok(Atendimento { eax: 0 }) // VOID; não inventar NTSTATUS.
    }
}

impl Snapshot for PciConfig {
    fn save(&self, out: &mut SnapshotWriter) {
        out.section("PCIC", 1);
        out.u32(self.profile as u32);
    }

    fn load(&mut self, inp: &mut SnapshotReader<'_>) -> Result<(), SnapshotError> {
        let bad = |what| SnapshotError::BadValue {
            section: "PCIC",
            what,
        };
        if inp.section("PCIC", 1)? != 1 {
            return Err(bad("versão de configuração PCI desconhecida"));
        }
        let profile = match inp.u32()? {
            1 => PciProfile::RetailClockV1,
            _ => return Err(bad("perfil de configuração PCI desconhecido")),
        };
        *self = Self { profile };
        Ok(())
    }
}

fn invalid(call: &Call, detail: &str) -> Erro {
    Erro::ArgumentoInvalido {
        nome: call.export.name,
        ordinal: call.export.ordinal,
        detalhe: detail.to_owned(),
    }
}

fn variant(bus: &Bus, call: &Call, detail: &'static str) -> Erro {
    Erro::VarianteNaoImplementada {
        nome: call.export.name,
        ordinal: call.export.ordinal,
        chamador: call.return_pc,
        detalhe: detail,
        argumentos: descrever_argumentos(bus, call),
    }
}
