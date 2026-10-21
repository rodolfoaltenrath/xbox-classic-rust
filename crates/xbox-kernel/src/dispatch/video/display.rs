//! Modo HLE inicial e visão emprestada do framebuffer por DMA.
use super::{VideoState, variant};
use crate::dispatch::{Atendimento, ContiguousMemory, EepromState, Erro, Resultado};
use crate::imports::Call;
use xbox_formats::eeprom::{Eeprom, PadraoDeVideo};
use xbox_mem::{Bus, GuestPhys};

pub const MODE_640_480_NTSC_M: u32 = 0x0401_0101;
pub const COLOR_A8R8G8B8: u32 = 0x12;
const WIDTH: u32 = 640;
const HEIGHT: u32 = 480;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayMode {
    pub(super) id: u32,
    pub(super) format: u32,
    pub(super) pitch: u32,
    pub(super) framebuffer: GuestPhys,
}
impl DisplayMode {
    pub const fn id(self) -> u32 {
        self.id
    }
    pub const fn format(self) -> u32 {
        self.format
    }
    pub const fn pitch(self) -> u32 {
        self.pitch
    }
    pub const fn framebuffer(self) -> GuestPhys {
        self.framebuffer
    }
    pub const fn width(self) -> u32 {
        WIDTH
    }
    pub const fn height(self) -> u32 {
        HEIGHT
    }
    pub const fn refresh_hz(self) -> u32 {
        60
    }
    pub const fn byte_len(self) -> u32 {
        self.pitch * HEIGHT
    }
    pub(super) fn valid(self) -> bool {
        self.id == MODE_640_480_NTSC_M
            && self.format == COLOR_A8R8G8B8
            && (WIDTH * 4..=16384).contains(&self.pitch)
            && self.pitch.is_multiple_of(4)
            && self.framebuffer.0.is_multiple_of(4)
            && self
                .pitch
                .checked_mul(HEIGHT)
                .and_then(|n| self.framebuffer.0.checked_add(n))
                .is_some()
    }
    fn backed(self, memory: &ContiguousMemory) -> bool {
        self.valid()
            && memory.allocations().iter().any(|a| {
                self.framebuffer.0 >= a.physical.0
                    && u64::from(self.framebuffer.0) + u64::from(self.byte_len())
                        <= u64::from(a.physical.0) + u64::from(a.size)
            })
    }
}
/// Inspeção de RAM configurada; enabled informa se a saída está habilitada.
/// O empréstimo impede retomar a CPU com acesso mutável ao Bus enquanto usado.
#[derive(Debug)]
pub struct FramebufferView<'a> {
    mode: DisplayMode,
    enabled: bool,
    bytes: &'a [u8],
}
impl<'a> FramebufferView<'a> {
    pub const fn mode(&self) -> DisplayMode {
        self.mode
    }
    pub const fn enabled(&self) -> bool {
        self.enabled
    }
    /// Bytes B,G,R,A de uma linha visível, excluindo padding de pitch.
    pub fn row(&self, y: u32) -> Option<&'a [u8]> {
        if y >= HEIGHT {
            return None;
        }
        let start = (y * self.mode.pitch) as usize;
        self.bytes.get(start..start + (WIDTH * 4) as usize)
    }
}
impl VideoState {
    pub const fn mode(&self) -> Option<DisplayMode> {
        self.mode
    }
    pub(in crate::dispatch) fn set_mode(
        &mut self,
        bus: &Bus,
        call: &Call,
        eeprom: &EepromState,
        memory: &ContiguousMemory,
    ) -> Resultado {
        let mut args = [0; 6];
        for (i, arg) in args.iter_mut().enumerate() {
            *arg = call.argument(bus, i as u32)?;
        }
        let [base, step, id, format, pitch, physical] = args;
        if base != super::VIDEO_BASE
            || step != 0
            || id != MODE_640_480_NTSC_M
            || format != COLOR_A8R8G8B8
        {
            return Err(variant(
                bus,
                call,
                "perfil admite Step=0 e modo NTSC-M 640x480 A8R8G8B8 na base NV2A",
            ));
        }
        let mode = DisplayMode {
            id,
            format,
            pitch,
            framebuffer: GuestPhys(physical),
        };
        if !mode.valid() || !mode.backed(memory) {
            return Err(Erro::ArgumentoInvalido {
                nome: call.export.name,
                ordinal: call.export.ordinal,
                detalhe: "pitch/endereço/framebuffer fora de um recurso físico contíguo possuído"
                    .to_owned(),
            });
        }
        if !ntsc_m(eeprom) {
            return Err(variant(
                bus,
                call,
                "modo NTSC-M exige padrão NTSC-M na EEPROM",
            ));
        }
        memory.validate_bus(bus).map_err(Erro::MemoriaContigua)?;
        self.mode = Some(mode);
        Ok(Atendimento { eax: 0 }) // Etapa HLE concluída; não repete o laço de Step.
    }
    pub(in crate::dispatch) fn validate_memory(
        &self,
        memory: &ContiguousMemory,
        eeprom: &EepromState,
    ) -> Result<(), &'static str> {
        if self
            .mode
            .is_some_and(|mode| !mode.backed(memory) || !ntsc_m(eeprom))
        {
            return Err("modo de vídeo diverge da EEPROM ou do recurso contíguo");
        }
        Ok(())
    }
    pub(in crate::dispatch) fn framebuffer<'a>(
        &self,
        bus: &'a Bus,
        memory: &ContiguousMemory,
        eeprom: &EepromState,
    ) -> Result<Option<FramebufferView<'a>>, Erro> {
        self.validate_memory(memory, eeprom)
            .map_err(Erro::VideoState)?;
        let Some(mode) = self.mode else {
            return Ok(None);
        };
        memory.validate_bus(bus).map_err(Erro::MemoriaContigua)?;
        let bytes = bus.dma(mode.framebuffer, mode.byte_len())?;
        Ok(Some(FramebufferView {
            mode,
            enabled: self.enabled,
            bytes,
        }))
    }
}
fn ntsc_m(eeprom: &EepromState) -> bool {
    Eeprom::parse(eeprom.bytes()).is_ok_and(|e| e.padrao_de_video == PadraoDeVideo::NtscM)
}
