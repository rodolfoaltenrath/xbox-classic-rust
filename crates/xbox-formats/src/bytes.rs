//! Leitura limitada de inteiros little-endian.
//!
//! Todo arquivo lido por este crate vem do usuário e pode estar truncado, corrompido
//! ou construído de má-fé. A regra do ROADMAP §13.3 é que nada disso pode causar
//! pânico — então nenhum acesso a `&[u8]` neste crate usa indexação direta: tudo passa
//! por estas funções, que devolvem [`Error::Format`] com a posição exata.
//!
//! [`Error::Format`]: xbox_core::Error::Format

use xbox_core::{Error, Result};

/// Fatia `len` bytes a partir de `off`, ou erro descrevendo o que faltou.
pub fn slice_at(data: &[u8], off: usize, len: usize) -> Result<&[u8]> {
    let fim = off
        .checked_add(len)
        .ok_or_else(|| Error::format(format!("deslocamento absurdo: {off} + {len}")))?;
    data.get(off..fim).ok_or_else(|| {
        Error::format(format!(
            "arquivo truncado: pedia {len} bytes em 0x{off:X}, arquivo tem 0x{:X}",
            data.len()
        ))
    })
}

/// Lê um `u8`.
pub fn u8_at(data: &[u8], off: usize) -> Result<u8> {
    Ok(slice_at(data, off, 1)?[0])
}

/// Lê um `u16` little-endian.
pub fn u16_at(data: &[u8], off: usize) -> Result<u16> {
    let b = slice_at(data, off, 2)?;
    Ok(u16::from_le_bytes([b[0], b[1]]))
}

/// Lê um `u32` little-endian.
pub fn u32_at(data: &[u8], off: usize) -> Result<u32> {
    let b = slice_at(data, off, 4)?;
    Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Lê um `u64` little-endian.
pub fn u64_at(data: &[u8], off: usize) -> Result<u64> {
    let b = slice_at(data, off, 8)?;
    Ok(u64::from_le_bytes([
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
    ]))
}

/// Lê uma string UTF-16LE de tamanho fixo (em caracteres), cortando no primeiro NUL.
pub fn utf16_at(data: &[u8], off: usize, chars: usize) -> Result<String> {
    let b = slice_at(data, off, chars * 2)?;
    let (pares, _) = b.as_chunks::<2>();
    let unidades: Vec<u16> = pares
        .iter()
        .map(|&par| u16::from_le_bytes(par))
        .take_while(|&u| u != 0)
        .collect();
    Ok(String::from_utf16_lossy(&unidades))
}

/// Lê uma string de bytes de tamanho fixo, cortando no primeiro NUL.
///
/// Os nomes em XBE/XDVDFS/FATX não declaram codificação. Na prática são ASCII; bytes
/// altos são interpretados como Latin-1, que nunca falha e preserva o byte original de
/// forma reversível.
pub fn latin1_at(data: &[u8], off: usize, len: usize) -> Result<String> {
    let b = slice_at(data, off, len)?;
    Ok(latin1(b))
}

/// Converte bytes em `String` tratando-os como Latin-1, parando no primeiro NUL.
pub fn latin1(b: &[u8]) -> String {
    b.iter()
        .copied()
        .take_while(|&c| c != 0)
        .map(char::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{latin1, slice_at, u32_at, utf16_at};

    #[test]
    fn leitura_fora_do_fim_e_erro_nao_panico() {
        let d = [1u8, 2, 3];
        assert!(slice_at(&d, 2, 4).is_err());
        assert!(u32_at(&d, 0).is_err());
        assert!(u32_at(&d, usize::MAX).is_err());
    }

    #[test]
    fn le_inteiros_little_endian() {
        let d = [0x78, 0x56, 0x34, 0x12];
        assert_eq!(u32_at(&d, 0).unwrap(), 0x1234_5678);
    }

    #[test]
    fn utf16_corta_no_nul() {
        let mut d = Vec::new();
        for c in "Halo\0\0".encode_utf16() {
            d.extend_from_slice(&c.to_le_bytes());
        }
        assert_eq!(utf16_at(&d, 0, 6).unwrap(), "Halo");
    }

    #[test]
    fn latin1_preserva_bytes_altos() {
        assert_eq!(latin1(b"caf\xe9\0ignorado"), "café");
    }
}
