//! Configuração nominal compartilhada entre CPU e chipset guest (ADR-0039).
//! Referências: nxdk profiling.c, Intel P6 e xboxdevwiki PCI, controlador 00.00:3.

/// Perfil inicial de clock. Valores públicos/sintéticos, sem acesso ao hardware host.
#[derive(Debug)]
pub struct RetailClockV1;

impl RetailClockV1 {
    /// Configuração funcional P6: bus 133 MHz, razão de CPU 5,5; demais bits zero.
    pub const EBL_CR_POWERON: u64 = 0x0104_0000;
    /// Registro PCI 0:0:3, offset 0x6C, publicado no inventário do `XboxDevWiki`.
    /// Divisor = byte 0 (1), multiplicador = byte 1 (8). Outros efeitos não modelados.
    pub const FSB_PLL_CONFIG: u32 = 0x0123_0801;
    /// Cristal nominal em unidades de 0,001 Hz, como usado pelo nxdk fixado.
    pub const CRYSTAL_MILLIHZ: u64 = 16_666_666_666;
    pub const CPU_RATIO_TENTHS: u64 = 55;

    /// Equação de frequência guest com os arredondamentos usados pelo nxdk.
    pub const fn cpu_hz() -> u64 {
        let divider = (Self::FSB_PLL_CONFIG & 0xFF) as u64;
        let multiplier = ((Self::FSB_PLL_CONFIG >> 8) & 0xFF) as u64;
        let fsb_millihz = Self::CRYSTAL_MILLIHZ / divider * multiplier;
        fsb_millihz * Self::CPU_RATIO_TENTHS / 10 / 1000
    }
}

// Qualquer mudança de perfil tem de manter o domínio nominal de Clock coerente.
const _: () = assert!(RetailClockV1::cpu_hz() == crate::time::CPU_HZ);
