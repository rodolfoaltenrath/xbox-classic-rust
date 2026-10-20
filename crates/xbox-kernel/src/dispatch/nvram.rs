//! Imagem de EEPROM da sessão e consulta integral do perfil HLE (ADR-0034).
use xbox_core::error::SnapshotError;
use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_formats::eeprom::{Eeprom, Idioma, PadraoDeVideo, TAMANHO};
use xbox_mem::{Acesso, Bus, GuestVirt};

use super::{Atendimento, Erro, Resultado, STATUS_ACCESS_VIOLATION, STATUS_SUCCESS, ram_range};
use crate::imports::Call;

pub const XC_MAX_ALL: u32 = 0xFFFF;
pub const REG_BINARY: u32 = 3;
pub const STATUS_BUFFER_TOO_SMALL: u32 = 0xC000_0023;

/// Proveniência da imagem, não prova de integridade ou de autenticidade física.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EepromSource {
    /// Gerador local, com bloco de segurança em claro; não é dump físico válido.
    SyntheticV1,
    /// Bytes fornecidos explicitamente pela aplicação; não recalcular checksums.
    Supplied,
}

/// Os bytes são autoritativos e imutáveis nesta etapa (`ExSave` ainda não existe).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EepromState {
    source: EepromSource,
    bytes: [u8; TAMANHO],
}

impl Default for EepromState {
    fn default() -> Self {
        // Perfil AOT inicial fixo; não lê configuração, arquivos ou BIOS do host.
        Self::synthetic(PadraoDeVideo::NtscM, Idioma::Portugues)
    }
}

impl EepromState {
    pub fn synthetic(video: PadraoDeVideo, language: Idioma) -> Self {
        Self {
            source: EepromSource::SyntheticV1,
            bytes: Eeprom::sintetica(video, language).bytes(),
        }
    }

    pub const fn supplied(bytes: [u8; TAMANHO]) -> Self {
        Self {
            source: EepromSource::Supplied,
            bytes,
        }
    }

    pub const fn source(&self) -> EepromSource {
        self.source
    }
    pub const fn bytes(&self) -> &[u8; TAMANHO] {
        &self.bytes
    }

    pub(super) fn query(&self, bus: &mut Bus, call: &Call) -> Resultado {
        let index = call.argument(bus, 0)?;
        let value_type = call.argument(bus, 1)?;
        let value = call.argument(bus, 2)?;
        let capacity = call.argument(bus, 3)?;
        let result_length = call.argument(bus, 4)?;
        if index != XC_MAX_ALL {
            return Err(Erro::VarianteNaoImplementada {
                nome: call.export.name,
                ordinal: call.export.ordinal,
                chamador: call.return_pc,
                detalhe: "consulta EEPROM implementa somente XC_MAX_ALL (0xFFFF)",
                argumentos: super::descrever_argumentos(bus, call),
            });
        }
        if !ram_range(bus, result_length, 4, Acesso::Escrita) {
            return Ok(Atendimento {
                eax: STATUS_ACCESS_VIOLATION,
            });
        }
        // Política de sondagem do perfil: buffer curto escreve apenas o tamanho
        // necessário; Type/Value não são acessados e podem ser nulos.
        if capacity < TAMANHO as u32 {
            bus.escrever_u32(GuestVirt(result_length), TAMANHO as u32)?;
            return Ok(Atendimento {
                eax: STATUS_BUFFER_TOO_SMALL,
            });
        }
        // Três saídas, uma operação lógica. Validar todas antes do primeiro store;
        // a posse exclusiva de Bus mantém os mapeamentos estáveis até o fim.
        if !ram_range(bus, value_type, 4, Acesso::Escrita)
            || !ram_range(bus, value, TAMANHO as u32, Acesso::Escrita)
        {
            return Ok(Atendimento {
                eax: STATUS_ACCESS_VIOLATION,
            });
        }
        // Ordem local explícita em aliases: Value, Type, ResultLength.
        bus.escrever_bytes(GuestVirt(value), &self.bytes)?;
        bus.escrever_u32(GuestVirt(value_type), REG_BINARY)?;
        bus.escrever_u32(GuestVirt(result_length), TAMANHO as u32)?;
        Ok(Atendimento {
            eax: STATUS_SUCCESS,
        })
    }
}

impl Snapshot for EepromState {
    fn save(&self, out: &mut SnapshotWriter) {
        out.section("EEPR", 1);
        out.u8(match self.source {
            EepromSource::SyntheticV1 => 1,
            EepromSource::Supplied => 2,
        });
        out.raw(&self.bytes);
    }

    fn load(&mut self, inp: &mut SnapshotReader<'_>) -> Result<(), SnapshotError> {
        let bad = |what| SnapshotError::BadValue {
            section: "EEPR",
            what,
        };
        if inp.section("EEPR", 1)? != 1 {
            return Err(bad("versão de EEPROM desconhecida"));
        }
        let source = match inp.u8()? {
            1 => EepromSource::SyntheticV1,
            2 => EepromSource::Supplied,
            _ => return Err(bad("origem de EEPROM desconhecida")),
        };
        let mut bytes = [0; TAMANHO];
        bytes.copy_from_slice(inp.raw(TAMANHO)?);
        *self = Self { source, bytes };
        Ok(())
    }
}
