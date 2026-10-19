//! Leitor do filesystem do HD e dos memory units (FATX).
//!
//! FATX é um FAT simplificado que a Microsoft usou no HD do Xbox, nos memory units e
//! nos cartões de memória. É onde moram os saves, o cache dos títulos e os arquivos de
//! sistema — ou seja, é o que o emulador precisa para dar `C:`, `E:`, `T:`, `U:`, `X:`,
//! `Y:` e `Z:` ao jogo.
//!
//! Somente leitura nesta fase. Escrita (necessária para o jogo **salvar**) entra na
//! Fase 9; até lá, as partições graváveis são atendidas por pasta do host através da
//! [`crate::vfs`], que é mais simples e mais seguro do que escrever num FAT emulado.
//!
//! ## Diferenças em relação ao FAT comum
//!
//! - setor de 512 bytes, mas o superbloco ocupa **4096** bytes e a FAT começa em
//!   `0x1000`;
//! - entradas de diretório têm 64 bytes com nome de até 42 caracteres, sem 8.3 e sem
//!   entradas de nome longo;
//! - a numeração de cluster começa em **1**, e o cluster 1 é o diretório raiz;
//! - a largura da entrada da FAT (16 ou 32 bits) vem do número de clusters, não de um
//!   campo.
//!
//! ## Robustez
//!
//! Cadeia de clusters é lista ligada vinda do disco: pode ter ciclo, pode apontar para
//! fora. O caminhamento mantém conjunto de visitados e limites de clusters, de entradas
//! e de profundidade — imagem corrompida dá erro, nunca laço infinito (ROADMAP §13.3).
//!
//! ## Conformidade
//!
//! Verificado contra fixture gerada por código; **não** contra um HD real (não temos
//! um). O que isso implica está registrado em `docs/compat/formatos.md`.

use std::collections::HashSet;
use std::path::Path;

use xbox_core::{Error, Result};

use crate::bytes::{latin1, slice_at, u16_at, u32_at};
use crate::media::Media;

/// Assinatura no início de uma partição FATX.
pub const MAGIC: [u8; 4] = *b"FATX";

/// Tamanho de setor.
pub const SECTOR_SIZE: u64 = 512;

/// Tamanho do superbloco, e deslocamento onde a FAT começa.
pub const FAT_OFFSET: u64 = 0x1000;

/// Tamanho de uma entrada de diretório.
pub const DIR_ENTRY_SIZE: usize = 64;

/// Comprimento máximo de nome.
pub const MAX_NAME_LEN: usize = 42;

/// Marcador de entrada apagada.
const NOME_APAGADO: u8 = 0xE5;

/// Limite defensivo de clusters em uma cadeia (≈16 GiB com cluster de 16 KiB).
const MAX_CLUSTERS_POR_CADEIA: usize = 1 << 20;

/// Limite defensivo de entradas por diretório.
const MAX_ENTRADAS_POR_DIR: usize = 4096;

/// Limite defensivo de profundidade de diretórios.
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

/// Partições padrão do HD do Xbox, em deslocamento e tamanho.
///
/// ⚠️ **A validar contra um HD real.** Esta tabela vem de documentação pública e é o
/// esqueleto de trabalho; nenhuma parte do código depende dela além de
/// [`Fatx::open_particao_do_hd`], que é conveniência.
pub const PARTICOES_DO_HD: [(char, u64, u64); 5] = [
    ('X', 0x0008_0000, 0x2EE0_0000), // cache 1
    ('Y', 0x2EE8_0000, 0x2EE0_0000), // cache 2
    ('Z', 0x5DC8_0000, 0x2EE0_0000), // cache 3
    ('C', 0x8CA8_0000, 0x1F40_0000), // sistema
    ('E', 0xABE8_0000, 0x0),         // dados: até o fim do disco
];

/// Superbloco de uma partição FATX.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SuperBlock {
    /// Identificador do volume.
    pub volume_id: u32,
    /// Tamanho do cluster, em setores.
    pub cluster_sectors: u32,
    /// Quantidade de cópias da FAT.
    pub fat_copies: u16,
}

impl SuperBlock {
    /// Tamanho do cluster em bytes.
    pub const fn cluster_bytes(self) -> u64 {
        self.cluster_sectors as u64 * SECTOR_SIZE
    }
}

/// Largura da entrada da FAT.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LarguraFat {
    /// 16 bits por entrada (FATX16).
    Bits16,
    /// 32 bits por entrada (FATX32).
    Bits32,
}

impl LarguraFat {
    /// Bytes por entrada.
    pub const fn bytes(self) -> u64 {
        match self {
            Self::Bits16 => 2,
            Self::Bits32 => 4,
        }
    }

    /// Valor a partir do qual a entrada marca fim de cadeia.
    pub const fn fim_de_cadeia(self) -> u32 {
        match self {
            Self::Bits16 => 0xFFF8,
            Self::Bits32 => 0xFFFF_FFF8,
        }
    }
}

/// Uma entrada de diretório FATX.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    /// Nome do arquivo ou diretório.
    pub name: String,
    /// Atributos.
    pub attributes: u8,
    /// Primeiro cluster da cadeia.
    pub first_cluster: u32,
    /// Tamanho em bytes (0 para diretório).
    pub size: u32,
    /// Data/hora de criação, no formato empacotado do FATX.
    pub creation_time: u32,
    /// Data/hora da última escrita.
    pub last_write_time: u32,
    /// Data/hora do último acesso.
    pub last_access_time: u32,
}

impl DirEntry {
    /// É um diretório?
    pub const fn is_dir(&self) -> bool {
        self.attributes & ATTR_DIRECTORY != 0
    }
}

/// Partição FATX aberta.
#[derive(Debug)]
pub struct Fatx<'a> {
    media: Media<'a>,
    /// Deslocamento da partição dentro da mídia.
    inicio: u64,
    /// Tamanho da partição.
    tamanho: u64,
    superbloco: SuperBlock,
    largura: LarguraFat,
    /// Quantidade de entradas válidas na FAT.
    entradas_fat: u32,
    /// Deslocamento, dentro da mídia, onde começa a área de dados.
    dados: u64,
}

impl<'a> Fatx<'a> {
    /// Abre uma partição que começa no início da mídia.
    pub fn open(media: impl Into<Media<'a>>) -> Result<Self> {
        let media = media.into();
        let tamanho = media.len();
        Self::montar(media, 0, tamanho)
    }

    /// Abre uma partição em um deslocamento da mídia.
    ///
    /// `tamanho` igual a 0 significa "até o fim da mídia".
    pub fn open_at(media: impl Into<Media<'a>>, inicio: u64, tamanho: u64) -> Result<Self> {
        let media = media.into();
        let disponivel = media.len().saturating_sub(inicio);
        let tamanho = if tamanho == 0 {
            disponivel
        } else {
            tamanho.min(disponivel)
        };
        Self::montar(media, inicio, tamanho)
    }

    /// Abre um arquivo de partição, lendo sob demanda.
    pub fn open_path(caminho: impl AsRef<Path>) -> Result<Self> {
        Self::open(Media::open(caminho)?)
    }

    /// Abre uma das partições padrão de uma imagem de HD completa.
    ///
    /// Usa [`PARTICOES_DO_HD`], que ainda não foi validada contra hardware real.
    pub fn open_particao_do_hd(media: impl Into<Media<'a>>, letra: char) -> Result<Self> {
        let letra = letra.to_ascii_uppercase();
        let (_, inicio, tamanho) = PARTICOES_DO_HD
            .iter()
            .find(|(l, _, _)| *l == letra)
            .ok_or_else(|| Error::format(format!("'{letra}:' não é partição do HD do Xbox")))?;
        Self::open_at(media, *inicio, *tamanho)
    }

    fn montar(media: Media<'a>, inicio: u64, tamanho: u64) -> Result<Self> {
        let cabecalho = media.read_at(inicio, 0x10)?;
        let d = &*cabecalho;
        if slice_at(d, 0, 4)? != MAGIC {
            return Err(Error::format(format!(
                "não é uma partição FATX: assinatura em 0x{inicio:X} é {:02X?}",
                slice_at(d, 0, 4)?
            )));
        }
        let superbloco = SuperBlock {
            volume_id: u32_at(d, 0x04)?,
            cluster_sectors: u32_at(d, 0x08)?,
            fat_copies: u16_at(d, 0x0C)?,
        };
        if superbloco.cluster_sectors == 0 || !superbloco.cluster_sectors.is_power_of_two() {
            return Err(Error::format(format!(
                "cluster de {} setores não é potência de dois",
                superbloco.cluster_sectors
            )));
        }
        if superbloco.fat_copies == 0 || superbloco.fat_copies > 2 {
            return Err(Error::format(format!(
                "{} cópias de FAT declaradas (esperado 1 ou 2)",
                superbloco.fat_copies
            )));
        }

        let cluster_bytes = superbloco.cluster_bytes();
        if tamanho <= FAT_OFFSET + cluster_bytes {
            return Err(Error::format(format!(
                "partição de {tamanho} bytes é pequena demais para cluster de {cluster_bytes}"
            )));
        }

        // A largura da entrada da FAT depende de quantos clusters existem, e quantos
        // clusters existem depende do tamanho da FAT. A documentação resolve essa
        // circularidade estimando pelo tamanho da partição e só então fixando a
        // largura — é o que fazemos aqui.
        let estimativa = tamanho / cluster_bytes + 1;
        let largura = if estimativa >= 0xFFF0 {
            LarguraFat::Bits32
        } else {
            LarguraFat::Bits16
        };
        let fat_bytes = arredondar_para_cima(estimativa * largura.bytes(), FAT_OFFSET)
            * u64::from(superbloco.fat_copies);
        let dados = inicio
            .checked_add(FAT_OFFSET + fat_bytes)
            .ok_or_else(|| Error::format("área de dados fora do alcance"))?;
        if dados >= inicio + tamanho {
            return Err(Error::format("FAT maior que a própria partição"));
        }
        let entradas_fat = u32::try_from((inicio + tamanho - dados) / cluster_bytes + 1)
            .map_err(|_| Error::format("clusters demais para um u32"))?;

        Ok(Self {
            media,
            inicio,
            tamanho,
            superbloco,
            largura,
            entradas_fat,
            dados,
        })
    }

    /// Superbloco da partição.
    pub const fn superbloco(&self) -> SuperBlock {
        self.superbloco
    }

    /// Largura das entradas da FAT.
    pub const fn largura(&self) -> LarguraFat {
        self.largura
    }

    /// Tamanho da partição, em bytes.
    pub const fn tamanho(&self) -> u64 {
        self.tamanho
    }

    /// Quantidade de clusters endereçáveis.
    pub const fn clusters(&self) -> u32 {
        self.entradas_fat
    }

    /// Entradas do diretório raiz (cluster 1).
    pub fn root(&self) -> Result<Vec<DirEntry>> {
        self.listar_cluster(1)
    }

    /// Entradas de um diretório.
    pub fn list(&self, dir: &DirEntry) -> Result<Vec<DirEntry>> {
        if !dir.is_dir() {
            return Err(Error::format(format!("'{}' não é diretório", dir.name)));
        }
        self.listar_cluster(dir.first_cluster)
    }

    /// Conteúdo completo de um arquivo.
    ///
    /// Sempre copia: os clusters de um arquivo podem estar espalhados pela partição,
    /// então não existe fatia contígua para emprestar.
    pub fn read(&self, entry: &DirEntry) -> Result<Vec<u8>> {
        self.read_range(entry, 0, entry.size as usize)
    }

    /// Lê um pedaço de um arquivo.
    pub fn read_range(&self, entry: &DirEntry, off: u64, len: usize) -> Result<Vec<u8>> {
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

        let cluster_bytes = self.superbloco.cluster_bytes();
        let cadeia = self.cadeia(entry.first_cluster)?;
        let mut saida = Vec::with_capacity(len);
        let mut restante = len;
        let mut posicao = off;

        for cluster in cadeia {
            if restante == 0 {
                break;
            }
            let fim_do_cluster = cluster_bytes;
            if posicao >= fim_do_cluster {
                posicao -= fim_do_cluster;
                continue;
            }
            let dentro = posicao;
            let disponivel = usize::try_from(fim_do_cluster - dentro).unwrap_or(usize::MAX);
            let pedaco = restante.min(disponivel);
            let base = self.offset_do_cluster(cluster)?;
            let bytes = self.media.read_at(base + dentro, pedaco)?;
            saida.extend_from_slice(&bytes);
            restante -= pedaco;
            posicao = 0;
        }

        if restante > 0 {
            return Err(Error::format(format!(
                "'{}' declara {} bytes mas a cadeia de clusters acabou com {restante} \
                 bytes faltando",
                entry.name, entry.size
            )));
        }
        Ok(saida)
    }

    /// Procura uma entrada por caminho, sem distinguir maiúsculas.
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
            if i + 1 != partes.len() && !achado.is_dir() {
                return Err(Error::format(format!(
                    "'{caminho}': '{parte}' não é diretório"
                )));
            }
            atual = Some(achado);
        }
        atual.ok_or_else(|| Error::format("caminho vazio"))
    }

    /// Percorre a partição inteira, devolvendo `(caminho, entrada)`.
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
        saida.sort_by_key(|(c, _)| c.to_ascii_lowercase());
        Ok(saida)
    }

    /// Deslocamento, na mídia, onde começa um cluster.
    fn offset_do_cluster(&self, cluster: u32) -> Result<u64> {
        if cluster == 0 || cluster >= self.entradas_fat {
            return Err(Error::format(format!(
                "cluster {cluster} fora da faixa válida (1..{})",
                self.entradas_fat
            )));
        }
        self.dados
            .checked_add(u64::from(cluster - 1) * self.superbloco.cluster_bytes())
            .ok_or_else(|| Error::format(format!("cluster {cluster} fora do alcance")))
    }

    /// Lê uma entrada da FAT.
    fn entrada_fat(&self, cluster: u32) -> Result<u32> {
        if cluster >= self.entradas_fat {
            return Err(Error::format(format!(
                "entrada {cluster} além da FAT ({} entradas)",
                self.entradas_fat
            )));
        }
        let off = self.inicio + FAT_OFFSET + u64::from(cluster) * self.largura.bytes();
        let bytes = self.media.read_at(off, self.largura.bytes() as usize)?;
        match self.largura {
            LarguraFat::Bits16 => Ok(u32::from(u16_at(&bytes, 0)?)),
            LarguraFat::Bits32 => u32_at(&bytes, 0),
        }
    }

    /// Cadeia de clusters a partir de um cluster inicial.
    fn cadeia(&self, inicial: u32) -> Result<Vec<u32>> {
        let mut saida = Vec::new();
        let mut vistos: HashSet<u32> = HashSet::new();
        let mut atual = inicial;

        while atual != 0 && atual < self.largura.fim_de_cadeia() {
            if !vistos.insert(atual) {
                return Err(Error::format(format!(
                    "cadeia de clusters com ciclo: {atual} aparece duas vezes"
                )));
            }
            if saida.len() >= MAX_CLUSTERS_POR_CADEIA {
                return Err(Error::format(format!(
                    "cadeia com mais de {MAX_CLUSTERS_POR_CADEIA} clusters"
                )));
            }
            saida.push(atual);
            atual = self.entrada_fat(atual)?;
        }
        Ok(saida)
    }

    fn listar_cluster(&self, inicial: u32) -> Result<Vec<DirEntry>> {
        let cluster_bytes = self.superbloco.cluster_bytes();
        let mut saida = Vec::new();

        'clusters: for cluster in self.cadeia(inicial)? {
            let base = self.offset_do_cluster(cluster)?;
            let bloco = self.media.read_at(base, cluster_bytes as usize)?;
            let d = &*bloco;

            let mut off = 0usize;
            while off + DIR_ENTRY_SIZE <= d.len() {
                let nome_len = d[off];
                // 0x00 e 0xFF marcam o fim das entradas deste diretório.
                if nome_len == 0x00 || nome_len == 0xFF {
                    break 'clusters;
                }
                if nome_len == NOME_APAGADO || nome_len as usize > MAX_NAME_LEN {
                    off += DIR_ENTRY_SIZE;
                    continue;
                }
                if saida.len() >= MAX_ENTRADAS_POR_DIR {
                    return Err(Error::format(format!(
                        "diretório com mais de {MAX_ENTRADAS_POR_DIR} entradas"
                    )));
                }

                let name = latin1(slice_at(d, off + 0x02, nome_len as usize)?);
                saida.push(DirEntry {
                    name,
                    attributes: d[off + 0x01],
                    first_cluster: u32_at(d, off + 0x2C)?,
                    size: u32_at(d, off + 0x30)?,
                    creation_time: u32_at(d, off + 0x34)?,
                    last_write_time: u32_at(d, off + 0x38)?,
                    last_access_time: u32_at(d, off + 0x3C)?,
                });
                off += DIR_ENTRY_SIZE;
            }
        }

        saida.sort_by_key(|e| e.name.to_ascii_lowercase());
        Ok(saida)
    }
}

/// Arredonda `valor` para cima, para o próximo múltiplo de `multiplo`.
const fn arredondar_para_cima(valor: u64, multiplo: u64) -> u64 {
    valor.div_ceil(multiplo) * multiplo
}

#[cfg(test)]
mod tests {
    use super::{ATTR_DIRECTORY, Fatx, LarguraFat};
    use crate::fixtures;

    #[test]
    fn abre_particao_e_le_superbloco() {
        let img = fixtures::fatx_minima();
        let fs = Fatx::open(&img).expect("deve abrir");
        assert_eq!(
            fs.superbloco().cluster_sectors,
            fixtures::FATX_CLUSTER_SETORES
        );
        assert_eq!(fs.superbloco().cluster_bytes(), 16 * 1024);
        assert_eq!(fs.largura(), LarguraFat::Bits16);
        assert!(fs.clusters() > 1);
    }

    #[test]
    fn lista_a_raiz() {
        let img = fixtures::fatx_minima();
        let fs = Fatx::open(&img).unwrap();
        let nomes: Vec<String> = fs.root().unwrap().into_iter().map(|e| e.name).collect();
        // Ordenação é sem distinguir caixa, então "savedata.bin" vem antes de "TDATA".
        assert_eq!(nomes, vec!["savedata.bin".to_owned(), "TDATA".to_owned()]);
    }

    #[test]
    fn le_arquivo_de_um_cluster() {
        let img = fixtures::fatx_minima();
        let fs = Fatx::open(&img).unwrap();
        let e = fs.find("savedata.bin").unwrap();
        assert!(!e.is_dir());
        assert_eq!(fs.read(&e).unwrap(), fixtures::FATX_SAVE);
    }

    #[test]
    fn le_arquivo_que_atravessa_clusters() {
        // O arquivo grande tem mais de um cluster de 16 KiB, então este teste é o que
        // exercita o caminhamento da cadeia na FAT.
        let img = fixtures::fatx_minima();
        let fs = Fatx::open(&img).unwrap();
        let e = fs.find("TDATA/grande.bin").unwrap();
        let dados = fs.read(&e).unwrap();
        assert_eq!(dados.len(), fixtures::FATX_GRANDE_LEN);
        assert!(
            dados.len() > 16 * 1024,
            "o teste precisa de mais de um cluster"
        );
        for (i, b) in dados.iter().enumerate() {
            assert_eq!(*b, (i % 251) as u8, "byte {i} veio errado");
        }
    }

    #[test]
    fn leitura_parcial_atravessando_cluster() {
        let img = fixtures::fatx_minima();
        let fs = Fatx::open(&img).unwrap();
        let e = fs.find("TDATA/grande.bin").unwrap();
        let off = 16 * 1024 - 8;
        let pedaco = fs.read_range(&e, off as u64, 16).unwrap();
        for (i, b) in pedaco.iter().enumerate() {
            assert_eq!(*b, ((off + i) % 251) as u8);
        }
        assert!(fs.read_range(&e, 0, fixtures::FATX_GRANDE_LEN + 1).is_err());
    }

    #[test]
    fn entra_em_subdiretorio() {
        let img = fixtures::fatx_minima();
        let fs = Fatx::open(&img).unwrap();
        let tdata = fs.find("TDATA").unwrap();
        assert!(tdata.attributes & ATTR_DIRECTORY != 0);
        let filhos = fs.list(&tdata).unwrap();
        assert_eq!(filhos.len(), 1);
        assert_eq!(filhos[0].name, "grande.bin");
    }

    #[test]
    fn walk_lista_tudo_em_ordem_estavel() {
        let img = fixtures::fatx_minima();
        let fs = Fatx::open(&img).unwrap();
        let caminhos: Vec<String> = fs.walk().unwrap().into_iter().map(|(c, _)| c).collect();
        assert_eq!(
            caminhos,
            vec![
                "savedata.bin".to_owned(),
                "TDATA".to_owned(),
                "TDATA/grande.bin".to_owned(),
            ]
        );
    }

    #[test]
    fn funciona_sobre_arquivo_em_disco() {
        let img = fixtures::fatx_minima();
        let dir = std::env::temp_dir().join("obsidian-fatx-teste");
        std::fs::create_dir_all(&dir).unwrap();
        let caminho = dir.join("particao.img");
        std::fs::write(&caminho, &img).unwrap();

        let fs = Fatx::open_path(&caminho).expect("deve abrir do disco");
        assert_eq!(fs.root().unwrap().len(), 2);

        std::fs::remove_file(&caminho).ok();
    }

    #[test]
    fn particao_em_deslocamento() {
        let (img, inicio) = fixtures::fatx_em_deslocamento();
        let fs = Fatx::open_at(&img, inicio, 0).expect("deve abrir no deslocamento");
        assert_eq!(fs.root().unwrap().len(), 2);
        // Sem o deslocamento, a assinatura não está lá.
        assert!(Fatx::open(&img).is_err());
    }

    #[test]
    fn assinatura_errada_e_recusada() {
        let mut img = fixtures::fatx_minima();
        img[0] = b'Z';
        let err = Fatx::open(&img).unwrap_err();
        assert!(
            err.to_string().contains("não é uma partição FATX"),
            "veio: {err}"
        );
    }

    #[test]
    fn cluster_invalido_no_superbloco_e_recusado() {
        for valor in [0u32, 3, 7, 100] {
            let mut img = fixtures::fatx_minima();
            img[0x08..0x0C].copy_from_slice(&valor.to_le_bytes());
            assert!(
                Fatx::open(&img).is_err(),
                "cluster de {valor} setores foi aceito"
            );
        }
    }

    #[test]
    fn ciclo_na_cadeia_de_clusters_e_recusado() {
        let img = fixtures::fatx_com_ciclo();
        let fs = Fatx::open(&img).unwrap();
        let e = fs.find("savedata.bin").unwrap();
        let err = fs.read(&e).unwrap_err();
        assert!(err.to_string().contains("ciclo"), "veio: {err}");
    }

    #[test]
    fn particao_truncada_nao_causa_panico() {
        let img = fixtures::fatx_minima();
        for n in (0..img.len()).step_by(1021) {
            if let Ok(fs) = Fatx::open(&img[..n]) {
                let _ = fs.walk();
            }
        }
    }
}
