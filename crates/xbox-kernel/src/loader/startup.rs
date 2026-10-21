//! Perfil inicial de uma thread: pilha, `NT_TIB` mínimo, TLS nxdk e imports.

use xbox_core::Error as CoreError;
use xbox_formats::Xbe;
use xbox_mem::{Acesso, Bus, GuestVirt, Perm};

use super::{Image, Mode, Result, data, mmio};
use crate::imports::{self, TRAMPOLINE_BASE, TRAMPOLINE_SIZE};

pub const STACK_BASE: u32 = 0x7000_0000;
pub const STACK_SIZE: u32 = 65536;
pub const TIB_BASE: u32 = 0x7FFD_0000;
pub const RETURN_SENTINEL: u32 = 0xFFFF_FFF0;

#[derive(Debug)]
pub struct Startup {
    pub image: Image,
    pub fs_base: u32,
    pub esp: u32,
}

impl Image {
    /// Construção transacional: uma imagem parcialmente preparada nunca escapa.
    /// Callbacks TLS e dados de kernel exigem implementação e são recusados.
    pub fn prepare_aot(bytes: &[u8]) -> Result<Startup> {
        let xbe = Xbe::parse(bytes)?;
        validate_startup_layout(&xbe)?;
        let exports = imports::plan(&xbe)?;
        let mut image = Self::load(bytes, Mode::Aot)?;
        let bus = &mut image.bus;
        // A origem e o índice precisam existir na imagem, antes das regiões novas.
        let template = if let Some(tls) = xbe.tls {
            let raw = tls
                .data_end_address
                .checked_sub(tls.data_start_address)
                .filter(|v| *v <= STACK_SIZE / 2)
                .ok_or_else(|| CoreError::format("faixa TLS invertida ou excessiva"))?;
            let mut template = vec![0; raw as usize];
            bus.ler_bytes(GuestVirt(tls.data_start_address), &mut template)?;
            let _ = bus.ler_u32(GuestVirt(tls.index_address))?;
            Some(template)
        } else {
            None
        };
        // MMIO é hardware, não import: a janela existe independentemente do que a
        // imagem importa, e o bus registra o que o guest toca nela.
        mmio::install(bus)?;
        map_fresh(bus, STACK_BASE, STACK_SIZE)?;
        map_fresh(bus, TIB_BASE, 4096)?;
        let stack_top = STACK_BASE + STACK_SIZE;
        bus.escrever_u32(GuestVirt(TIB_BASE), u32::MAX)?;
        bus.escrever_u32(GuestVirt(TIB_BASE + 4), stack_top)?;
        bus.escrever_u32(GuestVirt(TIB_BASE + 8), STACK_BASE)?;
        bus.escrever_u32(GuestVirt(TIB_BASE + 0x18), TIB_BASE)?;
        let mut esp = stack_top - 4;
        if let Some(tls) = xbe.tls {
            let alignment = (tls.characteristics >> 20) & 15;
            if tls.callback_address != 0 || tls.characteristics & !0x00F0_0000 != 0 || alignment > 5
            {
                return Err(CoreError::format(
                    "callbacks/alinhamento TLS ainda não suportados neste perfil nxdk",
                )
                .into());
            }
            let raw = tls
                .data_end_address
                .checked_sub(tls.data_start_address)
                .ok_or_else(|| CoreError::format("faixa TLS invertida"))?;
            let aligned = raw
                .checked_add(tls.size_of_zero_fill)
                .and_then(|v| v.checked_add(15))
                .map(|v| v & !15)
                .filter(|v| *v <= STACK_SIZE / 2)
                .ok_or_else(|| CoreError::format("TLS excede o limite de meia pilha inicial"))?;
            let size = aligned + 4;
            let slot = stack_top - size;
            let data = slot + 4;
            bus.escrever_bytes(GuestVirt(data), template.as_deref().unwrap_or_default())?;
            // A arena começa zerada: zero-fill e padding permanecem zero.
            bus.escrever_u32(GuestVirt(slot), data)?;
            // nxdk usa FS:[4] + (_tls_index * 4), com índice negativo.
            patch_u32(bus, tls.index_address, 0u32.wrapping_sub(size / 4))?;
            esp = slot - 4;
        }
        bus.escrever_u32(GuestVirt(esp), RETURN_SENTINEL)?;
        if !exports.is_empty() {
            let funcoes = exports
                .iter()
                .any(|e| matches!(e.kind, imports::Kind::Function(_)));
            let dados = exports.iter().any(|e| e.kind == imports::Kind::Data);
            if funcoes {
                map_fresh(bus, TRAMPOLINE_BASE, TRAMPOLINE_SIZE)?;
            }
            if dados {
                data::install(bus, data::DEFAULT_IMAGE_PATH)?;
            }
            // Slots não importados permanecem inválidos, mesmo com ordinal conhecido.
            for (index, export) in exports.iter().enumerate() {
                let target = imports::thunk_target(export)?;
                if matches!(export.kind, imports::Kind::Function(_)) {
                    bus.escrever_bytes(GuestVirt(target), &imports::marker(export.ordinal))?;
                }
                patch_u32(bus, xbe.kernel_thunk + index as u32 * 4, target)?;
            }
            if funcoes {
                let frame = bus.traduzir(GuestVirt(TRAMPOLINE_BASE), Acesso::Leitura)?;
                bus.mapear(GuestVirt(TRAMPOLINE_BASE), frame, 1, Perm::RX)?;
            }
        }
        Ok(Startup {
            image,
            fs_base: TIB_BASE,
            esp,
        })
    }
}

/// As regiões de startup não podem substituir páginas da imagem, mesmo quando
/// esta não possui TLS ou imports. Também é usado pela análise antes da emissão.
pub fn validate_startup_layout(xbe: &Xbe<'_>) -> xbox_core::Result<()> {
    super::validate_layout(xbe)?;
    let ranges = std::iter::once((xbe.header.base_address, xbe.header.size_of_headers)).chain(
        xbe.sections
            .iter()
            .map(|s| (s.virtual_address, s.virtual_size)),
    );
    for (base, size) in ranges.filter(|(_, size)| *size != 0) {
        // Reservadas sempre, mesmo quando a imagem não usa a região: assim o resultado
        // da validação de layout não depende de quais imports a imagem tem.
        for (reserved, length) in [
            (STACK_BASE, STACK_SIZE),
            (TIB_BASE, 4096),
            (TRAMPOLINE_BASE, TRAMPOLINE_SIZE),
            (data::DATA_RO_BASE, data::DATA_SIZE),
            (data::DATA_RW_BASE, data::DATA_SIZE),
            (mmio::NV2A_BASE, mmio::NV2A_SIZE),
        ] {
            if base < reserved + length && base + size > reserved {
                return Err(CoreError::format(format!(
                    "imagem ocupa região reservada: 0x{reserved:08X}"
                )));
            }
        }
    }
    Ok(())
}

pub(super) fn map_fresh(bus: &mut Bus, base: u32, size: u32) -> Result<()> {
    for va in (base..base + size).step_by(4096) {
        if bus.mmu().consultar(GuestVirt(va)).is_some() {
            return Err(
                CoreError::format(format!("imagem ocupa região reservada: 0x{va:08X}")).into(),
            );
        }
        let frame = bus
            .frames_mut()
            .alocar()
            .ok_or_else(|| CoreError::format("RAM insuficiente para startup AOT"))?;
        bus.mapear(GuestVirt(va), frame, 1, Perm::RW)?;
    }
    Ok(())
}

/// Patch do loader em dados/header RO, com restauração de proteção. Nunca altera
/// bytes executáveis: o Rust traduzido precisa continuar correspondendo à imagem.
fn patch_u32(bus: &mut Bus, va: u32, value: u32) -> Result<()> {
    let end = va
        .checked_add(3)
        .ok_or_else(|| CoreError::format("patch transborda"))?;
    let mut pages = Vec::new();
    for page in (va & !4095..=(end & !4095)).step_by(4096) {
        let (frame, perm) = bus
            .mmu()
            .consultar(GuestVirt(page))
            .ok_or_else(|| CoreError::format("patch fora da imagem mapeada"))?;
        if perm.permite(Acesso::Execucao) {
            return Err(
                CoreError::format("patch TLS/import não pode alterar página executável").into(),
            );
        }
        pages.push((page, frame, perm));
    }
    for &(page, frame, _) in &pages {
        bus.mapear(GuestVirt(page), frame, 1, Perm::RW)?;
    }
    let result = bus.escrever_u32(GuestVirt(va), value);
    for (page, frame, perm) in pages {
        bus.mapear(GuestVirt(page), frame, 1, perm)?;
    }
    Ok(result?)
}
