//! Parsers dos formatos de arquivo do Xbox.
//!
//! Tudo aqui lê **dado do usuário**: executáveis, imagens de disco, partições de HD,
//! dumps de EEPROM. Por isso o crate segue duas regras rígidas (ROADMAP §13.3):
//!
//! 1. nenhum acesso direto a `&[u8]` — toda leitura passa por [`bytes`], que devolve
//!    erro com a posição exata em vez de entrar em pânico;
//! 2. nenhum `unwrap`/`expect` em caminho de parsing; arquivo malformado é
//!    `Err`, sempre.
//!
//! Essas regras são verificadas por testes de robustez (`tests/robustez.rs`), que
//! truncam e corrompem fixtures válidas e exigem que nada entre em pânico nem deixe de
//! terminar. Os alvos equivalentes para `cargo-fuzz` estão em `fuzz/`.
//!
//! ## Módulos
//!
//! | Módulo | Formato |
//! |---|---|
//! | [`xbe`] | executáveis `.xbe`: header, certificado, seções, thunk table, logo |
//! | [`xdvdfs`] | filesystem do DVD, em imagens XISO |
//! | [`fatx`] | filesystem do HD e dos memory units |
//! | [`eeprom`] | os 256 bytes de configuração do console |
//! | [`vfs`] | as letras de drive que o jogo vê, sobre os formatos acima |
//! | [`media`] | origem dos bytes: fatia em memória ou arquivo lido sob demanda |
//! | [`fixtures`] | construtores de arquivos de teste, gerados por código |
//!
//! **Estado:** Fase 1 completa. A verificação de conformidade contra arquivos reais
//! está registrada em `docs/compat/formatos.md`.

pub mod bytes;
pub mod eeprom;
pub mod fatx;
pub mod fixtures;
pub mod media;
pub mod vfs;
pub mod xbe;
pub mod xdvdfs;

pub use eeprom::Eeprom;
pub use fatx::Fatx;
pub use media::Media;
pub use vfs::{Mount, Vfs};
pub use xbe::{Xbe, XbeKind};
pub use xdvdfs::{DirEntry, Xdvdfs};

/// Fase do roadmap em que este subsistema é implementado.
pub const ROADMAP_PHASE: &str = "Fase 1";

/// O que um caminho apontado pelo usuário contém.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TipoDeMidia {
    /// Um executável `.xbe` solto, ou uma pasta com `default.xbe`.
    Executavel,
    /// Uma imagem de disco XDVDFS.
    ImagemDeDisco,
    /// Uma partição FATX (HD ou memory unit).
    ParticaoFatx,
}

impl TipoDeMidia {
    /// Nome legível.
    pub const fn nome(self) -> &'static str {
        match self {
            Self::Executavel => "executável .xbe",
            Self::ImagemDeDisco => "imagem de disco XDVDFS",
            Self::ParticaoFatx => "partição FATX",
        }
    }
}

/// Identifica o tipo de mídia olhando os bytes, não a extensão.
///
/// A extensão mente: imagens aparecem como `.iso`, `.xiso`, `.img` ou sem extensão
/// nenhuma. Quem decide é a assinatura.
pub fn identificar(data: &[u8]) -> Option<TipoDeMidia> {
    if data.len() >= 4 && data[..4] == xbe::MAGIC {
        return Some(TipoDeMidia::Executavel);
    }
    if data.len() >= 4 && data[..4] == fatx::MAGIC {
        return Some(TipoDeMidia::ParticaoFatx);
    }
    if Xdvdfs::open(data).is_ok() {
        return Some(TipoDeMidia::ImagemDeDisco);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{TipoDeMidia, identificar};
    use crate::fixtures;

    #[test]
    fn identifica_cada_formato() {
        assert_eq!(
            identificar(&fixtures::xbe_minimo()),
            Some(TipoDeMidia::Executavel)
        );
        assert_eq!(
            identificar(&fixtures::xiso_minimo()),
            Some(TipoDeMidia::ImagemDeDisco)
        );
        assert_eq!(
            identificar(&fixtures::fatx_minima()),
            Some(TipoDeMidia::ParticaoFatx)
        );
    }

    #[test]
    fn nao_identifica_lixo() {
        assert_eq!(identificar(b""), None);
        assert_eq!(identificar(b"MZ\x90\x00um PE comum"), None);
    }
}
