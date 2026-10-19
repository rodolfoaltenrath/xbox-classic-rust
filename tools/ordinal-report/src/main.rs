//! Relatorio de cobertura de ordinais do kernel HLE
//!
//! **Estado:** esqueleto. Implementação na **Fase 4** do ROADMAP.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

/// Relatorio de cobertura de ordinais do kernel HLE
#[derive(Debug, Parser)]
#[command(name = "ordinal-report", version, about)]
struct Cli {
    /// Arquivo de entrada.
    arquivo: PathBuf,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let err = xbox_core::Error::not_implemented(
        "Relatorio de cobertura de ordinais do kernel HLE",
        "Fase 4",
    );
    eprintln!("erro: {err}");
    eprintln!("  entrada pedida: {}", cli.arquivo.display());
    ExitCode::FAILURE
}
