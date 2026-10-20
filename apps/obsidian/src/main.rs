//! Ponto de entrada do emulador.
//!
//! Na Fase 0 o binário existe, inicializa logging e configuração, e sabe dizer o que
//! ainda não faz — apontando a fase do ROADMAP responsável por cada caminho. Nenhum
//! `todo!()`: um caminho não implementado devolve [`xbox_core::Error::NotImplemented`]
//! com mensagem útil.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use xbox_core::{BuildInfo, Config, Error, Result};
use xbox_formats::{Fatx, Media, Mount, TipoDeMidia, Vfs, Xbe, Xdvdfs};

mod memory;

/// Emulador de Xbox clássico com interface Big Picture.
#[derive(Debug, Parser)]
#[command(name = "obsidian", version, about, long_about = None)]
struct Cli {
    /// Arquivo de configuração (padrão: obsidian.toml ao lado do executável).
    #[arg(short, long, global = true, value_name = "ARQUIVO")]
    config: Option<PathBuf>,

    /// Aumenta a verbosidade do log (-v, -vv).
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    verbose: u8,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Abre a interface Big Picture (comportamento padrão sem subcomando).
    Ui,
    /// Executa um jogo diretamente, sem passar pela interface.
    Run {
        /// Caminho de um .xbe, de uma imagem XISO ou de uma pasta extraída.
        jogo: PathBuf,
    },
    /// Mostra informações de um .xbe ou de uma imagem de disco.
    Info {
        /// Arquivo a inspecionar.
        arquivo: PathBuf,
    },
    /// Relata o ambiente do host e o estado de cada subsistema.
    Doctor,
    /// Valida seções, imports e alias físico pelo bus, sem executar código.
    CheckMemory { arquivo: PathBuf },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    xbox_core::log::init(cli.verbose);

    match executar(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            // Erro vai para stderr em linguagem humana, não como Debug de struct.
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
    let caminho_config = cli
        .config
        .clone()
        .unwrap_or_else(|| PathBuf::from(xbox_core::config::CONFIG_FILE_NAME));
    let config = Config::load(&caminho_config)?;
    tracing::debug!(config = ?caminho_config, "configuração carregada");

    match cli.command.as_ref().unwrap_or(&Command::Ui) {
        Command::Ui => abrir_ui(&config),
        Command::Run { jogo } => rodar_jogo(&config, jogo),
        Command::Info { arquivo } => mostrar_info(arquivo),
        Command::CheckMemory { arquivo } => memory::check(arquivo),
        Command::Doctor => {
            doctor(&config, &caminho_config);
            Ok(())
        }
    }
}

fn abrir_ui(_config: &Config) -> Result<()> {
    Err(Error::not_implemented(
        "abrir a interface Big Picture",
        "Fase 8",
    ))
}

/// Prepara tudo o que a Fase 4 vai precisar e relata o que falta.
///
/// A mídia é aberta, o executável é lido, os drives são montados e os ordinais do
/// kernel são contados. O que não existe ainda é a execução — e o erro diz isso com o
/// contexto todo em vez de uma mensagem seca.
fn rodar_jogo(config: &Config, jogo: &std::path::Path) -> Result<()> {
    let (vfs, xbe_bytes) = montar_sessao(config, jogo)?;
    let xbe = Xbe::parse(&xbe_bytes)?;
    let ordinais = xbe.kernel_imports().map_or(0, |o| o.len());

    println!("Pronto para executar:");
    println!("  título ................. {}", xbe.titulo());
    println!("  entry point ............ 0x{:08X}", xbe.entry_point);
    println!("  seções a mapear ........ {}", xbe.sections.len());
    println!("  ordinais do kernel ..... {ordinais}");
    for (letra, origem) in vfs.montagens() {
        println!("  {letra}: ..................... {origem}");
    }
    println!();

    Err(Error::not_implemented(
        "executar o código do título (CPU, kernel HLE, GPU)",
        "Fases 3 a 5",
    ))
}

/// Monta a sessão de um jogo: VFS com os drives do console e os bytes do executável.
fn montar_sessao<'a>(config: &Config, jogo: &'a std::path::Path) -> Result<(Vfs<'a>, Vec<u8>)> {
    if !jogo.exists() {
        return Err(Error::io_path(
            jogo,
            std::io::Error::new(std::io::ErrorKind::NotFound, "caminho não encontrado"),
        ));
    }

    let mut vfs = Vfs::new();
    let xbe_bytes = if jogo.is_dir() {
        // Pasta extraída: o próprio diretório é o `D:`.
        vfs.montar('D', Mount::Pasta(jogo.to_path_buf()))?;
        vfs.ler("D:\\default.xbe")?
    } else {
        let midia = Media::open(jogo)?;
        let cabecalho = midia.read_at(0, 4.min(midia.len() as usize))?;
        if *cabecalho == xbox_formats::xbe::MAGIC {
            // `.xbe` solto: `D:` é a pasta que o contém.
            let pasta = jogo.parent().unwrap_or(std::path::Path::new("."));
            vfs.montar('D', Mount::Pasta(pasta.to_path_buf()))?;
            midia.read_at(0, midia.len() as usize)?.into_owned()
        } else {
            let disco = Xdvdfs::open(midia)?;
            let entrada = disco.default_xbe()?;
            let bytes = disco.read(&entrada)?.into_owned();
            vfs.montar('D', Mount::Disco(disco))?;
            bytes
        }
    };

    // Drives graváveis do título: pasta do host, por decisão de projeto (ver
    // `xbox_formats::vfs`). Ficam debaixo de `data_dir`, ou ao lado do executável.
    let dados = config
        .paths
        .data_dir
        .clone()
        .unwrap_or_else(|| std::path::PathBuf::from("dados"));
    for (letra, sub) in [('T', "tdata"), ('U', "udata"), ('Z', "cache")] {
        let caminho = dados.join(sub);
        std::fs::create_dir_all(&caminho).map_err(|e| Error::io_path(&caminho, e))?;
        vfs.montar(letra, Mount::Pasta(caminho))?;
    }

    Ok((vfs, xbe_bytes))
}

fn mostrar_info(arquivo: &std::path::Path) -> Result<()> {
    if !arquivo.exists() {
        return Err(Error::io_path(
            arquivo,
            std::io::Error::new(std::io::ErrorKind::NotFound, "caminho não encontrado"),
        ));
    }

    // Pasta: descreve o `default.xbe` de dentro dela.
    let caminho = if arquivo.is_dir() {
        arquivo.join("default.xbe")
    } else {
        arquivo.to_path_buf()
    };
    if !caminho.exists() {
        return Err(Error::format(format!(
            "{} é uma pasta e não contém default.xbe",
            arquivo.display()
        )));
    }

    let midia = Media::open(&caminho)?;
    let tamanho = usize::try_from(midia.len())
        .map_err(|_| Error::format("arquivo maior que o espaço de endereçamento"))?;
    let cabecalho = midia.read_at(0, 4.min(tamanho))?;

    println!("{}", caminho.display());
    println!("  tamanho ................ {} bytes", midia.len());

    let bytes = if *cabecalho == xbox_formats::xbe::MAGIC {
        println!(
            "  tipo ................... {}",
            TipoDeMidia::Executavel.nome()
        );
        midia.read_at(0, tamanho)?.into_owned()
    } else if *cabecalho == xbox_formats::fatx::MAGIC {
        println!(
            "  tipo ................... {}",
            TipoDeMidia::ParticaoFatx.nome()
        );
        let fs = Fatx::open(midia)?;
        println!(
            "  cluster ................ {} bytes",
            fs.superbloco().cluster_bytes()
        );
        println!("  clusters ............... {}", fs.clusters());
        let itens = fs.walk()?;
        println!("  conteúdo ............... {} itens", itens.len());
        for (caminho, e) in itens.iter().take(20) {
            println!("    {caminho}{}", if e.is_dir() { "/" } else { "" });
        }
        return Ok(());
    } else {
        let disco = Xdvdfs::open(midia)?;
        println!(
            "  tipo ................... {}",
            TipoDeMidia::ImagemDeDisco.nome()
        );
        println!("  base da partição ....... 0x{:X}", disco.base());
        let itens = disco.walk()?;
        let arquivos = itens.iter().filter(|(_, e)| !e.is_dir()).count();
        println!(
            "  conteúdo ............... {arquivos} arquivos, {} diretórios",
            itens.len() - arquivos
        );
        let entrada = disco.default_xbe()?;
        disco.read(&entrada)?.into_owned()
    };

    let xbe = Xbe::parse(&bytes)?;
    println!();
    println!("  título ................. {}", xbe.titulo());
    println!(
        "  title id ............... {}",
        xbe.certificate.title_id_formatado()
    );
    println!("  build .................. {}", xbe.kind.nome());
    println!("  versão ................. {}", xbe.certificate.version);
    println!(
        "  regiões ................ {}",
        xbe.certificate.game_region.nomes().join(", ")
    );
    println!("  entry point ............ 0x{:08X}", xbe.entry_point);
    println!("  seções ................. {}", xbe.sections.len());
    println!("  bibliotecas ............ {}", xbe.libraries.len());
    match xbe.kernel_imports() {
        Ok(o) => println!("  ordinais do kernel ..... {}", o.len()),
        Err(e) => println!("  ordinais do kernel ..... não foi possível ler ({e})"),
    }
    println!();
    println!("Use `xbetool` para o relatório completo.");
    Ok(())
}

/// Subsistema e a fase do roadmap que o implementa.
///
/// As fases vêm de cada crate (`ROADMAP_PHASE`), então esta tabela não pode
/// dessincronizar do código — se um crate muda de fase, aqui muda junto.
const SUBSISTEMAS: &[(&str, &str)] = &[
    ("formatos (XBE/XISO/FATX)", xbox_formats::ROADMAP_PHASE),
    ("memória e bus", xbox_mem::ROADMAP_PHASE),
    ("CPU x86-32", xbox_cpu::ROADMAP_PHASE),
    ("kernel HLE", xbox_kernel::ROADMAP_PHASE),
    ("GPU NV2A", xbox_gpu::ROADMAP_PHASE),
    ("áudio APU", xbox_audio::ROADMAP_PHASE),
    ("controles", xbox_input::ROADMAP_PHASE),
    ("backends de SO", xbox_platform::ROADMAP_PHASE),
    ("interface Big Picture", xbox_ui::ROADMAP_PHASE),
    ("ferramentas de dev", xbox_dbg::ROADMAP_PHASE),
];

fn doctor(config: &Config, caminho_config: &std::path::Path) {
    let b = BuildInfo::current();
    println!(
        "obsidian {}  ({} em {}/{})",
        b.version, b.profile, b.host_os, b.host_arch
    );
    println!();

    println!("Host");
    let cpus = std::thread::available_parallelism()
        .map_or_else(|_| "desconhecido".to_owned(), |n| n.get().to_string());
    println!("  núcleos lógicos ........ {cpus}");
    println!("  config ................. {}", caminho_config.display());
    println!(
        "  config existe .......... {}",
        if caminho_config.exists() {
            "sim"
        } else {
            "não (usando padrões)"
        }
    );
    println!();

    println!("Console emulado");
    println!("  RAM .................... {} MiB", b.emulated_ram_mib);
    println!("  região ................. {:?}", config.system.region);
    println!("  backend de CPU ......... {:?}", config.system.cpu_backend);
    println!("  sincronização .......... {:?}", config.video.sync);
    println!();

    println!("Features deste build");
    for (nome, ativa) in [
        ("jit", cfg!(feature = "jit")),
        ("hv", cfg!(feature = "hv")),
        ("ram128", cfg!(feature = "ram128")),
        ("kms", cfg!(feature = "kms")),
        ("vk-direct", cfg!(feature = "vk-direct")),
        ("trace-verbose", cfg!(feature = "trace-verbose")),
        ("dbg-gdb", cfg!(feature = "dbg-gdb")),
    ] {
        println!(
            "  {nome:.<24} {}",
            if ativa { "ligada" } else { "desligada" }
        );
    }
    println!();

    println!("Subsistemas (fase do ROADMAP que implementa cada um)");
    for (nome, fase) in SUBSISTEMAS {
        println!("  {nome:.<26} {fase}");
    }
}

#[cfg(test)]
mod tests {
    use super::{Cli, SUBSISTEMAS};
    use clap::{CommandFactory, Parser};

    #[test]
    fn cli_e_coerente() {
        Cli::command().debug_assert();
    }

    #[test]
    fn sem_subcomando_abre_a_ui() {
        let cli = Cli::parse_from(["obsidian"]);
        assert!(cli.command.is_none());
    }

    #[test]
    fn verbosidade_acumula() {
        let cli = Cli::parse_from(["obsidian", "-vv", "doctor"]);
        assert_eq!(cli.verbose, 2);
    }

    #[test]
    fn todos_os_subsistemas_declaram_fase() {
        assert_eq!(SUBSISTEMAS.len(), 10);
        for (nome, fase) in SUBSISTEMAS {
            assert!(
                fase.starts_with("Fase"),
                "{nome} declara fase estranha: {fase}"
            );
        }
    }
}
