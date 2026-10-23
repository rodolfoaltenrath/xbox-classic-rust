//! Arquivo de sessão ligado aos bytes exatos do XBE (ADR-0076).
//!
//! A restauração constrói outra Session: um arquivo inválido nunca altera a sessão
//! em uso. CRC detecta corrupção acidental, não autentica arquivos de terceiros.
//! Os formatos internos e suas migrações continuam pertencendo aos subsistemas.
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use xbox_core::error::SnapshotError;
use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};

use crate::runtime::Session;

/// Limite do arquivo inteiro, antes de alocar/carregar seu conteúdo (256 MiB).
pub const MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("arquivo de estado: {0}")]
    Invalid(&'static str),
    #[error(transparent)]
    Snapshot(#[from] SnapshotError),
    #[error(transparent)]
    Runtime(#[from] crate::Error),
    #[error("arquivo de estado: {0}")]
    Io(#[from] std::io::Error),
}

/// Serializa CPU, RAM, dispositivos, kernel e threads. `image` deve ser o XBE
/// original da sessão, nunca a RAM que o título pode ter modificado.
pub fn encode(session: &Session, image: &[u8]) -> Result<Vec<u8>, Error> {
    let mut state = SnapshotWriter::new();
    session.save(&mut state);
    let state = state.finish();
    if image.len().saturating_add(state.len()).saturating_add(40) as u64 > MAX_FILE_BYTES {
        return Err(Error::Invalid("excede o limite de 256 MiB"));
    }
    let mut out = SnapshotWriter::new();
    out.section("SAVE", 1);
    out.bytes(image);
    out.bytes(&state);
    out.u32(crc32(&state));
    Ok(out.finish())
}

fn bytes<'a>(reader: &mut SnapshotReader<'a>) -> Result<&'a [u8], Error> {
    let length = usize::try_from(reader.u64()?)
        .map_err(|_| Error::Invalid("tamanho não representável neste host"))?;
    Ok(reader.raw(length)?)
}

/// Só retorna a sessão depois de validar contêiner, imagem, integridade e todos
/// os subsistemas. Recusa bytes excedentes no contêiner e no snapshot interno.
pub fn decode(file: &[u8], image: &[u8]) -> Result<Session, Error> {
    if file.len() as u64 > MAX_FILE_BYTES {
        return Err(Error::Invalid("excede o limite de 256 MiB"));
    }
    let mut reader = SnapshotReader::new(file)?;
    if reader.section("SAVE", 1)? != 1 {
        return Err(Error::Invalid("versão SAVE desconhecida"));
    }
    if bytes(&mut reader)? != image {
        return Err(Error::Invalid("XBE diferente do executável recompilado"));
    }
    let state = bytes(&mut reader)?;
    if reader.u32()? != crc32(state) {
        return Err(Error::Invalid("CRC32 do estado não confere"));
    }
    if reader.remaining() != 0 {
        return Err(Error::Invalid("bytes excedentes no arquivo"));
    }
    let mut restored = Session::load(image)?;
    let mut reader = SnapshotReader::new(state)?;
    restored.load(&mut reader)?;
    if reader.remaining() != 0 {
        return Err(Error::Invalid("bytes excedentes no snapshot da sessão"));
    }
    Ok(restored)
}

/// Leitura limitada inclusive se o arquivo crescer depois de aberto.
pub fn load_file(path: &Path, image: &[u8]) -> Result<Session, Error> {
    let file = File::open(path)?;
    if file.metadata()?.len() > MAX_FILE_BYTES {
        return Err(Error::Invalid("excede o limite de 256 MiB"));
    }
    let mut data = Vec::new();
    file.take(MAX_FILE_BYTES + 1).read_to_end(&mut data)?;
    decode(&data, image)
}

/// Publica arquivo completo sem substituir destino existente. Temporário no
/// mesmo diretório, `sync_all` antes do hard link; falha de publicação conserva o
/// destino. Requer filesystem com hard links (NTFS/ext4); não há fallback destrutivo.
/// Não promete durabilidade do diretório contra perda de energia.
pub fn save_new(path: &Path, session: &Session, image: &[u8]) -> Result<(), Error> {
    publish(path, &encode(session, image)?)?;
    Ok(())
}

fn publish(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let (temp, mut file) = loop {
        let temp = parent.join(format!(
            ".obsidian-state-{}-{}.part",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        match OpenOptions::new().write(true).create_new(true).open(&temp) {
            Ok(file) => break (temp, file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    };
    let result = (|| {
        file.write_all(data)?;
        file.sync_all()?;
        drop(file);
        std::fs::hard_link(&temp, path)
    })();
    // Só remove o temporário criado por esta chamada. Um crash pode deixar .part;
    // esses arquivos não são candidatos a restauração automática.
    let _ = std::fs::remove_file(temp);
    result
}

const fn crc_table() -> [u32; 256] {
    let mut table = [0; 256];
    let mut i = 0;
    while i < 256 {
        let mut word = i as u32;
        let mut bit = 0;
        while bit < 8 {
            word = if word & 1 == 0 {
                word >> 1
            } else {
                (word >> 1) ^ 0xEDB88320
            };
            bit += 1;
        }
        table[i] = word;
        i += 1;
    }
    table
}

fn crc32(bytes: &[u8]) -> u32 {
    const TABLE: [u32; 256] = crc_table();
    !bytes.iter().fold(u32::MAX, |crc, &byte| {
        (crc >> 8) ^ TABLE[((crc ^ u32::from(byte)) & 255) as usize]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estado_completo_vinculado_a_imagem_e_restaurado_sem_mutar_original() {
        let image = xbox_formats::fixtures::xbe_com_codigo(crate::DEMO_CODE).unwrap();
        let mut session = Session::load(&image).unwrap();
        session.state.regs[0] = 0x12345678;
        session
            .bus
            .escrever_u32(xbox_mem::GuestVirt(0x12020), 0xABCD)
            .unwrap();
        let saved = encode(&session, &image).unwrap();
        let restored = decode(&saved, &image).unwrap();
        assert_eq!(restored.state, session.state);
        assert_eq!(encode(&restored, &image).unwrap(), saved);
        let mut different = image.clone();
        different[0x1000] ^= 1;
        assert!(
            decode(&saved, &different)
                .unwrap_err()
                .to_string()
                .contains("XBE diferente")
        );
        let mut corrupt = saved.clone();
        let at = corrupt.len() - 5;
        corrupt[at] ^= 1;
        assert!(
            decode(&corrupt, &image)
                .unwrap_err()
                .to_string()
                .contains("CRC32")
        );
        assert_eq!(encode(&session, &image).unwrap(), saved);
        for end in [0, 8, 12, 19, 20, 28, 40, image.len(), saved.len() - 1] {
            assert!(
                decode(&saved[..end], &image).is_err(),
                "aceitou corte {end}"
            );
        }
        let mut future = saved.clone();
        future[16..20].copy_from_slice(&2u32.to_le_bytes());
        assert!(decode(&future, &image).is_err());
        future[16..20].copy_from_slice(&0u32.to_le_bytes());
        assert!(decode(&future, &image).is_err());
        let mut extra = saved.clone();
        extra.push(0);
        assert!(
            decode(&extra, &image)
                .unwrap_err()
                .to_string()
                .contains("excedentes")
        );
        // Snapshot interno inválido com CRC válido também precisa ser recusado.
        let mut reader = SnapshotReader::new(&saved).unwrap();
        reader.section("SAVE", 1).unwrap();
        bytes(&mut reader).unwrap();
        let mut nested = bytes(&mut reader).unwrap().to_vec();
        nested.push(0);
        let mut writer = SnapshotWriter::new();
        writer.section("SAVE", 1);
        writer.bytes(&image);
        writer.bytes(&nested);
        writer.u32(crc32(&nested));
        assert!(
            decode(&writer.finish(), &image)
                .unwrap_err()
                .to_string()
                .contains("excedentes")
        );
    }

    #[test]
    fn leitura_limitada_recusa_arquivo_esparso_grande() {
        let path = std::env::temp_dir().join(format!(
            "obsidian-oversize-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        file.set_len(MAX_FILE_BYTES + 1).unwrap();
        drop(file);
        assert!(
            load_file(&path, b"irrelevante")
                .unwrap_err()
                .to_string()
                .contains("256 MiB")
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn crc32_conhecido_e_publicacao_sem_sobrescrita_ou_residuo() {
        assert_eq!(crc32(b"123456789"), 0xCBF43926);
        assert_eq!(crc32(b""), 0);
        let folder = std::env::temp_dir().join(format!(
            "obsidian-publish-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&folder).unwrap();
        let path = folder.join("estado com espaços.obsstate");
        publish(&path, b"original").unwrap();
        assert!(publish(&path, b"substituto").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"original");
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 1);
        assert!(publish(&folder.join("inexistente/a"), b"x").is_err());
        std::fs::remove_dir_all(folder).unwrap();
    }
}
