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
    },
    /// Traduz um .xbe. Instruções ainda não suportadas são erros de tradução.
    Translate {
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
        /// Compila e executa o programa traduzido; não há kernel, vídeo ou áudio.
        #[arg(long)]
        run: bool,
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
    let (bytes, output, run) = match cli.command {
        Action::Demo { out, run } => (xbox_formats::fixtures::xbe_com_codigo(DEMO_CODE)?, out, run),
        Action::Translate { input, out, run } => (std::fs::read(input)?, out, run),
    };
    let program = analysis::analyze(&bytes)?;
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

fn write_project(output: &Path, bytes: &[u8], source: &str) -> Result<(), Box<dyn Error>> {
    if output.exists() && std::fs::read_dir(output)?.next().is_some() {
        return Err(format!("diretório de saída deve estar vazio: {}", output.display()).into());
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let quoted = |p: PathBuf| toml::Value::String(p.to_string_lossy().into_owned()).to_string();
    let recomp = quoted(root.join("crates/xbox-recomp"));
    let memory = quoted(root.join("crates/xbox-mem"));
    let core = quoted(root.join("crates/xbox-core"));
    std::fs::create_dir_all(output.join("src"))?;
    std::fs::write(
        output.join("Cargo.toml"),
        format!(
            "[package]\nname = \"obsidian-native\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n[workspace]\n\n[dependencies]\nxbox-recomp = {{ path = {recomp} }}\nxbox-mem = {{ path = {memory} }}\nxbox-core = {{ path = {core} }}\n"
        ),
    )?;
    std::fs::write(output.join("src/image.xbe"), bytes)?;
    std::fs::write(output.join("src/generated.rs"), source)?;
    std::fs::write(
        output.join("src/main.rs"),
        r#"mod generated;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = xbox_recomp::runtime::Session::load(include_bytes!("image.xbe"))?;
    let exit = generated::run(&mut session.state, &mut session.bus, 1_000_000)?;
    if exit != xbox_recomp::runtime::Exit::Returned {
        return Err("limite de 1000000 blocos atingido; execução interrompida".into());
    }
    println!("Execução nativa concluída: EAX={}, instruções={}", session.state.regs[0], session.state.retired);
    Ok(())
}
"#,
    )?;
    Ok(())
}
