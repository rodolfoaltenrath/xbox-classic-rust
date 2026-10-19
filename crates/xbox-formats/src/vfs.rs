//! Sistema de arquivos virtual: as letras de drive que o jogo vê.
//!
//! O título do Xbox não sabe o que é um XISO nem um FATX: ele abre `D:\default.xbe`,
//! `T:\save.bin`, `Z:\cache.dat`. Esta camada é quem transforma isso em leitura numa
//! imagem de disco, numa partição FATX ou numa pasta do host — e é a interface que o
//! `NtCreateFile`/`NtReadFile` do kernel HLE (Fase 4) vai consumir.
//!
//! ## As letras do console
//!
//! | Letra | Conteúdo | Como montamos |
//! |---|---|---|
//! | `D:` | disco do jogo | imagem XDVDFS ou pasta do host com `default.xbe` |
//! | `T:` | dados persistentes do título (saves) | **pasta do host** |
//! | `U:` | dados de usuário do título | **pasta do host** |
//! | `Z:` | cache do título | **pasta do host** |
//! | `C:` | partição de sistema | FATX ou pasta do host |
//! | `E:` | partição de dados | FATX ou pasta do host |
//! | `X:`, `Y:` | caches do sistema | FATX ou pasta do host |
//!
//! Partições graváveis são **pasta do host** por decisão de projeto: escrever num FATX
//! emulado exigiria alocação de clusters, atualização de FAT e tolerância a falha no
//! meio da escrita, e o ganho seria nenhum — o jogo não sabe a diferença, e um save
//! virado arquivo no disco do usuário é mais fácil de copiar e de recuperar.
//!
//! ## Caminhos de dispositivo
//!
//! O kernel também recebe caminhos na forma `\Device\CdRom0\default.xbe`. Essas formas
//! são traduzidas para a letra correspondente — ver [`ALIASES_DE_DISPOSITIVO`].
//!
//! ## Travessia de diretório
//!
//! Um título pode pedir `T:\..\..\..\Windows\System32\config`. Em montagem de pasta do
//! host isso sairia da raiz montada e leria arquivo do usuário. A resolução **recusa**
//! qualquer componente `..`, recusa componentes com `:` e, por cinto e suspensório,
//! confere depois que o caminho final continua debaixo da raiz. Está coberto por teste.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use xbox_core::{Error, Result};

use crate::fatx::Fatx;
use crate::xdvdfs::Xdvdfs;

/// Apelidos de dispositivo que o kernel usa, em minúsculas, e a letra correspondente.
///
/// ⚠️ A correspondência entre partição do HD e letra vem de documentação pública e está
/// na lista de verificação de `docs/compat/formatos.md`.
pub const ALIASES_DE_DISPOSITIVO: [(&str, char); 7] = [
    ("\\device\\cdrom0", 'D'),
    ("\\device\\harddisk0\\partition1", 'E'),
    ("\\device\\harddisk0\\partition2", 'C'),
    ("\\device\\harddisk0\\partition3", 'X'),
    ("\\device\\harddisk0\\partition4", 'Y'),
    ("\\device\\harddisk0\\partition5", 'Z'),
    ("\\device\\harddisk0\\partition6", 'F'),
];

/// De onde vem o conteúdo de um drive.
#[derive(Debug)]
pub enum Mount<'a> {
    /// Imagem de disco de jogo.
    Disco(Xdvdfs<'a>),
    /// Partição FATX (somente leitura nesta fase).
    Particao(Fatx<'a>),
    /// Pasta do host.
    Pasta(PathBuf),
}

impl Mount<'_> {
    /// Descrição curta, para log e para a tela de diagnóstico.
    pub fn descricao(&self) -> String {
        match self {
            Self::Disco(fs) => format!("imagem XDVDFS (base 0x{:X})", fs.base()),
            Self::Particao(fs) => format!("partição FATX ({} bytes)", fs.tamanho()),
            Self::Pasta(p) => format!("pasta do host ({})", p.display()),
        }
    }

    /// A montagem aceita escrita?
    pub const fn gravavel(&self) -> bool {
        matches!(self, Self::Pasta(_))
    }
}

/// Metadados de um arquivo ou diretório.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileInfo {
    /// Nome, sem caminho.
    pub nome: String,
    /// Tamanho em bytes (0 para diretório).
    pub tamanho: u64,
    /// É diretório?
    pub diretorio: bool,
}

/// Conjunto de drives montados.
#[derive(Debug, Default)]
pub struct Vfs<'a> {
    montagens: BTreeMap<char, Mount<'a>>,
}

impl<'a> Vfs<'a> {
    /// VFS vazia.
    pub fn new() -> Self {
        Self {
            montagens: BTreeMap::new(),
        }
    }

    /// Monta um drive. Substitui o que estiver na letra.
    pub fn montar(&mut self, letra: char, mount: Mount<'a>) -> Result<()> {
        let letra = normalizar_letra(letra)?;
        if let Mount::Pasta(raiz) = &mount
            && !raiz.is_dir()
        {
            return Err(Error::format(format!(
                "'{letra}:' aponta para {}, que não é uma pasta",
                raiz.display()
            )));
        }
        tracing::debug!(drive = %letra, origem = %mount.descricao(), "drive montado");
        self.montagens.insert(letra, mount);
        Ok(())
    }

    /// Desmonta um drive, devolvendo o que estava lá.
    pub fn desmontar(&mut self, letra: char) -> Option<Mount<'a>> {
        self.montagens.remove(&normalizar_letra(letra).ok()?)
    }

    /// O drive está montado?
    pub fn montado(&self, letra: char) -> bool {
        normalizar_letra(letra).is_ok_and(|l| self.montagens.contains_key(&l))
    }

    /// Letras montadas, em ordem.
    pub fn letras(&self) -> Vec<char> {
        self.montagens.keys().copied().collect()
    }

    /// Descrição de cada montagem, para diagnóstico.
    pub fn montagens(&self) -> Vec<(char, String)> {
        self.montagens
            .iter()
            .map(|(l, m)| (*l, m.descricao()))
            .collect()
    }

    /// Separa um caminho do guest em letra de drive e caminho relativo.
    ///
    /// Aceita `D:\x`, `D:/x`, `d:x` e as formas de dispositivo de
    /// [`ALIASES_DE_DISPOSITIVO`].
    pub fn resolver(caminho: &str) -> Result<(char, String)> {
        let limpo = caminho.trim();
        if limpo.is_empty() {
            return Err(Error::format("caminho vazio"));
        }

        // Forma `\??\D:\x`, que o kernel também usa.
        let limpo = limpo.strip_prefix("\\??\\").unwrap_or(limpo);

        // Forma de dispositivo.
        let minusculo = limpo.to_ascii_lowercase().replace('/', "\\");
        for (alias, letra) in ALIASES_DE_DISPOSITIVO {
            if let Some(resto) = minusculo.strip_prefix(alias)
                && (resto.is_empty() || resto.starts_with('\\'))
            {
                let inicio = alias.len();
                return Ok((
                    letra,
                    limpo[inicio..].trim_start_matches(['\\', '/']).to_owned(),
                ));
            }
        }

        // Forma `X:...`
        let mut chars = limpo.chars();
        let letra = chars.next().ok_or_else(|| Error::format("caminho vazio"))?;
        if chars.next() != Some(':') {
            return Err(Error::format(format!(
                "'{caminho}' não começa com letra de drive nem com dispositivo conhecido"
            )));
        }
        let resto: String = chars.collect();
        Ok((
            normalizar_letra(letra)?,
            resto.trim_start_matches(['\\', '/']).to_owned(),
        ))
    }

    /// Metadados de um caminho.
    pub fn info(&self, caminho: &str) -> Result<FileInfo> {
        let (letra, relativo) = Self::resolver(caminho)?;
        let mount = self.mount(letra)?;
        if relativo.is_empty() {
            return Ok(FileInfo {
                nome: format!("{letra}:"),
                tamanho: 0,
                diretorio: true,
            });
        }
        match mount {
            Mount::Disco(fs) => {
                let e = fs.find(&relativo)?;
                Ok(FileInfo {
                    nome: e.name.clone(),
                    tamanho: u64::from(e.size),
                    diretorio: e.is_dir(),
                })
            }
            Mount::Particao(fs) => {
                let e = fs.find(&relativo)?;
                Ok(FileInfo {
                    nome: e.name.clone(),
                    tamanho: u64::from(e.size),
                    diretorio: e.is_dir(),
                })
            }
            Mount::Pasta(raiz) => {
                let alvo = caminho_no_host(raiz, &relativo)?;
                let meta = std::fs::metadata(&alvo).map_err(|e| Error::io_path(&alvo, e))?;
                Ok(FileInfo {
                    nome: nome_final(&relativo),
                    tamanho: meta.len(),
                    diretorio: meta.is_dir(),
                })
            }
        }
    }

    /// Lê um arquivo inteiro.
    pub fn ler(&self, caminho: &str) -> Result<Vec<u8>> {
        let info = self.info(caminho)?;
        if info.diretorio {
            return Err(Error::format(format!("'{caminho}' é diretório")));
        }
        let tamanho = usize::try_from(info.tamanho)
            .map_err(|_| Error::format("arquivo maior que o espaço de endereçamento"))?;
        self.ler_faixa(caminho, 0, tamanho)
    }

    /// Lê um pedaço de um arquivo.
    ///
    /// É esta a forma que o `NtReadFile` usa: o jogo lê blocos durante a gameplay, e
    /// carregar um arquivo de 55 MB inteiro para devolver 2 KB seria absurdo.
    pub fn ler_faixa(&self, caminho: &str, off: u64, len: usize) -> Result<Vec<u8>> {
        let (letra, relativo) = Self::resolver(caminho)?;
        match self.mount(letra)? {
            Mount::Disco(fs) => {
                let e = fs.find(&relativo)?;
                Ok(fs.read_range(&e, off, len)?.into_owned())
            }
            Mount::Particao(fs) => {
                let e = fs.find(&relativo)?;
                fs.read_range(&e, off, len)
            }
            Mount::Pasta(raiz) => {
                use std::io::{Read, Seek, SeekFrom};
                let alvo = caminho_no_host(raiz, &relativo)?;
                let mut f = std::fs::File::open(&alvo).map_err(|e| Error::io_path(&alvo, e))?;
                f.seek(SeekFrom::Start(off))
                    .map_err(|e| Error::io_path(&alvo, e))?;
                let mut buf = vec![0u8; len];
                f.read_exact(&mut buf)
                    .map_err(|e| Error::io_path(&alvo, e))?;
                Ok(buf)
            }
        }
    }

    /// Lista um diretório.
    pub fn listar(&self, caminho: &str) -> Result<Vec<FileInfo>> {
        let (letra, relativo) = Self::resolver(caminho)?;
        let mut saida = match self.mount(letra)? {
            Mount::Disco(fs) => {
                let entradas = if relativo.is_empty() {
                    fs.root()?
                } else {
                    let dir = fs.find(&relativo)?;
                    fs.list(&dir)?
                };
                entradas
                    .into_iter()
                    .map(|e| FileInfo {
                        diretorio: e.is_dir(),
                        nome: e.name,
                        tamanho: u64::from(e.size),
                    })
                    .collect()
            }
            Mount::Particao(fs) => {
                let entradas = if relativo.is_empty() {
                    fs.root()?
                } else {
                    let dir = fs.find(&relativo)?;
                    fs.list(&dir)?
                };
                entradas
                    .into_iter()
                    .map(|e| FileInfo {
                        diretorio: e.is_dir(),
                        nome: e.name,
                        tamanho: u64::from(e.size),
                    })
                    .collect()
            }
            Mount::Pasta(raiz) => {
                let alvo = caminho_no_host(raiz, &relativo)?;
                let mut v = Vec::new();
                for entrada in std::fs::read_dir(&alvo).map_err(|e| Error::io_path(&alvo, e))? {
                    let entrada = entrada.map_err(|e| Error::io_path(&alvo, e))?;
                    let meta = entrada.metadata().map_err(|e| Error::io_path(&alvo, e))?;
                    v.push(FileInfo {
                        nome: entrada.file_name().to_string_lossy().into_owned(),
                        tamanho: meta.len(),
                        diretorio: meta.is_dir(),
                    });
                }
                v
            }
        };
        saida.sort_by(|a, b| {
            a.nome
                .to_ascii_lowercase()
                .cmp(&b.nome.to_ascii_lowercase())
        });
        Ok(saida)
    }

    /// O caminho existe?
    pub fn existe(&self, caminho: &str) -> bool {
        self.info(caminho).is_ok()
    }

    fn mount(&self, letra: char) -> Result<&Mount<'a>> {
        self.montagens.get(&letra).ok_or_else(|| {
            let montadas: String = self
                .montagens
                .keys()
                .map(|l| format!("{l}:"))
                .collect::<Vec<_>>()
                .join(" ");
            Error::format(format!(
                "'{letra}:' não está montado (montados: {})",
                if montadas.is_empty() {
                    "nenhum".to_owned()
                } else {
                    montadas
                }
            ))
        })
    }
}

fn normalizar_letra(letra: char) -> Result<char> {
    if !letra.is_ascii_alphabetic() {
        return Err(Error::format(format!("'{letra}' não é letra de drive")));
    }
    Ok(letra.to_ascii_uppercase())
}

fn nome_final(relativo: &str) -> String {
    relativo
        .rsplit(['\\', '/'])
        .find(|p| !p.is_empty())
        .unwrap_or(relativo)
        .to_owned()
}

/// Traduz um caminho do guest em caminho do host, **sem deixar sair da raiz montada**.
///
/// Recusa `..`, recusa componente com `:` e, depois de montar o caminho, confere que
/// ele continua debaixo da raiz. A checagem dupla é de propósito: a primeira barra o
/// ataque óbvio, a segunda cobre o que o sistema de arquivos possa interpretar de forma
/// inesperada (link simbólico, nome reservado, normalização de Unicode).
fn caminho_no_host(raiz: &Path, relativo: &str) -> Result<PathBuf> {
    let mut alvo = raiz.to_path_buf();
    for parte in relativo.split(['\\', '/']) {
        if parte.is_empty() || parte == "." {
            continue;
        }
        if parte == ".." {
            return Err(Error::format(format!(
                "caminho '{relativo}' tenta sair da raiz montada"
            )));
        }
        if parte.contains(':') {
            return Err(Error::format(format!(
                "componente '{parte}' em '{relativo}' não é nome de arquivo válido"
            )));
        }
        alvo.push(parte);
    }

    // Segunda barreira: nenhum componente do resultado pode ser `..`, e o prefixo
    // precisa continuar sendo a raiz.
    if alvo.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(Error::format("caminho resolvido contém '..'"));
    }
    if !alvo.starts_with(raiz) {
        return Err(Error::format("caminho resolvido saiu da raiz montada"));
    }

    // Terceira barreira: canonicalizar. As duas acima não detectam **link simbólico** —
    // um link dentro da pasta montada aponta para fora sem que nenhum componente do
    // caminho seja `..`. Canonicalizar resolve os links e revela o alvo real.
    //
    // O que isto **não** fecha é a corrida entre canonicalizar e abrir: o alvo pode ser
    // trocado por um link nesse intervalo. A correção é abertura relativa a descritor
    // de diretório (`openat2(RESOLVE_BENEATH)` no Linux, verificação do handle no
    // Windows), que é código específico de SO e mora em `xbox-platform` — ADR-0012 e
    // item da Fase 7.
    let raiz_real = std::fs::canonicalize(raiz).map_err(|e| Error::io_path(raiz, e))?;
    match std::fs::canonicalize(&alvo) {
        Ok(real) if real.starts_with(&raiz_real) => Ok(real),
        Ok(_) => Err(Error::format(format!(
            "caminho '{relativo}' sai da raiz montada através de link simbólico"
        ))),
        // Não existe: deixa o chamador falhar ao abrir, com o erro de E/S certo.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(alvo),
        Err(e) => Err(Error::io_path(&alvo, e)),
    }
}

#[cfg(test)]
mod tests {
    use super::{Mount, Vfs};
    use crate::fatx::Fatx;
    use crate::fixtures;
    use crate::xbe::Xbe;
    use crate::xdvdfs::Xdvdfs;

    fn pasta_de_teste(nome: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("obsidian-vfs-teste").join(nome);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn resolve_formas_de_caminho() {
        for (entrada, esperado) in [
            ("D:\\default.xbe", ('D', "default.xbe")),
            ("D:/default.xbe", ('D', "default.xbe")),
            ("d:default.xbe", ('D', "default.xbe")),
            ("T:\\saves\\ug2.bin", ('T', "saves\\ug2.bin")),
            ("D:\\", ('D', "")),
            ("\\??\\D:\\default.xbe", ('D', "default.xbe")),
            ("\\Device\\CdRom0\\default.xbe", ('D', "default.xbe")),
            ("\\Device\\CdRom0", ('D', "")),
            ("\\device\\harddisk0\\partition1\\udata", ('E', "udata")),
        ] {
            let (letra, resto) =
                Vfs::resolver(entrada).unwrap_or_else(|e| panic!("{entrada}: {e}"));
            assert_eq!((letra, resto.as_str()), esperado, "entrada: {entrada}");
        }
    }

    #[test]
    fn caminho_sem_drive_e_recusado() {
        for ruim in [
            "",
            "   ",
            "default.xbe",
            "\\Device\\Desconhecido\\x",
            "1:\\x",
        ] {
            assert!(Vfs::resolver(ruim).is_err(), "aceitou '{ruim}'");
        }
    }

    #[test]
    fn monta_imagem_como_d_e_le_o_executavel() {
        let img = fixtures::xiso_minimo();
        let mut vfs = Vfs::new();
        vfs.montar('D', Mount::Disco(Xdvdfs::open(&img).unwrap()))
            .unwrap();

        assert!(vfs.montado('d'), "a letra deve ser normalizada");
        let info = vfs.info("D:\\default.xbe").unwrap();
        assert!(!info.diretorio);

        let bytes = vfs.ler("D:\\default.xbe").unwrap();
        let xbe = Xbe::parse(&bytes).unwrap();
        assert_eq!(xbe.titulo(), fixtures::TITULO);

        // E pelo caminho de dispositivo, que é o que o kernel recebe.
        let iguais = vfs.ler("\\Device\\CdRom0\\default.xbe").unwrap();
        assert_eq!(bytes, iguais);
    }

    #[test]
    fn monta_fatx_como_e_e_le_arquivo() {
        let particao = fixtures::fatx_minima();
        let mut vfs = Vfs::new();
        vfs.montar('E', Mount::Particao(Fatx::open(&particao).unwrap()))
            .unwrap();

        let dados = vfs.ler("E:\\savedata.bin").unwrap();
        assert_eq!(dados, fixtures::FATX_SAVE);

        let grande = vfs.ler_faixa("E:\\TDATA\\grande.bin", 100, 16).unwrap();
        for (i, b) in grande.iter().enumerate() {
            assert_eq!(*b, ((100 + i) % 251) as u8);
        }
    }

    #[test]
    fn monta_pasta_do_host_como_t() {
        let raiz = pasta_de_teste("tdata");
        std::fs::write(raiz.join("save.bin"), b"estado do jogo").unwrap();
        std::fs::create_dir_all(raiz.join("sub")).unwrap();

        let mut vfs = Vfs::new();
        vfs.montar('T', Mount::Pasta(raiz.clone())).unwrap();

        assert_eq!(vfs.ler("T:\\save.bin").unwrap(), b"estado do jogo");
        assert_eq!(vfs.ler_faixa("T:\\save.bin", 7, 2).unwrap(), b"do");

        let listagem = vfs.listar("T:\\").unwrap();
        let nomes: Vec<&str> = listagem.iter().map(|f| f.nome.as_str()).collect();
        assert_eq!(nomes, vec!["save.bin", "sub"]);
        assert!(listagem.iter().find(|f| f.nome == "sub").unwrap().diretorio);

        std::fs::remove_dir_all(&raiz).ok();
    }

    #[test]
    fn travessia_de_diretorio_e_bloqueada() {
        // Este é o teste de segurança da camada: um título malicioso (ou só mal
        // escrito) não pode ler fora da pasta montada.
        let raiz = pasta_de_teste("travessia");
        std::fs::write(raiz.join("ok.bin"), b"dentro").unwrap();
        let fora = raiz.parent().unwrap().join("fora.bin");
        std::fs::write(&fora, b"isto nao deveria ser legivel").unwrap();

        let mut vfs = Vfs::new();
        vfs.montar('T', Mount::Pasta(raiz.clone())).unwrap();

        for ataque in [
            "T:\\..\\fora.bin",
            "T:\\sub\\..\\..\\fora.bin",
            "T:/../fora.bin",
            "T:\\..\\..\\..\\..\\Windows\\System32\\config",
            "T:\\C:\\Windows",
        ] {
            let r = vfs.ler(ataque);
            assert!(r.is_err(), "travessia aceita: {ataque}");
            let msg = r.unwrap_err().to_string();
            assert!(
                msg.contains("sair da raiz") || msg.contains("não é nome de arquivo válido"),
                "{ataque} falhou pelo motivo errado: {msg}"
            );
        }

        // E o caminho legítimo continua funcionando.
        assert_eq!(vfs.ler("T:\\ok.bin").unwrap(), b"dentro");

        std::fs::remove_file(&fora).ok();
        std::fs::remove_dir_all(&raiz).ok();
    }

    #[test]
    fn drive_nao_montado_da_erro_que_diz_o_que_esta_montado() {
        let img = fixtures::xiso_minimo();
        let mut vfs = Vfs::new();
        vfs.montar('D', Mount::Disco(Xdvdfs::open(&img).unwrap()))
            .unwrap();

        let err = vfs.ler("Z:\\cache.dat").unwrap_err().to_string();
        assert!(err.contains("'Z:' não está montado"), "veio: {err}");
        assert!(
            err.contains("D:"),
            "o erro deveria listar o que está montado: {err}"
        );
    }

    #[test]
    fn pasta_inexistente_nao_pode_ser_montada() {
        let mut vfs = Vfs::new();
        let r = vfs.montar('T', Mount::Pasta("nao/existe/em/lugar/algum".into()));
        assert!(r.is_err());
    }

    #[test]
    fn desmontar_devolve_a_montagem() {
        let img = fixtures::xiso_minimo();
        let mut vfs = Vfs::new();
        vfs.montar('D', Mount::Disco(Xdvdfs::open(&img).unwrap()))
            .unwrap();
        assert!(vfs.desmontar('D').is_some());
        assert!(!vfs.montado('D'));
        assert!(vfs.desmontar('D').is_none());
    }

    #[test]
    fn conjunto_completo_como_na_fase_4() {
        // O arranjo que o emulador vai montar para rodar um jogo: disco, saves, cache.
        let img = fixtures::xiso_minimo();
        let particao = fixtures::fatx_minima();
        let tdata = pasta_de_teste("completo-t");
        let cache = pasta_de_teste("completo-z");

        let mut vfs = Vfs::new();
        vfs.montar('D', Mount::Disco(Xdvdfs::open(&img).unwrap()))
            .unwrap();
        vfs.montar('E', Mount::Particao(Fatx::open(&particao).unwrap()))
            .unwrap();
        vfs.montar('T', Mount::Pasta(tdata.clone())).unwrap();
        vfs.montar('Z', Mount::Pasta(cache.clone())).unwrap();

        assert_eq!(vfs.letras(), vec!['D', 'E', 'T', 'Z']);
        assert!(vfs.existe("D:\\default.xbe"));
        assert!(vfs.existe("E:\\savedata.bin"));
        assert!(!vfs.existe("D:\\nao-existe"));

        let gravaveis: Vec<char> = vfs
            .letras()
            .into_iter()
            .filter(|l| matches!(l, 'T' | 'Z'))
            .collect();
        assert_eq!(gravaveis, vec!['T', 'Z']);

        std::fs::remove_dir_all(&tdata).ok();
        std::fs::remove_dir_all(&cache).ok();
    }
}
