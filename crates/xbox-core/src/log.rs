//! Inicialização do logging.
//!
//! Usa `tracing` em vez de `log` porque o emulador é multithread (ROADMAP §5.2) e
//! spans são a única forma sã de ler um log em que CPU, GPU e áudio escrevem ao mesmo
//! tempo.
//!
//! Filtragem: a variável de ambiente `OBSIDIAN_LOG` tem precedência (sintaxe do
//! `EnvFilter`, ex.: `OBSIDIAN_LOG=xbox_gpu=trace,xbox_cpu=debug`). Sem ela, o nível
//! vem da verbosidade passada na linha de comando.
//!
//! **Sem cores:** a feature `ansi` do `tracing-subscriber` está desligada (ADR-0008).
//! Além do motivo de toolchain, o modo appliance da Fase 10 não tem terminal — log
//! colorido ali é só poluição no console de boot.

use tracing_subscriber::EnvFilter;

/// Variável de ambiente que controla o filtro de log.
pub const ENV_VAR: &str = "OBSIDIAN_LOG";

/// Inicializa o subscriber global.
///
/// `verbosity` vem da contagem de `-v` na linha de comando: 0 = `info`, 1 = `debug`,
/// 2 ou mais = `trace`.
///
/// Chamar duas vezes é inofensivo: a segunda chamada é ignorada.
pub fn init(verbosity: u8) {
    let filtro = EnvFilter::try_from_env(ENV_VAR)
        .unwrap_or_else(|_| EnvFilter::new(default_filter(verbosity)));

    let resultado = tracing_subscriber::fmt()
        .with_env_filter(filtro)
        .with_target(true)
        .with_thread_names(true)
        .try_init();

    if resultado.is_ok() {
        tracing::debug!(
            verbosity,
            "logging inicializado (sobreponha com {ENV_VAR}=...)"
        );
    }
}

/// Filtro padrão para uma verbosidade.
fn default_filter(verbosity: u8) -> &'static str {
    match verbosity {
        0 => "warn,obsidian=info,xbox_core=info",
        1 => "info,obsidian=debug,xbox_core=debug",
        _ => "debug,obsidian=trace,xbox_core=trace",
    }
}

#[cfg(test)]
mod tests {
    use super::default_filter;

    #[test]
    fn verbosidade_aumenta_o_nivel() {
        assert!(default_filter(0).contains("info"));
        assert!(default_filter(1).contains("debug"));
        assert!(default_filter(5).contains("trace"));
    }
}
