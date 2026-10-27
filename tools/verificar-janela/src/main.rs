//! Verificação visual real da janela do Obsidian no Linux (PLANO 8.2, ADR-0086).
//!
//! Equivalente do `scripts/verify-window.ps1`: executa o binário gerado com
//! `--janela`, encontra a janela por **outra** conexão X, captura o conteúdo que o
//! servidor tem e envia o que um usuário enviaria. Três modos:
//!
//! - `apos`: espera o orçamento acabar ("janela mantida aberta"), compara a captura
//!   com a exportação `--quadro-apresentado` e fecha;
//! - `durante`: fecha enquanto o guest ainda executa e exige a parada pelo host;
//! - `continuo`: `--continuo` com Espaço/F5/F9 e conferência das linhas do log.
//!
//! Tudo vai para um diretório novo: logs, quadros BGRA, PPM e `resultado.txt`. O
//! status de saída é 0 só quando todos os critérios do modo passaram.
//!
//! Uso: `verificar-janela <apos|durante|continuo> <executavel> <saida-nova> [fatias]`

use std::io::Write;
use std::process::ExitCode;

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
use linux::executar;

/// Fora do Linux não há servidor X para verificar; no Windows o equivalente é o
/// `scripts/verify-window.ps1`.
#[cfg(not(target_os = "linux"))]
fn executar(_args: &[String]) -> Result<(), String> {
    Err("verificação X11 só existe no Linux; no Windows use scripts/verify-window.ps1".into())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match executar(&args) {
        Ok(()) => {
            println!("JANELA OK");
            ExitCode::SUCCESS
        }
        Err(erro) => {
            let _ = writeln!(std::io::stderr(), "JANELA FALHOU: {erro}");
            ExitCode::FAILURE
        }
    }
}
