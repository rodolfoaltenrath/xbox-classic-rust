//! Inspeciona executáveis `.xbe`.
//!
//! Aceita um `.xbe`, uma pasta com `default.xbe` dentro, ou uma imagem XISO — neste
//! caso o `default.xbe` é extraído da imagem. É a primeira ferramenta do projeto a
//! fazer trabalho de verdade, e serve como verificação manual dos parsers da Fase 1.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use xbox_core::{Error, Result};
use xbox_formats::Xdvdfs;
use xbox_formats::media::Media;
use xbox_formats::xbe::{Logo, Xbe};

/// Inspeciona executáveis .xbe
#[derive(Debug, Parser)]
#[command(name = "xbetool", version, about)]
struct Cli {
    /// Arquivo .xbe, pasta com default.xbe, ou imagem XISO.
    arquivo: PathBuf,

    /// Desenha o logo do título em ASCII.
    #[arg(long)]
    logo: bool,

    /// Lista os ordinais do kernel que o título importa (a lista de trabalho da Fase 4).
    #[arg(long)]
    thunks: bool,

    /// Escreve os bytes de uma seção em um arquivo: --extrair .text:saida.bin
    #[arg(long, value_name = "SECAO:DESTINO")]
    extrair: Option<String>,
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
    let caminho = resolver_caminho(&cli.arquivo)?;
    let midia = Media::open(&caminho)?;
    let tamanho = usize::try_from(midia.len())
        .map_err(|_| Error::format("arquivo maior que o espaço de endereçamento do host"))?;

    // Imagem de jogo tem gigabytes: identificar e extrair o executável **sem carregar
    // a mídia inteira** é requisito, não otimização.
    let cabecalho = midia.read_at(0, 4.min(tamanho))?;
    let e_xbe = *cabecalho == xbox_formats::xbe::MAGIC;

    let bytes: Vec<u8> = if e_xbe {
        midia.read_at(0, tamanho)?.into_owned()
    } else {
        let fs = Xdvdfs::open(midia)?;
        let entrada = fs.default_xbe()?;
        println!(
            "(extraído de uma imagem XDVDFS: default.xbe, {} bytes, setor {})\n",
            entrada.size, entrada.sector
        );
        fs.read(&entrada)?.into_owned()
    };

    let xbe = Xbe::parse(&bytes)?;
    imprimir_relatorio(&xbe, &caminho, bytes.len());

    if cli.logo {
        imprimir_logo(&xbe);
    }
    if cli.thunks {
        imprimir_thunks(&xbe);
    }
    if let Some(spec) = &cli.extrair {
        extrair_secao(&xbe, spec)?;
    }
    Ok(())
}

/// Lista os ordinais do kernel importados pelo título.
///
/// É o inventário exato do que a Fase 4 precisa implementar para este jogo rodar.
fn imprimir_thunks(xbe: &Xbe<'_>) {
    println!();
    match xbe.kernel_imports() {
        Ok(ordinais) if ordinais.is_empty() => {
            println!("Importações do kernel: nenhuma (o título não usa a thunk table)");
        }
        Ok(ordinais) => {
            println!(
                "Importações do kernel: {} ordinais (thunk table em 0x{:08X})",
                ordinais.len(),
                xbe.kernel_thunk
            );
            for linha in ordinais.chunks(12) {
                let texto: Vec<String> = linha.iter().map(|o| format!("{o:>4}")).collect();
                println!("  {}", texto.join(" "));
            }
        }
        Err(e) => println!("Importações do kernel: não foi possível ler ({e})"),
    }
}

/// Aceita uma pasta e procura `default.xbe` dentro dela.
fn resolver_caminho(entrada: &Path) -> Result<PathBuf> {
    if entrada.is_dir() {
        let candidato = entrada.join("default.xbe");
        if candidato.is_file() {
            return Ok(candidato);
        }
        return Err(Error::format(format!(
            "{} é uma pasta e não contém default.xbe",
            entrada.display()
        )));
    }
    Ok(entrada.to_path_buf())
}

fn imprimir_relatorio(xbe: &Xbe<'_>, caminho: &Path, tamanho: usize) {
    let h = &xbe.header;
    let c = &xbe.certificate;

    println!("{}", caminho.display());
    // "do .xbe" e não "do arquivo": quando a entrada é uma imagem, este número é o
    // tamanho do executável extraído, não o da imagem.
    println!("  tamanho do .xbe ........ {tamanho} bytes");
    println!();

    println!("Título");
    println!("  nome ................... {}", xbe.titulo());
    println!(
        "  title id ............... {} (0x{:08X})",
        c.title_id_formatado(),
        c.title_id
    );
    println!("  versão ................. {}", c.version);
    println!("  disco .................. {}", c.disk_number + 1);
    println!(
        "  regiões ................ {}",
        lista(&c.game_region.nomes())
    );
    println!(
        "  mídias ................. {}",
        lista(&c.allowed_media.nomes())
    );
    println!("  classificação .......... 0x{:08X}", c.game_ratings);
    println!("  tamanho do certificado . 0x{:X}", c.size);
    println!();

    println!("Imagem");
    println!("  tipo de build .......... {}", xbe.kind.nome());
    println!("  base ................... 0x{:08X}", h.base_address);
    println!(
        "  entry point ............ 0x{:08X}  (XOR 0x{:08X} no arquivo)",
        xbe.entry_point, h.entry_point_raw
    );
    println!(
        "  thunk do kernel ........ 0x{:08X}  (XOR 0x{:08X} no arquivo)",
        xbe.kernel_thunk, h.kernel_thunk_raw
    );
    println!("  tamanho da imagem ...... 0x{:X}", h.size_of_image);
    println!("  tamanho dos headers .... 0x{:X}", h.size_of_headers);
    println!(
        "  init flags ............. 0x{:08X} {}",
        h.init_flags.0,
        lista(&h.init_flags.nomes())
    );
    println!(
        "  pilha / heap ........... 0x{:X} / 0x{:X} (reserva 0x{:X})",
        h.pe_stack_commit, h.pe_heap_commit, h.pe_heap_reserve
    );
    if let Some(p) = xbe.debug_pathname() {
        println!("  caminho de debug ....... {p}");
    }
    if let Some(t) = &xbe.tls {
        println!(
            "  tls .................... dados 0x{:08X}..0x{:08X}, índice 0x{:08X}, zero fill {}",
            t.data_start_address, t.data_end_address, t.index_address, t.size_of_zero_fill
        );
    } else {
        println!("  tls .................... nenhum");
    }
    println!();

    println!("Seções ({})", xbe.sections.len());
    println!(
        "  {:<16} {:>4}  {:<10} {:<10} {:<10} {:<10}",
        "nome", "perm", "va", "vsize", "raw", "rsize"
    );
    for s in &xbe.sections {
        println!(
            "  {:<16} {:>4}  0x{:08X} 0x{:08X} 0x{:08X} 0x{:08X}",
            s.name,
            s.flags.rwx(),
            s.virtual_address,
            s.virtual_size,
            s.raw_address,
            s.raw_size
        );
    }
    println!();

    println!("Bibliotecas linkadas no título ({})", xbe.libraries.len());
    if xbe.libraries.is_empty() {
        println!("  (nenhuma declarada)");
    }
    for l in &xbe.libraries {
        println!(
            "  {:<10} {:<12} flags 0x{:04X}",
            l.name,
            l.versao(),
            l.flags
        );
    }
}

fn lista(nomes: &[&str]) -> String {
    if nomes.is_empty() {
        "(nenhuma)".to_owned()
    } else {
        nomes.join(", ")
    }
}

/// Desenha o logo em caracteres, do mais escuro ao mais claro.
///
/// Serve de verificação visual do decodificador de RLE: se o logo sair como ruído ou
/// listras erradas, o problema aparece aqui antes de chegar na interface da Fase 8.
fn imprimir_logo(xbe: &Xbe<'_>) {
    const RAMPA: &[u8] = b" .:-=+*#%@";

    println!();
    let Some(logo) = xbe.logo() else {
        println!("Logo: o título não tem logo.");
        return;
    };
    println!("Logo ({}x{}, 4 bits por pixel)", logo.width, logo.height);
    for y in 0..logo.height {
        let linha: String = (0..logo.width)
            .map(|x| {
                let nivel = usize::from(logo.pixel(x, y)) * (RAMPA.len() - 1) / 15;
                char::from(RAMPA[nivel])
            })
            .collect();
        println!("  {linha}");
    }
    let _ = Logo::LARGURA;
}

fn extrair_secao(xbe: &Xbe<'_>, spec: &str) -> Result<()> {
    let (nome, destino) = spec.split_once(':').ok_or_else(|| {
        Error::config(format!("--extrair espera SECAO:DESTINO, recebeu '{spec}'"))
    })?;
    let (idx, secao) = xbe.section_por_nome(nome).ok_or_else(|| {
        let disponiveis: Vec<&str> = xbe.sections.iter().map(|s| s.name.as_str()).collect();
        Error::config(format!(
            "seção '{nome}' não existe; há: {}",
            disponiveis.join(", ")
        ))
    })?;
    let bytes = xbe.section_data(idx)?;
    std::fs::write(destino, bytes).map_err(|e| Error::io_path(destino, e))?;
    println!();
    println!(
        "escrito: {destino} ({} bytes da seção {})",
        bytes.len(),
        secao.name
    );
    Ok(())
}
