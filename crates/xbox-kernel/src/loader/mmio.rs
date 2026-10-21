//! Janela virtual de MMIO do NV2A, mapeada como identidade sobre o físico.
//!
//! A revisão da Fase 2 (`docs/arch/memoria.md`) fixou que `0xFD000000+` é MMIO com
//! mapeamento identidade, montado pelo kernel na tabela de páginas. O bus já classifica
//! esse físico como dispositivo, mas **ninguém montava a janela**: o `XVideoSetMode` do
//! nxdk escreve a rampa de gamma em `0xFD6813C8` e a MMU respondia `NaoMapeado`, que é
//! falha de página — algo que o console real nunca produziria nesse endereço.
//!
//! ## O que isto implementa, e o que não implementa
//!
//! **Só o mapeamento.** O dispositivo continua sem modelo: o bus registra cada acesso na
//! telemetria (endereço, operação, tamanho, primeiro valor, PC quando conhecido), as
//! escritas são descartadas e as leituras devolvem **zero como placeholder declarado**,
//! não como comportamento observado em hardware (ver `xbox_mem::dispositivos`).
//!
//! Isso é deliberado: o relatório de telemetria vira a lista exata de registradores que
//! o título toca, em vez de uma leitura especulativa da documentação do NV2A. Modelar um
//! registrador sem evidência de que o caminho executado o usa seria inventar hardware.
//!
//! ## Por que a janela inteira, e não só as páginas do vídeo
//!
//! O hello toca quatro sub-blocos (PMC, PCRTC, PCIO, PRAMDAC). Mapear só as páginas
//! conhecidas faria o primeiro registrador novo cair em `NaoMapeado`, o que para a
//! execução num erro que **não identifica o registrador** — o ciclo de "rodar, ler o
//! erro, converter o endereço, tentar de novo" que a telemetria elimina.
//!
//! ## Permissão
//!
//! Leitura e escrita, nunca execução: registrador não é código.

use xbox_mem::{Bus, GuestPhys, GuestVirt, Perm};

use super::Result;

/// Início da janela do NV2A.
pub const NV2A_BASE: u32 = 0xFD00_0000;
/// Tamanho da janela do NV2A: 16 MiB.
pub const NV2A_SIZE: u32 = 0x0100_0000;

/// Páginas grandes (4 MiB) necessárias para cobrir a janela.
const PAGINAS_GRANDES: u32 = NV2A_SIZE / xbox_core::LARGE_PAGE_SIZE as u32;

/// Instala a janela de MMIO do NV2A como identidade sobre o físico.
///
/// Página grande por escolha de custo: quatro entradas de diretório em vez de 4096 de
/// tabela, e a janela é contígua e uniforme (todas RW, nenhuma execução).
pub fn install(bus: &mut Bus) -> Result<()> {
    for indice in 0..PAGINAS_GRANDES {
        let base = NV2A_BASE + indice * xbox_core::LARGE_PAGE_SIZE as u32;
        bus.mmu_mut()
            .mapear_grande(GuestVirt(base), GuestPhys(base), Perm::RW)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{NV2A_BASE, NV2A_SIZE};

    #[test]
    fn janela_e_multipla_de_pagina_grande_e_nao_transborda() {
        assert_eq!(NV2A_SIZE % xbox_core::LARGE_PAGE_SIZE as u32, 0);
        assert!(
            NV2A_BASE.checked_add(NV2A_SIZE).is_some(),
            "a janela não pode transbordar o espaço de 32 bits"
        );
        assert_eq!(NV2A_BASE % xbox_core::LARGE_PAGE_SIZE as u32, 0);
    }
}
