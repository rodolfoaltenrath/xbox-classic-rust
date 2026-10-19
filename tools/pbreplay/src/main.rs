//! Grava e reproduz traces de pushbuffer do NV2A
//!
//! **Estado:** esqueleto. Implementação na **Fase 9** do ROADMAP.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

/// Grava e reproduz traces de pushbuffer do NV2A
#[derive(Debug, Parser)]
#[command(name = "pbreplay", version, about)]
struct Cli {
    /// Arquivo de entrada.
    arquivo: PathBuf,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let err = xbox_core::Error::not_implemented(
        "Grava e reproduz traces de pushbuffer do NV2A",
        "Fase 9",
    );
    eprintln!("erro: {err}");
    eprintln!("  entrada pedida: {}", cli.arquivo.display());
    ExitCode::FAILURE
}
