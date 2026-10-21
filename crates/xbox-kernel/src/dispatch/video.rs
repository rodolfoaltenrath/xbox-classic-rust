//! Encoder sintético de cabo padrão; não programa NV2A ou hardware host.
use super::{Atendimento, EepromState, Erro, Resultado, descrever_argumentos, ram_range};
use crate::imports::Call;
use xbox_core::{Snapshot, SnapshotReader, SnapshotWriter, error::SnapshotError};
use xbox_formats::eeprom::{Eeprom, FlagsDeVideo, PadraoDeVideo};
use xbox_mem::{Acesso, Bus, GuestVirt};
mod display;
pub use display::{COLOR_A8R8G8B8, DisplayMode, FramebufferView, MODE_640_480_NTSC_M};

pub const VIDEO_BASE: u32 = 0xFD00_0000;
pub const VIDEO_ENC_GET_SETTINGS: u32 = 6;
pub const VIDEO_ENC_VIDEOENABLE: u32 = 9;
pub const VIDEO_ENC_FLICKERFILTER: u32 = 11;
pub const VIDEO_ENC_SOFTEN_FILTER: u32 = 14;
pub const AV_PACK_STANDARD: u32 = 1;
const FLAGS: u32 = FlagsDeVideo::WIDESCREEN
    | FlagsDeVideo::MODO_480P
    | FlagsDeVideo::MODO_720P
    | FlagsDeVideo::MODO_1080I
    | FlagsDeVideo::LETTERBOX
    | 0x0040_0000
    | 0x0080_0000;

/// Perfil de cabo padrão e cold boot, inicialmente sem saída habilitada.
/// Modo e RAM de scanout são explícitos; vblank/apresentação não são executados.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct VideoState {
    enabled: bool,
    mode: Option<DisplayMode>,
    // Sem valor herdado de firmware no cold boot; só pedidos guest observados.
    flicker_filter: Option<u8>,
    soften_filter: Option<bool>,
}
impl VideoState {
    /// Cold boot sem framebuffer herdado. `AvSetSavedDataAddress` ainda recusa.
    pub const SAVED_DATA_ADDRESS: u32 = 0;
    pub const fn enabled(&self) -> bool {
        self.enabled
    }
    /// Nível de três bits solicitado pelo guest, sem simular o filtro analógico.
    pub const fn flicker_filter(&self) -> Option<u8> {
        self.flicker_filter
    }
    /// Pedido de suavização; None até o guest configurar explicitamente.
    pub const fn soften_filter(&self) -> Option<bool> {
        self.soften_filter
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
        if option == VIDEO_ENC_FLICKERFILTER || option == VIDEO_ENC_SOFTEN_FILTER {
            let max = if option == VIDEO_ENC_FLICKERFILTER {
                7
            } else {
                1
            };
            if parameter > max || output != 0 {
                return Err(variant(
                    bus,
                    call,
                    "flicker exige nível 0..7, soften exige 0/1, ambos com Result nulo",
                ));
            }
            if option == VIDEO_ENC_FLICKERFILTER {
                self.flicker_filter = Some(parameter as u8);
            } else {
                self.soften_filter = Some(parameter != 0);
            }
            return Ok(Atendimento { eax: 0 }); // VOID; pedido registrado no encoder.
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
                "opção ou Param ainda não implementado no perfil do encoder",
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
        out.section("AVST", 3);
        out.u32(1); // StandardCableColdBootV1.
        out.bool(self.enabled);
        out.bool(self.mode.is_some());
        if let Some(mode) = self.mode {
            out.u32(mode.id);
            out.u32(mode.format);
            out.u32(mode.pitch);
            out.u32(mode.framebuffer.0);
        }
        // 0xFF representa pedido ausente, sem inventar defaults do hardware.
        out.u8(self.flicker_filter.unwrap_or(0xFF));
        out.u8(self.soften_filter.map_or(0xFF, u8::from));
    }
    fn load(&mut self, inp: &mut SnapshotReader<'_>) -> Result<(), SnapshotError> {
        let bad = |what| SnapshotError::BadValue {
            section: "AVST",
            what,
        };
        let version = inp.section("AVST", 3)?;
        if !matches!(version, 1..=3) || inp.u32()? != 1 {
            return Err(bad("versão/perfil de encoder desconhecido"));
        }
        let enabled = match inp.u8()? {
            0 => false,
            1 => true,
            _ => return Err(bad("estado de blanking inválido")),
        };
        let mode = if version >= 2 {
            match inp.u8()? {
                0 => None,
                1 => {
                    let mode = DisplayMode {
                        id: inp.u32()?,
                        format: inp.u32()?,
                        pitch: inp.u32()?,
                        framebuffer: xbox_mem::GuestPhys(inp.u32()?),
                    };
                    if !mode.valid() {
                        return Err(bad("geometria/formato de modo inválido"));
                    }
                    Some(mode)
                }
                _ => return Err(bad("indicador de modo inválido")),
            }
        } else {
            None
        };
        let (flicker_filter, soften_filter) = if version >= 3 {
            let flicker = match inp.u8()? {
                level @ 0..=7 => Some(level),
                0xFF => None,
                _ => return Err(bad("nível de flicker inválido")),
            };
            let soften = match inp.u8()? {
                0 => Some(false),
                1 => Some(true),
                0xFF => None,
                _ => return Err(bad("pedido de suavização inválido")),
            };
            (flicker, soften)
        } else {
            (None, None)
        };
        *self = Self {
            enabled,
            mode,
            flicker_filter,
            soften_filter,
        };
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
