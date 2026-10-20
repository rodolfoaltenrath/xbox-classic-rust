//! Ferramenta experimental de recompilação estática. Ver docs/dev/recomp.md.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use clap::{Parser, Subcommand};
use xbox_recomp::{DEMO_CODE, analysis, emit};

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Recompilação experimental XBE → Rust nativo (subconjunto inteiro x86-32)"
)]
struct Cli {
    #[command(subcommand)]
    command: Action,
}

#[derive(Debug, Subcommand)]
enum Action {
    /// Gera a demonstração aberta: soma, loop, call/ret e escrita na memória.
    Demo {
        #[arg(long)]
        out: PathBuf,
        /// Compila e executa o projeto gerado usando Cargo e rustc.
        #[arg(long)]
        run: bool,
        /// Raiz de código adicional (VA decimal ou 0xHEX); pode ser repetida.
        #[arg(long = "entry", value_parser = parse_address)]
        entries: Vec<u32>,
    },
    /// Traduz um .xbe. Instruções ainda não suportadas são erros de tradução.
    Translate {
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
        /// Compila e executa o programa traduzido; não há kernel, vídeo ou áudio.
        #[arg(long)]
        run: bool,
        /// Callback/alvo adicional que não pôde ser inferido estaticamente.
        #[arg(long = "entry", value_parser = parse_address)]
        entries: Vec<u32>,
    },
}

fn main() -> ExitCode {
    match execute(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("erro: {e}");
            ExitCode::FAILURE
        }
    }
}

fn execute(cli: Cli) -> Result<(), Box<dyn Error>> {
    let (bytes, output, run, entries) = match cli.command {
        Action::Demo { out, run, entries } => (
            xbox_formats::fixtures::xbe_com_codigo(DEMO_CODE)?,
            out,
            run,
            entries,
        ),
        Action::Translate {
            input,
            out,
            run,
            entries,
        } => (std::fs::read(input)?, out, run, entries),
    };
    let program = analysis::analyze_with_entries(&bytes, &entries)?;
    // O loader deve aceitar a imagem antes de escrever artefatos ou compilar.
    let _session = xbox_recomp::runtime::Session::load(&bytes)?;
    write_project(&output, &bytes, &emit::rust_source(&program))?;
    println!(
        "{} instruções alcançáveis, {} blocos Rust → {}",
        program.instructions.len(),
        program.blocks.len(),
        output.display()
    );
    if run {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let status = Command::new(cargo)
            .arg("run")
            .arg("--offline")
            .arg("--manifest-path")
            .arg(output.join("Cargo.toml"))
            .status()?;
        if !status.success() {
            return Err(format!("compilação/execução nativa falhou: {status}").into());
        }
    }
    Ok(())
}

fn parse_address(text: &str) -> Result<u32, String> {
    let result = if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16)
    } else {
        text.parse()
    };
    result.map_err(|_| format!("endereço x86 de 32 bits inválido: {text}"))
}

fn write_project(output: &Path, bytes: &[u8], source: &str) -> Result<(), Box<dyn Error>> {
    if output.exists() && std::fs::read_dir(output)?.next().is_some() {
        return Err(format!("diretório de saída deve estar vazio: {}", output.display()).into());
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let quoted = |p: PathBuf| toml::Value::String(p.to_string_lossy().into_owned()).to_string();
    let recomp = quoted(root.join("crates/xbox-recomp"));
    let memory = quoted(root.join("crates/xbox-mem"));
    let core = quoted(root.join("crates/xbox-core"));
    let kernel = quoted(root.join("crates/xbox-kernel"));
    std::fs::create_dir_all(output.join("src"))?;
    std::fs::write(
        output.join("Cargo.toml"),
        format!(
            "[package]\nname = \"obsidian-native\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n[workspace]\n\n[dependencies]\nxbox-recomp = {{ path = {recomp} }}\nxbox-mem = {{ path = {memory} }}\nxbox-core = {{ path = {core} }}\nxbox-kernel = {{ path = {kernel} }}\n"
        ),
    )?;
    std::fs::write(output.join("src/image.xbe"), bytes)?;
    std::fs::write(output.join("src/generated.rs"), source)?;
    std::fs::write(
        output.join("src/main.rs"),
        r#"mod generated;

use xbox_recomp::runtime::Exit;

/// Orçamento por fatia e número máximo de fatias. O orçamento total e a interrupção
/// pelo host são trabalho da entrega 5; aqui o limite existe para que um serviço
/// faltante não vire laço infinito.
const BLOCOS_POR_FATIA: u64 = 1_000_000;
const FATIAS: u32 = 64;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = xbox_recomp::runtime::Session::load(include_bytes!("image.xbe"))?;
    let mut dispatcher = xbox_kernel::dispatch::Dispatcher::new();

    for _ in 0..FATIAS {
        match generated::run(&mut session.state, &mut session.bus, BLOCOS_POR_FATIA)? {
            Exit::Returned => {
                println!(
                    "Execução nativa concluída: EAX={}, instruções={}, chamadas atendidas={}",
                    session.state.regs[0], session.state.retired, dispatcher.atendidas()
                );
                return Ok(());
            }
            Exit::Budget => continue,
            Exit::Breakpoint { pc } => {
                return Err(format!(
                    "exceção guest #BP (vetor 3) por INT3 em 0x{pc:08X}; próximo EIP=0x{:08X}; entrega de exceções ainda não implementada",
                    session.state.eip
                ).into());
            }
            Exit::Halted { pc, interrupts_enabled } => {
                return Err(format!(
                    "CPU guest suspensa por HLT em 0x{pc:08X}; próximo EIP=0x{:08X}, IF={}; entrega de interrupções ainda não implementada",
                    session.state.eip, u8::from(interrupts_enabled)
                ).into());
            }
            // O erro do dispatcher identifica nome, ordinal, chamador e argumentos.
            // A CPU fica intacta: o estado pendente continua retomável.
            Exit::KernelCall { .. } => session.atender_kernel(&mut dispatcher)?,
        }
    }
    Err(format!(
        "execução interrompida após {} blocos; {} chamadas atendidas, {} recusadas",
        BLOCOS_POR_FATIA * u64::from(FATIAS),
        dispatcher.atendidas(),
        dispatcher.recusadas()
    )
    .into())
}
"#,
    )?;
    Ok(())
}
