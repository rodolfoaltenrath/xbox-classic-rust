//! Tipos de erro compartilhados.
//!
//! Regra do projeto (ROADMAP §13.3): nenhum `unwrap()`/`expect()` em caminho que
//! processa dado do usuário — XBE, ISO, snapshot ou arquivo de configuração. Dado
//! malformado é **erro tipado**, nunca pânico.

use std::path::{Path, PathBuf};

/// `Result` padrão do projeto.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Erro de qualquer subsistema do emulador.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Falha de E/S com o caminho envolvido — preferir esta variante à `Io`.
    #[error("erro de E/S em {path}: {source}")]
    IoPath {
        /// Caminho que causou a falha.
        path: PathBuf,
        /// Erro original.
        #[source]
        source: std::io::Error,
    },

    /// Falha de E/S sem caminho associado.
    #[error("erro de E/S: {0}")]
    Io(#[from] std::io::Error),

    /// Configuração ausente, malformada ou com valor inválido.
    #[error("configuração inválida: {0}")]
    Config(String),

    /// Arquivo do usuário com formato inválido (XBE, XISO, FATX, EEPROM).
    #[error("formato inválido: {0}")]
    Format(String),

    /// Falha ao salvar ou restaurar um snapshot.
    #[error("snapshot: {0}")]
    Snapshot(#[from] SnapshotError),

    /// O host não oferece um recurso necessário (ex.: Vulkan, VT-x).
    #[error("recurso indisponível no host: {0}")]
    HostUnsupported(&'static str),

    /// Funcionalidade prevista no roadmap mas ainda não escrita.
    ///
    /// Existe para que um caminho não implementado falhe com uma mensagem útil
    /// (dizendo em que fase ele chega) em vez de `todo!()` ou de silêncio.
    #[error("ainda não implementado ({phase}): {what}")]
    NotImplemented {
        /// O que foi tentado.
        what: &'static str,
        /// Fase do roadmap em que isso é implementado.
        phase: &'static str,
    },
}

impl Error {
    /// Erro de E/S com caminho.
    pub fn io_path(path: impl AsRef<Path>, source: std::io::Error) -> Self {
        Self::IoPath {
            path: path.as_ref().to_path_buf(),
            source,
        }
    }

    /// Erro de configuração.
    pub fn config(msg: impl Into<String>) -> Self {
        Self::Config(msg.into())
    }

    /// Erro de formato de arquivo.
    pub fn format(msg: impl Into<String>) -> Self {
        Self::Format(msg.into())
    }

    /// Caminho não implementado, apontando a fase do roadmap.
    pub const fn not_implemented(what: &'static str, phase: &'static str) -> Self {
        Self::NotImplemented { what, phase }
    }
}

/// Falhas específicas de leitura/escrita de snapshot.
#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    /// Os bytes não começam com a assinatura esperada.
    #[error("assinatura de snapshot inválida")]
    BadMagic,

    /// Formato de container de versão desconhecida.
    #[error("formato de snapshot v{found} não suportado (este build escreve v{expected})")]
    BadFormatVersion {
        /// Versão encontrada no arquivo.
        found: u32,
        /// Versão que este build entende.
        expected: u32,
    },

    /// A seção lida não é a esperada — indica snapshot de outra build ou corrompido.
    #[error("seção esperada '{expected}', encontrada '{found}'")]
    BadSection {
        /// Tag esperada.
        expected: &'static str,
        /// Tag encontrada (escapada).
        found: String,
    },

    /// Seção escrita por uma build mais nova.
    #[error("seção '{section}' está na v{found}, este build suporta até v{max}")]
    SectionTooNew {
        /// Tag da seção.
        section: &'static str,
        /// Versão encontrada.
        found: u32,
        /// Versão máxima suportada.
        max: u32,
    },

    /// Fim inesperado dos dados.
    #[error("snapshot truncado: faltam {needed} bytes na posição {pos}")]
    Truncated {
        /// Posição de leitura.
        pos: usize,
        /// Bytes que faltaram.
        needed: usize,
    },

    /// Valor fora do domínio válido (ex.: tag de enum desconhecida).
    #[error("valor inválido na seção '{section}': {what}")]
    BadValue {
        /// Tag da seção.
        section: &'static str,
        /// Descrição do valor inválido.
        what: &'static str,
    },
}

#[cfg(test)]
mod tests {
    use super::Error;

    #[test]
    fn mensagem_de_nao_implementado_cita_a_fase() {
        let e = Error::not_implemented("executar um jogo", "Fase 4");
        let msg = e.to_string();
        assert!(msg.contains("Fase 4"), "mensagem foi: {msg}");
        assert!(msg.contains("executar um jogo"), "mensagem foi: {msg}");
    }

    #[test]
    fn erro_de_io_preserva_o_caminho() {
        let io = std::io::Error::new(std::io::ErrorKind::NotFound, "nada aqui");
        let e = Error::io_path("C:/jogos/default.xbe", io);
        assert!(e.to_string().contains("default.xbe"));
    }
}
