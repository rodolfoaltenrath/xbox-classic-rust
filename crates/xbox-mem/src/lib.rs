//! Memória do guest: arena, MMU, classificação de regiões e barramento.
//!
//! Implementa a Fase 2 do [ROADMAP](../../../ROADMAP.md), conforme a revisão
//! arquitetural em `docs/arch/memoria.md` e os ADRs 0009 a 0013.
//!
//! ## O que este crate garante
//!
//! 1. **Espaços de endereçamento distintos** ([`addr`]): virtual do guest, físico do
//!    Xbox e deslocamento na arena são tipos diferentes, sem conversão implícita. O bug
//!    de passar um virtual onde se espera um físico não compila.
//! 2. **Endereço inválido do guest nunca alcança memória do host.** Toda tradução
//!    devolve `Result`, toda região é classificada explicitamente e o acesso final é
//!    indexação verificada em `&[u8]`.
//! 3. **Semântica de memória do x86**: acesso desalinhado e atravessando página
//!    funcionam, com o endereço correto em `CR2` quando falham.
//! 4. **Nada de comportamento de hardware inventado**: acesso a dispositivo não emulado
//!    é registrado com endereço, operação, tamanho e contexto
//!    ([`dispositivos::Telemetria`]), e a resposta é um placeholder declarado.
//!
//! ## `unsafe`
//!
//! Vive **só** em [`arena`]: 16 blocos (contando ambos os sistemas) e um `unsafe impl Send`, com invariantes em
//! `docs/arch/memoria-unsafe.md`. Todo o resto do crate é Rust seguro e verificável por
//! Miri.
//!
//! ## O que não está aqui, de propósito
//!
//! - **fastmem**: adiado para a Fase 3b pelo ADR-0010, depois de medição;
//! - **janela tiled**: reconhecida e recusada; é transformação de endereço, não
//!   espelho, e o formato é definido pela GPU (Fase 5);
//! - **cache de tradução (TLB)**: o percurso de dois níveis é duas indexações de vetor;
//!   acrescentar cache sem medir seria repetir o erro que o ADR-0010 corrigiu;
//! - **emulação de qualquer dispositivo**: Fases 5, 6 e 7.
//!
//! ## Exemplo
//!
//! ```
//! use xbox_mem::{Acesso, Bus, GuestPhys, GuestVirt, Perm};
//!
//! let mut bus = Bus::novo()?;
//!
//! // O kernel aloca um frame e mapeia uma página do título.
//! let fisico = bus.frames_mut().alocar().expect("RAM livre");
//! bus.mapear(GuestVirt(0x0001_0000), fisico, 1, Perm::RWX)?;
//!
//! bus.escrever_u32(GuestVirt(0x0001_0010), 0xCAFE_BABE)?;
//! assert_eq!(bus.ler_u32(GuestVirt(0x0001_0010))?, 0xCAFE_BABE);
//!
//! // Endereço não mapeado falha de forma controlada, com CR2 correto.
//! let falta = bus.ler_u32(GuestVirt(0x0002_0000)).unwrap_err();
//! assert!(falta.e_falha_de_pagina());
//! assert_eq!(falta.endereco, GuestVirt(0x0002_0000));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

pub mod addr;

// `unsafe` é permitido **apenas** neste módulo. O resto do crate continua sob o
// `unsafe_code = "deny"` do workspace, e `undocumented_unsafe_blocks` torna erro um
// bloco sem `// SAFETY:` — a disciplina do ROADMAP §13.3 vira falha de compilação em
// vez de combinado verbal.
#[allow(unsafe_code)]
#[deny(clippy::undocumented_unsafe_blocks)]
pub mod arena;

pub mod bus;
pub mod dispositivos;
pub mod frames;
pub mod mmu;
pub mod regioes;

pub use addr::{Acesso, ArenaOff, GuestPhys, GuestVirt, Perm};
pub use arena::Arena;
pub use bus::Bus;
pub use dispositivos::{Contexto, Dispositivo, Motivo, Operacao, Registro, Telemetria};
pub use frames::AlocadorDeFrames;
pub use mmu::{Falta, Mmu, MotivoDeFalta};
pub use regioes::{MapaFisico, Regiao};

/// Fase do roadmap em que este subsistema é implementado.
pub const ROADMAP_PHASE: &str = "Fase 2";

/// Endereços das janelas virtuais que o kernel do Xbox monta.
///
/// ⚠️ Valores de documentação pública, **não validados contra hardware**. A validação
/// vem nas Fases 3 e 4, quando código de jogo de verdade os exercitar — ver o aviso em
/// [`regioes`] e `docs/compat/formatos.md`.
pub mod janelas {
    use crate::addr::GuestVirt;

    /// Onde a imagem do título é carregada.
    pub const IMAGEM_DO_TITULO: GuestVirt = GuestVirt(0x0001_0000);

    /// Janela contígua: mapeia a RAM física 1:1, cacheada.
    ///
    /// É o que `MmAllocateContiguousMemory` devolve, e é por isso que o par
    /// virtual/físico precisa de tipos distintos (ADR-0009).
    pub const CONTIGUA: GuestVirt = GuestVirt(0x8000_0000);

    /// Janela *tiled*. Reconhecida e não implementada (Fase 5).
    pub const TILED: GuestVirt = GuestVirt(0xF000_0000);
}

#[cfg(test)]
mod tests {
    use crate::{Acesso, Bus, GuestPhys, GuestVirt, MotivoDeFalta, Perm, janelas};

    #[test]
    fn fase_declarada() {
        assert_eq!(super::ROADMAP_PHASE, "Fase 2");
    }

    #[test]
    fn janela_contigua_espelha_a_imagem_do_titulo() {
        // O arranjo real: a mesma RAM vista por duas janelas virtuais.
        let mut bus = Bus::com_arena(crate::Arena::com_tamanho(8 * 1024 * 1024).unwrap()).unwrap();
        let fisico = bus.frames_mut().alocar_contiguo(1).unwrap();

        bus.mapear(janelas::IMAGEM_DO_TITULO, fisico, 1, Perm::RWX)
            .unwrap();
        bus.mapear(
            GuestVirt(janelas::CONTIGUA.0 + fisico.0),
            fisico,
            1,
            Perm::RW,
        )
        .unwrap();

        bus.escrever_u32(janelas::IMAGEM_DO_TITULO, 0x1234_5678)
            .unwrap();
        let pela_contigua = bus
            .ler_u32(GuestVirt(janelas::CONTIGUA.0 + fisico.0))
            .unwrap();
        assert_eq!(pela_contigua, 0x1234_5678);

        // E a permissão é por janela: a contígua não é executável.
        assert!(
            bus.traduzir(janelas::IMAGEM_DO_TITULO, Acesso::Execucao)
                .is_ok()
        );
        assert!(
            bus.traduzir(GuestVirt(janelas::CONTIGUA.0 + fisico.0), Acesso::Execucao)
                .is_err()
        );
    }

    #[test]
    fn escrita_cruzando_pagina_valida_toda_faixa_antes_de_alterar_ram() {
        let mut bus = Bus::novo().unwrap();
        let frame = bus.frames_mut().alocar().unwrap();
        bus.mapear(GuestVirt(0x1000), frame, 1, Perm::RW).unwrap();
        bus.escrever_u16(GuestVirt(0x1FFE), 0x1234).unwrap();
        for second in [
            None,
            Some((frame, Perm::RO)),
            Some((GuestPhys(0x40000000), Perm::RW)),
        ] {
            if let Some((pa, perm)) = second {
                bus.mapear(GuestVirt(0x2000), pa, 1, perm).unwrap();
            }
            let fault = bus.escrever_u32(GuestVirt(0x1FFE), u32::MAX).unwrap_err();
            assert_eq!(fault.endereco, GuestVirt(0x2000));
            assert_eq!(fault.acesso, Acesso::Escrita);
            assert_eq!(bus.ler_u16(GuestVirt(0x1FFE)).unwrap(), 0x1234);
        }
        // Fim do espaço virtual: transbordo também falha antes da escrita.
        bus.mapear(GuestVirt(0xFFFFF000), frame, 1, Perm::RW)
            .unwrap();
        let fault = bus.escrever_u32(GuestVirt(0xFFFFFFFE), 0).unwrap_err();
        assert_eq!(fault.motivo, MotivoDeFalta::Transbordo);
        assert_eq!(bus.ler_u16(GuestVirt(0xFFFFFFFE)).unwrap(), 0x1234);
    }
}
