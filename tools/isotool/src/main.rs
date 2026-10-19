//! Lista e extrai imagens XISO (filesystem XDVDFS).

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use xbox_core::{Error, Result};
use xbox_formats::Xdvdfs;
use xbox_formats::media::Media;
use xbox_formats::xbe::Xbe;

/// Lista e extrai imagens XISO/XDVDFS
#[derive(Debug, Parser)]
#[command(name = "isotool", version, about)]
struct Cli {
    /// Imagem de disco.
    imagem: PathBuf,

    /// Base da partição em hexadecimal, quando a detecção automática falhar.
    #[arg(long, value_name = "HEX")]
    base: Option<String>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Mostra o volume e o executável principal (padrão).
    Info,
    /// Lista um diretório.
    Ls {
        /// Caminho dentro da imagem; vazio = raiz.
        caminho: Option<String>,
    },
    /// Lista a imagem inteira.
    Tree,
    /// Extrai um arquivo da imagem.
    Extract {
        /// Caminho dentro da imagem.
        caminho: String,
        /// Arquivo de destino no host.
        destino: PathBuf,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match executar(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("erro: {e}");
            let mut fonte = std::error::Error::source(&e);
            while let Some(f) = fonte {
                eprintln!("  causa: {f}");
                fonte = f.source();
            }
            ExitCode::FAILURE
        }
    }
}

fn executar(cli: &Cli) -> Result<()> {
    // Lê sob demanda: uma imagem de jogo tem gigabytes e `info`/`ls` tocam só alguns
    // setores.
    let midia = Media::open(&cli.imagem)?;
    let tamanho_da_midia = midia.len();

    let fs = match &cli.base {
        Some(texto) => {
            let limpo = texto.trim_start_matches("0x");
            let base = u64::from_str_radix(limpo, 16)
                .map_err(|e| Error::config(format!("--base '{texto}' não é hexadecimal: {e}")))?;
            Xdvdfs::open_at(midia, base)?
        }
        None => Xdvdfs::open(midia)?,
    };

    match cli.command.as_ref().unwrap_or(&Command::Info) {
        Command::Info => info(&fs, &cli.imagem, tamanho_da_midia),
        Command::Ls { caminho } => ls(&fs, caminho.as_deref()),
        Command::Tree => tree(&fs),
        Command::Extract { caminho, destino } => extract(&fs, caminho, destino),
    }
}

fn info(fs: &Xdvdfs<'_>, caminho: &Path, tamanho: u64) -> Result<()> {
    let v = fs.volume();
    println!("{}", caminho.display());
    // Décimos de MiB em aritmética inteira: evita cast para f64 (que perde precisão em
    // imagens grandes) e dá o mesmo resultado legível.
    let decimos_mib = tamanho * 10 / (1024 * 1024);
    println!(
        "  tamanho ................ {tamanho} bytes ({}.{} MiB)",
        decimos_mib / 10,
        decimos_mib % 10
    );
    println!(
        "  base da partição ....... 0x{:X}{}",
        fs.base(),
        if fs.base() == 0 {
            " (imagem cortada)"
        } else {
            " (imagem completa)"
        }
    );
    println!(
        "  diretório raiz ......... setor {} ({} bytes)",
        v.root_sector, v.root_size
    );
    println!("  timestamp .............. 0x{:016X}", v.timestamp);
    println!();

    let (arquivos, diretorios, bytes) =
        fs.walk()?
            .iter()
            .fold((0u32, 0u32, 0u64), |(a, d, b), (_, e)| {
                if e.is_dir() {
                    (a, d + 1, b)
                } else {
                    (a + 1, d, b + u64::from(e.size))
                }
            });
    println!("Conteúdo");
    println!("  arquivos ............... {arquivos} ({bytes} bytes)");
    println!("  diretórios ............. {diretorios}");
    println!();

    match fs.default_xbe() {
        Ok(entrada) => {
            let bytes = fs.read(&entrada)?;
            println!(
                "default.xbe ({} bytes, setor {})",
                entrada.size, entrada.sector
            );
            match Xbe::parse(&bytes) {
                Ok(xbe) => {
                    println!("  título ................. {}", xbe.titulo());
                    println!(
                        "  title id ............... {}",
                        xbe.certificate.title_id_formatado()
                    );
                    println!("  build .................. {}", xbe.kind.nome());
                    println!("  entry point ............ 0x{:08X}", xbe.entry_point);
                }
                Err(e) => println!("  não parseou: {e}"),
            }
        }
        Err(e) => println!("default.xbe: {e}"),
    }
    Ok(())
}

fn ls(fs: &Xdvdfs<'_>, caminho: Option<&str>) -> Result<()> {
    let entradas = match caminho.filter(|c| !c.is_empty() && *c != "/" && *c != "\\") {
        None => fs.root()?,
        Some(c) => {
            let dir = fs.find(c)?;
            fs.list(&dir)?
        }
    };
    for e in entradas {
        let tipo = if e.is_dir() { "dir " } else { "file" };
        println!("{tipo} {:>12}  setor {:>8}  {}", e.size, e.sector, e.name);
    }
    Ok(())
}

fn tree(fs: &Xdvdfs<'_>) -> Result<()> {
    for (caminho, e) in fs.walk()? {
        let profundidade = caminho.matches('/').count();
        let recuo = "  ".repeat(profundidade);
        let nome = caminho.rsplit('/').next().unwrap_or(&caminho);
        if e.is_dir() {
            println!("{recuo}{nome}/");
        } else {
            println!("{recuo}{nome}  ({} bytes)", e.size);
        }
    }
    Ok(())
}

fn extract(fs: &Xdvdfs<'_>, caminho: &str, destino: &Path) -> Result<()> {
    let entrada = fs.find(caminho)?;
    let bytes = fs.read(&entrada)?;
    std::fs::write(destino, &bytes).map_err(|e| Error::io_path(destino, e))?;
    println!("escrito: {} ({} bytes)", destino.display(), bytes.len());
    Ok(())
}
