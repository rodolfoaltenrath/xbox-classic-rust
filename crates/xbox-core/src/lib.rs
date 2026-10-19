//! Tipos e serviços compartilhados por todos os subsistemas do emulador.
//!
//! Este crate é deliberadamente pequeno e sem dependência de nenhum outro crate do
//! projeto: ele é a raiz do grafo. Nada aqui sabe o que é um XBE, uma GPU ou um
//! sistema operacional.
//!
//! Conteúdo:
//!
//! - [`error`] — tipos de erro compartilhados.
//! - [`snapshot`] — serialização determinística de estado emulado (ROADMAP §4.4).
//! - [`time`] — relógio virtual em ciclos do guest e agendador de eventos.
//! - [`config`] — configuração do emulador (TOML).
//! - [`log`] — inicialização do `tracing`.
//!
//! **Estado:** implementado na Fase 0 do [ROADMAP](../../../ROADMAP.md).

pub mod config;
pub mod error;
pub mod log;
pub mod snapshot;
pub mod time;

pub use config::Config;
pub use error::{Error, Result, SnapshotError};
pub use snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
pub use time::{Clock, Cycles, EventKind, Scheduler};

/// Versão do emulador, vinda do `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Quantidade de RAM unificada do console emulado, em bytes.
///
/// O Xbox retail tem 64 MiB. Kits de desenvolvimento tinham 128 MiB, e alguns
/// homebrews exigem isso — daí a feature `ram128`.
pub const RAM_SIZE: usize = if cfg!(feature = "ram128") {
    128 * 1024 * 1024
} else {
    64 * 1024 * 1024
};

/// Tamanho de página do guest (x86-32, páginas pequenas).
pub const PAGE_SIZE: usize = 4096;

/// Tamanho de página grande do guest (PSE de 4 MiB).
pub const LARGE_PAGE_SIZE: usize = 4 * 1024 * 1024;

/// Descreve o build atual, para logs e para a tela "sobre" da UI.
#[derive(Debug, Clone, Copy)]
pub struct BuildInfo {
    /// Versão do pacote.
    pub version: &'static str,
    /// Sistema operacional do host, conforme `std::env::consts::OS`.
    pub host_os: &'static str,
    /// Arquitetura do host, conforme `std::env::consts::ARCH`.
    pub host_arch: &'static str,
    /// Perfil de compilação: `debug` ou `release`.
    pub profile: &'static str,
    /// RAM emulada, em MiB.
    pub emulated_ram_mib: usize,
}

impl BuildInfo {
    /// Coleta as informações do build atual.
    pub const fn current() -> Self {
        Self {
            version: VERSION,
            host_os: std::env::consts::OS,
            host_arch: std::env::consts::ARCH,
            profile: if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            },
            emulated_ram_mib: RAM_SIZE / (1024 * 1024),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{LARGE_PAGE_SIZE, PAGE_SIZE, RAM_SIZE};

    #[test]
    fn ram_e_multiplo_de_pagina() {
        assert_eq!(RAM_SIZE % PAGE_SIZE, 0);
        assert_eq!(RAM_SIZE % LARGE_PAGE_SIZE, 0);
    }

    #[test]
    fn ram_padrao_e_64_mib() {
        if cfg!(feature = "ram128") {
            assert_eq!(RAM_SIZE, 128 * 1024 * 1024);
        } else {
            assert_eq!(RAM_SIZE, 64 * 1024 * 1024);
        }
    }
}
