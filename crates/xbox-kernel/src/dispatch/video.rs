//! Encoder sintético de cabo padrão; não programa NV2A ou hardware host.
use super::{Atendimento, EepromState, Erro, Resultado, descrever_argumentos, ram_range};
use crate::imports::Call;
use xbox_core::{Snapshot, SnapshotReader, SnapshotWriter, error::SnapshotError};
use xbox_formats::eeprom::{Eeprom, FlagsDeVideo, PadraoDeVideo};
use xbox_mem::{Acesso, Bus, GuestVirt};

pub const VIDEO_BASE: u32 = 0xFD00_0000;
pub const VIDEO_ENC_GET_SETTINGS: u32 = 6;
pub const VIDEO_ENC_VIDEOENABLE: u32 = 9;
pub const AV_PACK_STANDARD: u32 = 1;
const FLAGS: u32 = FlagsDeVideo::WIDESCREEN
    | FlagsDeVideo::MODO_480P
    | FlagsDeVideo::MODO_720P
    | FlagsDeVideo::MODO_1080I
    | FlagsDeVideo::LETTERBOX
    | 0x0040_0000
    | 0x0080_0000;

/// Perfil de cabo padrão e cold boot, inicialmente sem saída habilitada.
/// Não representa um framebuffer, vblank ou a execução do scanout.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct VideoState {
    enabled: bool,
}
impl VideoState {
    /// Cold boot sem framebuffer herdado. `AvSetSavedDataAddress` ainda recusa.
    pub const SAVED_DATA_ADDRESS: u32 = 0;
    pub const fn enabled(&self) -> bool {
        self.enabled
    }
    pub(super) fn encoder(
        &mut self,
        bus: &mut Bus,
        call: &Call,
        eeprom: &EepromState,
    ) -> Resultado {
        let base = call.argument(bus, 0)?;
        let option = call.argument(bus, 1)?;
        let parameter = call.argument(bus, 2)?;
        let output = call.argument(bus, 3)?;
        if base != VIDEO_BASE {
            return Err(variant(bus, call, "RegisterBase exige a base NV2A guest"));
        }
        if option == VIDEO_ENC_VIDEOENABLE {
            if parameter > 1 || output != 0 {
                return Err(variant(
                    bus,
                    call,
                    "VIDEOENABLE admite blanking 0/1 e Result nulo",
                ));
            }
            // O nxdk passa TRUE para desabilitar o sinal: Param é blanking.
            self.enabled = parameter == 0;
            return Ok(Atendimento { eax: 0 });
        }
        if option != VIDEO_ENC_GET_SETTINGS || parameter != 0 {
            return Err(variant(
                bus,
                call,
                "perfil admite GET_SETTINGS com base NV2A e Param zero",
            ));
        }
        if !ram_range(bus, output, 4, Acesso::Escrita) {
            return Err(Erro::ArgumentoInvalido {
                nome: call.export.name,
                ordinal: call.export.ordinal,
                detalhe: "Result exige quatro bytes de RAM guest gravável".to_owned(),
            });
        }
        let decoded = Eeprom::parse(eeprom.bytes()).map_err(crate::loader::Error::from)?;
        if matches!(decoded.padrao_de_video, PadraoDeVideo::Desconhecido(_))
            || decoded.flags_de_video.0 & !FLAGS != 0
        {
            return Err(variant(
                bus,
                call,
                "padrão/flags de vídeo da EEPROM fora do perfil modelado",
            ));
        }
        let settings =
            decoded.padrao_de_video.valor() | decoded.flags_de_video.0 | AV_PACK_STANDARD;
        bus.escrever_u32(GuestVirt(output), settings)?;
        Ok(Atendimento { eax: 0 }) // VOID, não confundir EAX com Result.
    }
}
impl Snapshot for VideoState {
    fn save(&self, out: &mut SnapshotWriter) {
        out.section("AVST", 1);
        out.u32(1); // StandardCableColdBootV1.
        out.bool(self.enabled);
    }
    fn load(&mut self, inp: &mut SnapshotReader<'_>) -> Result<(), SnapshotError> {
        let bad = |what| SnapshotError::BadValue {
            section: "AVST",
            what,
        };
        if inp.section("AVST", 1)? != 1 || inp.u32()? != 1 {
            return Err(bad("versão/perfil de encoder desconhecido"));
        }
        let enabled = match inp.u8()? {
            0 => false,
            1 => true,
            _ => return Err(bad("estado de blanking inválido")),
        };
        *self = Self { enabled };
        Ok(())
    }
}
pub(super) fn variant(bus: &Bus, call: &Call, detalhe: &'static str) -> Erro {
    Erro::VarianteNaoImplementada {
        nome: call.export.name,
        ordinal: call.export.ordinal,
        chamador: call.return_pc,
        detalhe,
        argumentos: descrever_argumentos(bus, call),
    }
}
