//! Traits de backend de SO: display, input, audio, timer, memoria virtual
//!
//! **Estado:** apresentação de quadros implementada para Windows (ADR-0052); o resto das
//! fronteiras (input, áudio, timer) continua esqueleto, na **Fase 0+** do
//! [ROADMAP](../../../ROADMAP.md).
//!
//! Este crate é um dos autorizados a usar `unsafe` (ROADMAP §13.3). Toda construção
//! `unsafe` precisa de um comentário `// SAFETY:` declarando a invariante mantida.
//!
//! É também o **único** lugar onde `#[cfg(target_os)]` pode aparecer (ROADMAP §2): quem
//! usa este crate enxerga [`display::Apresentador`] e [`criar_janela`], nunca o sistema
//! operacional por baixo.
#![allow(unsafe_code)]

pub mod display;

#[cfg(windows)]
mod windows;

use xbox_core::Result;

/// Fase do roadmap em que este subsistema é implementado.
pub const ROADMAP_PHASE: &str = "Fase 0+";

/// Cria uma janela que apresenta quadros de `largura × altura`.
///
/// Só existe implementação para **Windows**. Em outro sistema devolve erro dizendo qual é
/// a lacuna, em vez de cair silenciosamente num apresentador que não mostra nada: um
/// emulador que "apresenta" para lugar nenhum é pior que um que avisa. O Linux desktop e o
/// modo appliance (DRM/KMS) são trabalho das Fases 8 e 10.
#[cfg(windows)]
pub fn criar_janela(
    titulo: &str,
    largura: u32,
    altura: u32,
) -> Result<Box<dyn display::Apresentador>> {
    Ok(Box::new(windows::Janela::nova(titulo, largura, altura)?))
}

/// Cria uma janela que apresenta quadros de `largura × altura` (indisponível aqui).
#[cfg(not(windows))]
pub fn criar_janela(
    _titulo: &str,
    _largura: u32,
    _altura: u32,
) -> Result<Box<dyn display::Apresentador>> {
    Err(xbox_core::Error::not_implemented(
        "janela de apresentação neste sistema operacional",
        "Fases 8 e 10",
    ))
}

#[cfg(test)]
mod tests {
    #[cfg(not(windows))]
    #[test]
    fn fora_do_windows_a_janela_diz_que_nao_existe_em_vez_de_fingir() {
        let erro = super::criar_janela("x", 640, 480).unwrap_err().to_string();
        assert!(erro.contains("Fases 8 e 10"), "{erro}");
    }
}
