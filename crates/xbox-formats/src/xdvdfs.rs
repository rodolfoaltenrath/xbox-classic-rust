//! Leitor do filesystem do DVD do Xbox (XDVDFS), usado em imagens XISO.
//!
//! É o filesystem que o `D:` do console expõe. Somente leitura — não existe caso de
//! uso para escrever num disco de jogo.
//!
//! ## Estrutura
//!
//! Setores de 2048 bytes. O descritor de volume fica no **setor 32** da partição de
//! jogo, com a assinatura `MICROSOFT*XBOX*MEDIA` no começo e no fim do setor. Cada
//! diretório é uma **árvore binária** de entradas, onde os ponteiros filhos são
//! deslocamentos em unidades de 4 bytes dentro dos dados do diretório.
//!
//! Um ponteiro vazio aparece como `0xFFFF` **ou como `0`** — imagens reais usam `0`,
//! verificado em disco de varejo. Como o deslocamento 0 é a raiz da própria árvore,
//! tratar `0` como "sem filho" é correto nos dois casos.
//!
//! ## Onde a partição começa
//!
//! - **imagens cortadas** (o que `extract-xiso` e afins produzem): partição em 0;
//! - **dumps completos**, com a partição de vídeo: partição em `0x18300000`.
//!
//! [`Xdvdfs::open`] detecta qual é; [`Xdvdfs::open_at`] força uma base.
//!
//! ## Leitura sob demanda
//!
//! A mídia é um [`Media`], então o leitor funciona tanto sobre bytes em memória quanto
//! sobre um arquivo de vários gigabytes sem carregá-lo — ver [`crate::media`].
//!
//! ## Robustez
//!
//! A árvore vem do arquivo, então ponteiros podem formar ciclo ou apontar para fora. O
//! caminhamento mantém um conjunto de deslocamentos visitados e limites de entradas,
//! de tamanho de diretório e de profundidade: imagem maliciosa dá erro, nunca laço
//! infinito nem alocação ilimitada (ROADMAP §13.3).

use std::borrow::Cow;
use std::collections::HashSet;
use std::path::Path;

use xbox_core::{Error, Result};

use crate::bytes::{latin1_at, u8_at, u16_at, u32_at, u64_at};
use crate::media::Media;

/// Tamanho de um setor.
pub const SECTOR_SIZE: u64 = 2048;

/// Setor do descritor de volume, contado a partir da base da partição.
pub const VOLUME_SECTOR: u64 = 32;

/// Assinatura do descritor de volume.
pub const MAGIC: &[u8; 20] = b"MICROSOFT*XBOX*MEDIA";

/// Bases de partição conhecidas, na ordem em que são testadas.
pub const BASES_CONHECIDAS: [u64; 2] = [0, 0x1830_0000];

/// Tamanho do cabeçalho de uma entrada de diretório, sem o nome.
const ENTRY_HEADER_SIZE: usize = 14;

/// Marcador de "sem filho" nos ponteiros da árvore.
const SEM_FILHO: u16 = 0xFFFF;

/// Limite defensivo de entradas por diretório.
const MAX_ENTRADAS_POR_DIR: usize = 4096;

/// Limite defensivo do tamanho de um diretório. Diretórios reais têm poucos KB; sem
/// este limite, um campo de tamanho corrompido viraria uma alocação de gigabytes.
const MAX_BYTES_POR_DIR: u32 = 1 << 20;

/// Limite defensivo de profundidade ao percorrer a árvore de diretórios.
pub const MAX_PROFUNDIDADE: usize = 32;

/// Atributo: somente leitura.
pub const ATTR_READONLY: u8 = 0x01;
/// Atributo: oculto.
pub const ATTR_HIDDEN: u8 = 0x02;
/// Atributo: de sistema.
pub const ATTR_SYSTEM: u8 = 0x04;
/// Atributo: diretório.
pub const ATTR_DIRECTORY: u8 = 0x10;
/// Atributo: arquivo comum.
pub const ATTR_ARCHIVE: u8 = 0x20;
/// Atributo: normal.
pub const ATTR_NORMAL: u8 = 0x80;

/// Descritor de volume.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Volume {
    /// Setor onde começa o diretório raiz.
    pub root_sector: u32,
    /// Tamanho do diretório raiz, em bytes.
    pub root_size: u32,
    /// Data/hora de criação (`FILETIME`).
    pub timestamp: u64,
}

/// Uma entrada de diretório.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    /// Nome, como gravado na imagem.
    pub name: String,
    /// Setor inicial dos dados.
    pub sector: u32,
    /// Tamanho em bytes (para diretório, o tamanho dos dados do diretório).
    pub size: u32,
    /// Atributos.
    pub attributes: u8,
}

impl DirEntry {
    /// É um diretório?
    pub const fn is_dir(&self) -> bool {
        self.attributes & ATTR_DIRECTORY != 0
    }
}

/// Imagem XDVDFS aberta.
#[derive(Debug)]
pub struct Xdvdfs<'a> {
    media: Media<'a>,
    base: u64,
    volume: Volume,
}

impl<'a> Xdvdfs<'a> {
    /// Abre uma mídia, detectando a base da partição entre [`BASES_CONHECIDAS`].
    pub fn open(media: impl Into<Media<'a>>) -> Result<Self> {
        let media = media.into();
        let mut tentativas = Vec::new();
        for base in BASES_CONHECIDAS {
            match Self::ler_volume(&media, base) {
                Ok(volume) => {
                    return Ok(Self {
                        media,
                        base,
                        volume,
                    });
                }
                Err(e) => tentativas.push(format!("base 0x{base:X}: {e}")),
            }
        }
        Err(Error::format(format!(
            "não parece uma imagem XDVDFS ({})",
            tentativas.join("; ")
        )))
    }

    /// Abre uma mídia com a base da partição informada, sem detecção.
    pub fn open_at(media: impl Into<Media<'a>>, base: u64) -> Result<Self> {
        let media = media.into();
        let volume = Self::ler_volume(&media, base)?;
        Ok(Self {
            media,
            base,
            volume,
        })
    }

    /// Abre um arquivo de imagem, lendo sob demanda.
    ///
    /// É o caminho certo para imagem de jogo real: nada de carregar gigabytes na RAM.
    pub fn open_path(caminho: impl AsRef<Path>) -> Result<Self> {
        Self::open(Media::open(caminho)?)
    }

    fn ler_volume(media: &Media<'a>, base: u64) -> Result<Volume> {
        let off = base
            .checked_add(VOLUME_SECTOR * SECTOR_SIZE)
            .ok_or_else(|| Error::format("base de partição absurda"))?;
        let setor = media.read_at(off, SECTOR_SIZE as usize)?;
        let d = &*setor;

        if &d[..MAGIC.len()] != MAGIC {
            return Err(Error::format("assinatura ausente"));
        }
        // A assinatura também aparece no fim do setor; conferir as duas evita aceitar
        // um arquivo que só coincide nos primeiros 20 bytes.
        if &d[SECTOR_SIZE as usize - MAGIC.len()..] != MAGIC {
            return Err(Error::format("assinatura final ausente"));
        }
        Ok(Volume {
            root_sector: u32_at(d, 0x14)?,
            root_size: u32_at(d, 0x18)?,
            timestamp: u64_at(d, 0x1C)?,
        })
    }

    /// Base da partição de jogo detectada.
    pub const fn base(&self) -> u64 {
        self.base
    }

    /// Descritor de volume.
    pub const fn volume(&self) -> Volume {
        self.volume
    }

    /// Tamanho total da mídia.
    pub fn tamanho_da_midia(&self) -> u64 {
        self.media.len()
    }

    /// Entradas do diretório raiz, ordenadas por nome.
    pub fn root(&self) -> Result<Vec<DirEntry>> {
        self.listar_faixa(self.volume.root_sector, self.volume.root_size)
    }

    /// Entradas de um diretório.
    pub fn list(&self, dir: &DirEntry) -> Result<Vec<DirEntry>> {
        if !dir.is_dir() {
            return Err(Error::format(format!("'{}' não é diretório", dir.name)));
        }
        self.listar_faixa(dir.sector, dir.size)
    }

    /// Conteúdo completo de um arquivo.
    ///
    /// Em mídia de arquivo isto aloca o tamanho do arquivo — cuidado com os de dezenas
    /// de megabytes; para esses, use [`Xdvdfs::read_range`].
    pub fn read(&self, entry: &DirEntry) -> Result<Cow<'a, [u8]>> {
        self.read_range(entry, 0, entry.size as usize)
    }

    /// Lê um pedaço de um arquivo.
    ///
    /// É esta a forma que o `NtReadFile` do kernel HLE (Fase 4) vai usar: o jogo lê
    /// pedaços durante a gameplay, não o arquivo inteiro.
    pub fn read_range(&self, entry: &DirEntry, off: u64, len: usize) -> Result<Cow<'a, [u8]>> {
        if entry.is_dir() {
            return Err(Error::format(format!("'{}' é diretório", entry.name)));
        }
        let fim = off
            .checked_add(len as u64)
            .ok_or_else(|| Error::format("faixa absurda"))?;
        if fim > u64::from(entry.size) {
            return Err(Error::format(format!(
                "'{}' tem {} bytes; pedia {len} bytes em {off}",
                entry.name, entry.size
            )));
        }
        let inicio = self.offset_de(entry.sector)? + off;
        self.media.read_at(inicio, len)
    }

    /// Procura uma entrada por caminho, separado por `/` ou `\`, sem distinguir
    /// maiúsculas (como o console faz).
    pub fn find(&self, caminho: &str) -> Result<DirEntry> {
        let partes: Vec<&str> = caminho
            .split(['/', '\\'])
            .filter(|p| !p.is_empty() && *p != ".")
            .collect();
        if partes.is_empty() {
            return Err(Error::format("caminho vazio"));
        }

        let mut atual: Option<DirEntry> = None;
        for (i, parte) in partes.iter().enumerate() {
            let entradas = match &atual {
                None => self.root()?,
                Some(e) => self.list(e)?,
            };
            let achado = entradas
                .into_iter()
                .find(|e| e.name.eq_ignore_ascii_case(parte))
                .ok_or_else(|| Error::format(format!("'{caminho}': '{parte}' não existe")))?;
            let ultimo = i + 1 == partes.len();
            if !ultimo && !achado.is_dir() {
                return Err(Error::format(format!(
                    "'{caminho}': '{parte}' não é diretório"
                )));
            }
            atual = Some(achado);
        }
        atual.ok_or_else(|| Error::format("caminho vazio"))
    }

    /// Atalho para o executável principal do disco.
    pub fn default_xbe(&self) -> Result<DirEntry> {
        self.find("default.xbe")
    }

    /// Percorre a imagem inteira, devolvendo `(caminho, entrada)` de tudo.
    ///
    /// Ordenado por caminho, e portanto estável entre execuções.
    pub fn walk(&self) -> Result<Vec<(String, DirEntry)>> {
        let mut saida = Vec::new();
        let mut pilha: Vec<(String, DirEntry, usize)> = Vec::new();

        for e in self.root()? {
            pilha.push((e.name.clone(), e, 1));
        }
        while let Some((caminho, entrada, profundidade)) = pilha.pop() {
            if profundidade > MAX_PROFUNDIDADE {
                return Err(Error::format(format!(
                    "diretórios aninhados além de {MAX_PROFUNDIDADE} níveis em '{caminho}'"
                )));
            }
            if entrada.is_dir() {
                for filho in self.list(&entrada)? {
                    pilha.push((format!("{caminho}/{}", filho.name), filho, profundidade + 1));
                }
            }
            saida.push((caminho, entrada));
        }
        saida.sort_by_key(|(caminho, _)| caminho.to_ascii_lowercase());
        Ok(saida)
    }

    fn offset_de(&self, sector: u32) -> Result<u64> {
        self.base
            .checked_add(u64::from(sector) * SECTOR_SIZE)
            .ok_or_else(|| Error::format(format!("setor {sector} fora do alcance")))
    }

    fn listar_faixa(&self, sector: u32, tamanho: u32) -> Result<Vec<DirEntry>> {
        if tamanho == 0 {
            return Ok(Vec::new());
        }
        if tamanho > MAX_BYTES_POR_DIR {
            return Err(Error::format(format!(
                "diretório no setor {sector} declara {tamanho} bytes (limite {MAX_BYTES_POR_DIR})"
            )));
        }
        let inicio = self.offset_de(sector)?;
        let buffer = self.media.read_at(inicio, tamanho as usize)?;
        let dados = &*buffer;

        let mut saida = Vec::new();
        let mut pilha = vec![0u32];
        let mut vistos: HashSet<u32> = HashSet::new();

        while let Some(dw) = pilha.pop() {
            if !vistos.insert(dw) {
                // Já visitado: a imagem tem um ciclo na árvore. Ignorar basta, porque o
                // conjunto garante terminação.
                continue;
            }
            if saida.len() >= MAX_ENTRADAS_POR_DIR {
                return Err(Error::format(format!(
                    "diretório no setor {sector} declara mais de {MAX_ENTRADAS_POR_DIR} entradas"
                )));
            }

            let off = (dw as usize) * 4;
            let esquerda = u16_at(dados, off)?;
            let direita = u16_at(dados, off + 2)?;
            let entry_sector = u32_at(dados, off + 4)?;
            let entry_size = u32_at(dados, off + 8)?;
            let attributes = u8_at(dados, off + 12)?;
            let nome_len = u8_at(dados, off + 13)?;

            if nome_len == 0 || nome_len == 0xFF {
                // Preenchimento de fim de setor, não entrada.
                continue;
            }
            let name = latin1_at(dados, off + ENTRY_HEADER_SIZE, nome_len as usize)?;
            saida.push(DirEntry {
                name,
                sector: entry_sector,
                size: entry_size,
                attributes,
            });

            for filho in [esquerda, direita] {
                // `0` é o deslocamento da própria raiz da árvore: como filho só pode
                // significar "sem filho" — e é assim que imagens reais marcam.
                if filho != SEM_FILHO && filho != 0 {
                    pilha.push(u32::from(filho));
                }
            }
        }

        saida.sort_by_key(|e| e.name.to_ascii_lowercase());
        Ok(saida)
    }
}

#[cfg(test)]
mod tests {
    use super::Xdvdfs;
    use crate::fixtures;
    use crate::media::Media;
    use crate::xbe::Xbe;

    #[test]
    fn abre_imagem_cortada() {
        let img = fixtures::xiso_minimo();
        let fs = Xdvdfs::open(&img).expect("deve abrir");
        assert_eq!(fs.base(), 0);
        assert_eq!(fs.volume().root_size % 4, 0);
    }

    #[test]
    fn abre_imagem_com_particao_deslocada() {
        // A base real de uma imagem completa é 0x18300000, mas construir a fixture
        // nesse endereço alocaria ~406 MB. A aritmética exercitada é a mesma, então o
        // teste usa uma base pequena com `open_at`.
        let base = 0x4000;
        let img = fixtures::xiso_com_base(base);
        let fs = Xdvdfs::open_at(&img, base).expect("deve abrir");
        assert_eq!(fs.base(), base);
        assert!(fs.default_xbe().is_ok());
        // E a detecção automática não deve confundir essa imagem com base 0.
        assert!(Xdvdfs::open(&img).is_err());
    }

    #[test]
    fn lista_a_raiz() {
        let img = fixtures::xiso_minimo();
        let fs = Xdvdfs::open(&img).unwrap();
        let nomes: Vec<String> = fs.root().unwrap().into_iter().map(|e| e.name).collect();
        assert_eq!(nomes, vec!["default.xbe".to_owned(), "media".to_owned()]);
    }

    #[test]
    fn entra_em_subdiretorio() {
        let img = fixtures::xiso_minimo();
        let fs = Xdvdfs::open(&img).unwrap();
        let media = fs.find("media").unwrap();
        assert!(media.is_dir());
        let filhos = fs.list(&media).unwrap();
        assert_eq!(filhos.len(), 1);
        assert_eq!(filhos[0].name, "readme.txt");
    }

    #[test]
    fn le_arquivo_em_subdiretorio_por_caminho() {
        let img = fixtures::xiso_minimo();
        let fs = Xdvdfs::open(&img).unwrap();
        let e = fs
            .find("MEDIA\\ReadMe.TXT")
            .expect("busca sem distinguir caixa");
        assert_eq!(&*fs.read(&e).unwrap(), fixtures::README);
    }

    #[test]
    fn leitura_parcial_de_arquivo() {
        let img = fixtures::xiso_minimo();
        let fs = Xdvdfs::open(&img).unwrap();
        let e = fs.find("media/readme.txt").unwrap();
        let pedaco = fs.read_range(&e, 8, 6).unwrap();
        assert_eq!(&*pedaco, &fixtures::README[8..14]);
        assert!(fs.read_range(&e, 0, fixtures::README.len() + 1).is_err());
    }

    #[test]
    fn o_default_xbe_da_imagem_e_um_xbe_valido() {
        // Este é o teste que amarra os dois formatos: o que sai do XDVDFS tem que
        // entrar no parser de XBE sem intermediário.
        let img = fixtures::xiso_minimo();
        let fs = Xdvdfs::open(&img).unwrap();
        let entrada = fs.default_xbe().unwrap();
        let bytes = fs.read(&entrada).unwrap();
        let xbe = Xbe::parse(&bytes).expect("default.xbe deve parsear");
        assert_eq!(xbe.titulo(), fixtures::TITULO);
    }

    #[test]
    fn walk_lista_tudo_em_ordem_estavel() {
        let img = fixtures::xiso_minimo();
        let fs = Xdvdfs::open(&img).unwrap();
        let caminhos: Vec<String> = fs.walk().unwrap().into_iter().map(|(c, _)| c).collect();
        assert_eq!(
            caminhos,
            vec![
                "default.xbe".to_owned(),
                "media".to_owned(),
                "media/readme.txt".to_owned(),
            ]
        );
    }

    #[test]
    fn funciona_sobre_arquivo_sem_carregar_tudo() {
        let img = fixtures::xiso_minimo();
        let dir = std::env::temp_dir().join("obsidian-xdvdfs-teste");
        std::fs::create_dir_all(&dir).unwrap();
        let caminho = dir.join("fixture.iso");
        std::fs::write(&caminho, &img).unwrap();

        let fs = Xdvdfs::open_path(&caminho).expect("deve abrir do disco");
        assert_eq!(fs.tamanho_da_midia(), img.len() as u64);
        let e = fs.default_xbe().unwrap();
        let bytes = fs.read(&e).unwrap();
        assert!(
            matches!(bytes, std::borrow::Cow::Owned(_)),
            "arquivo deveria copiar"
        );
        assert!(Xbe::parse(&bytes).is_ok());

        std::fs::remove_file(&caminho).ok();
    }

    #[test]
    fn arquivo_que_nao_e_imagem_e_recusado() {
        let err = Xdvdfs::open(Media::from_slice(b"isto nao e uma imagem")).unwrap_err();
        assert!(
            err.to_string().contains("não parece uma imagem"),
            "veio: {err}"
        );
    }

    #[test]
    fn caminho_inexistente_e_erro_claro() {
        let img = fixtures::xiso_minimo();
        let fs = Xdvdfs::open(&img).unwrap();
        let err = fs.find("nao/existe.bin").unwrap_err();
        assert!(err.to_string().contains("não existe"), "veio: {err}");
    }

    #[test]
    fn ciclo_na_arvore_nao_trava() {
        let img = fixtures::xiso_com_ciclo();
        let fs = Xdvdfs::open(&img).unwrap();
        let entradas = fs.root().expect("deve terminar");
        assert_ne!(entradas.len(), 0);
    }

    #[test]
    fn diretorio_de_tamanho_absurdo_e_recusado_sem_alocar() {
        let mut img = fixtures::xiso_minimo();
        // root_size no descritor de volume: 2 GB.
        let off = 32 * 2048 + 0x18;
        img[off..off + 4].copy_from_slice(&0x8000_0000u32.to_le_bytes());
        let fs = Xdvdfs::open(&img).unwrap();
        let err = fs.root().unwrap_err();
        assert!(err.to_string().contains("limite"), "veio: {err}");
    }

    #[test]
    fn imagem_truncada_nao_causa_panico() {
        let img = fixtures::xiso_minimo();
        for n in (0..img.len()).step_by(997) {
            if let Ok(fs) = Xdvdfs::open(&img[..n]) {
                let _ = fs.walk();
            }
        }
    }
}
