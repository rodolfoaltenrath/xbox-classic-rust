//! Origem dos bytes de uma mídia: fatia em memória ou arquivo em disco.
//!
//! ## Por que isto existe
//!
//! A primeira versão dos parsers recebia `&[u8]` do arquivo inteiro. Funciona para
//! fixture de 80 KB e **não funciona** para a realidade: uma imagem de jogo tem de 2 a
//! 7 GB, e ler 2,87 GB na RAM para inspecionar um setor de 2 KB é inaceitável — tanto
//! na ferramenta quanto, mais tarde, no emulador, onde o `NtReadFile` do jogo lê
//! pedaços sob demanda durante a gameplay.
//!
//! ## Zero-copy onde dá, cópia onde é inevitável
//!
//! [`Media::read_at`] devolve [`Cow`]:
//!
//! - origem [`Media::Slice`] (fixture em teste, e no futuro um arquivo mapeado em
//!   memória) devolve `Cow::Borrowed` — **nenhuma cópia**;
//! - origem [`Media::File`] devolve `Cow::Owned` — a cópia é o preço de não ter o
//!   arquivo inteiro em memória.
//!
//! Assim existe **uma** implementação do formato, e não uma versão "de teste" e outra
//! "de produção" que divergem com o tempo.

use std::borrow::Cow;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use xbox_core::{Error, Result};

/// De onde vêm os bytes de uma mídia.
#[derive(Debug)]
pub enum Media<'a> {
    /// Bytes já em memória.
    Slice(&'a [u8]),
    /// Arquivo lido sob demanda.
    ///
    /// O `Mutex` existe porque os métodos de leitura recebem `&self` enquanto
    /// `Seek`/`Read` exigem `&mut File`. Posicionar e ler sob lock é suficiente aqui
    /// (leitura de setor é rara); o caminho quente da Fase 4 vai querer leitura
    /// posicionada de verdade, e isso é trabalho de `xbox-platform`.
    File {
        /// Arquivo aberto.
        arquivo: Mutex<File>,
        /// Tamanho total, consultado uma vez na abertura.
        tamanho: u64,
        /// Caminho, só para mensagens de erro.
        caminho: PathBuf,
    },
}

impl<'a> Media<'a> {
    /// Mídia a partir de bytes em memória.
    pub const fn from_slice(dados: &'a [u8]) -> Self {
        Self::Slice(dados)
    }

    /// Abre um arquivo para leitura sob demanda.
    pub fn open(caminho: impl AsRef<Path>) -> Result<Self> {
        let caminho = caminho.as_ref();
        let arquivo = File::open(caminho).map_err(|e| Error::io_path(caminho, e))?;
        let tamanho = arquivo
            .metadata()
            .map_err(|e| Error::io_path(caminho, e))?
            .len();
        Ok(Self::File {
            arquivo: Mutex::new(arquivo),
            tamanho,
            caminho: caminho.to_path_buf(),
        })
    }

    /// Tamanho total da mídia, em bytes.
    pub fn len(&self) -> u64 {
        match self {
            Self::Slice(d) => d.len() as u64,
            Self::File { tamanho, .. } => *tamanho,
        }
    }

    /// A mídia está vazia?
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Nome da mídia, para mensagens.
    pub fn nome(&self) -> String {
        match self {
            Self::Slice(d) => format!("<{} bytes em memória>", d.len()),
            Self::File { caminho, .. } => caminho.display().to_string(),
        }
    }

    /// Lê `len` bytes a partir de `off`.
    ///
    /// Erra — sem alocar — quando a faixa passa do fim da mídia. Isso é o que impede
    /// que um campo de tamanho corrompido vire uma alocação de gigabytes.
    pub fn read_at(&self, off: u64, len: usize) -> Result<Cow<'a, [u8]>> {
        let fim = off
            .checked_add(len as u64)
            .ok_or_else(|| Error::format(format!("faixa absurda: {off} + {len}")))?;
        if fim > self.len() {
            return Err(Error::format(format!(
                "leitura fora da mídia: pedia {len} bytes em 0x{off:X}, mídia tem 0x{:X} ({})",
                self.len(),
                self.nome()
            )));
        }

        match self {
            Self::Slice(dados) => {
                let inicio = usize::try_from(off)
                    .map_err(|_| Error::format("deslocamento não cabe em usize"))?;
                let fim = usize::try_from(fim)
                    .map_err(|_| Error::format("deslocamento não cabe em usize"))?;
                Ok(Cow::Borrowed(&dados[inicio..fim]))
            }
            Self::File {
                arquivo, caminho, ..
            } => {
                let mut guarda = arquivo
                    .lock()
                    .map_err(|_| Error::format("mídia envenenada por pânico em outra thread"))?;
                guarda
                    .seek(SeekFrom::Start(off))
                    .map_err(|e| Error::io_path(caminho, e))?;
                let mut buf = vec![0u8; len];
                guarda
                    .read_exact(&mut buf)
                    .map_err(|e| Error::io_path(caminho, e))?;
                Ok(Cow::Owned(buf))
            }
        }
    }
}

impl<'a> From<&'a [u8]> for Media<'a> {
    fn from(d: &'a [u8]) -> Self {
        Self::Slice(d)
    }
}

impl<'a> From<&'a Vec<u8>> for Media<'a> {
    fn from(d: &'a Vec<u8>) -> Self {
        Self::Slice(d)
    }
}

#[cfg(test)]
mod tests {
    use super::Media;

    #[test]
    fn slice_nao_copia() {
        let dados = vec![1u8, 2, 3, 4, 5];
        let m = Media::from_slice(&dados);
        let lido = m.read_at(1, 3).unwrap();
        assert!(
            matches!(lido, std::borrow::Cow::Borrowed(_)),
            "slice deveria emprestar"
        );
        assert_eq!(&*lido, &[2, 3, 4]);
    }

    #[test]
    fn leitura_fora_da_midia_erra_sem_alocar() {
        let m = Media::from_slice(&[0u8; 10]);
        assert!(m.read_at(5, 10).is_err());
        assert!(m.read_at(0, usize::MAX).is_err());
        assert!(m.read_at(u64::MAX, 1).is_err());
    }

    #[test]
    fn arquivo_le_sob_demanda() {
        let dir = std::env::temp_dir().join("obsidian-media-teste");
        std::fs::create_dir_all(&dir).unwrap();
        let caminho = dir.join("midia.bin");
        let conteudo: Vec<u8> = (0..=255u8).collect();
        std::fs::write(&caminho, &conteudo).unwrap();

        let m = Media::open(&caminho).unwrap();
        assert_eq!(m.len(), 256);
        let lido = m.read_at(200, 10).unwrap();
        assert!(
            matches!(lido, std::borrow::Cow::Owned(_)),
            "arquivo deveria copiar"
        );
        assert_eq!(&*lido, &conteudo[200..210]);
        assert!(m.read_at(250, 10).is_err(), "além do fim deve errar");

        std::fs::remove_file(&caminho).ok();
    }
}
