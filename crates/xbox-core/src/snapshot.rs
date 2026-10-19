//! Serialização determinística de estado emulado.
//!
//! Implementa a **Decisão 4** do ROADMAP (§4.4): save state, *quick resume* e replay
//! determinístico são requisitos de arquitetura, não remendos posteriores. Todo estado
//! emulado implementa [`Snapshot`] desde o começo.
//!
//! ## Por que não `serde` aqui
//!
//! `serde` é ótimo para configuração (e é o que usamos em [`crate::config`]), mas para
//! estado de emulação queremos:
//!
//! - **formato binário estável e explícito**, com seções versionadas individualmente,
//!   para que um save state antigo possa ser lido quando só um subsistema mudou;
//! - **zero mágica**: o que é escrito está visível no código, byte a byte;
//! - **sem ponteiros**: a regra do §4.4 é que nenhuma estrutura do guest guarde ponteiro
//!   do host. Escrever o snapshot à mão é o que torna uma violação dessa regra óbvia —
//!   se você não consegue serializar, é porque guardou um ponteiro.
//!
//! ## Formato
//!
//! ```text
//! "OBSIDSNP"  (8 bytes)  assinatura
//! u32                    versão do container
//! [ seção ]*             sequência de seções
//!
//! seção := tag[4] | u32 versão | payload definido pelo subsistema
//! ```
//!
//! Inteiros são little-endian (o host é x86, o guest é x86; não há conversão).

use crate::error::SnapshotError;

/// Assinatura no início de todo snapshot.
const MAGIC: &[u8; 8] = b"OBSIDSNP";

/// Versão do container. Muda apenas se a estrutura de seções mudar.
pub const FORMAT_VERSION: u32 = 1;

/// Estado emulado que pode ser salvo e restaurado.
///
/// Implementações devem ser **determinísticas**: salvar, restaurar e salvar de novo
/// precisa produzir bytes idênticos. Isso é verificado pelos testes de replay
/// (ROADMAP §9).
pub trait Snapshot {
    /// Escreve o estado. Infalível por construção: o destino é um `Vec` em memória.
    fn save(&self, out: &mut SnapshotWriter);

    /// Restaura o estado, substituindo o atual.
    ///
    /// Em caso de erro, o objeto pode ficar em estado parcialmente restaurado — o
    /// chamador deve descartar a sessão inteira, não tentar continuar.
    fn load(&mut self, inp: &mut SnapshotReader<'_>) -> Result<(), SnapshotError>;
}

/// Acumula os bytes de um snapshot.
#[derive(Debug)]
pub struct SnapshotWriter {
    buf: Vec<u8>,
}

impl SnapshotWriter {
    /// Cria um writer já com o cabeçalho do container.
    pub fn new() -> Self {
        let mut buf = Vec::with_capacity(64 * 1024);
        buf.extend_from_slice(MAGIC);
        buf.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        Self { buf }
    }

    /// Abre uma seção. `tag` deve ter exatamente 4 bytes ASCII.
    pub fn section(&mut self, tag: &'static str, version: u32) {
        debug_assert_eq!(tag.len(), 4, "tag de seção precisa de 4 bytes: {tag:?}");
        self.buf.extend_from_slice(tag.as_bytes());
        self.buf.extend_from_slice(&version.to_le_bytes());
    }

    /// Escreve um `bool` como um byte 0/1.
    pub fn bool(&mut self, v: bool) {
        self.buf.push(u8::from(v));
    }

    /// Escreve um `u8`.
    pub fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }

    /// Escreve um `u16` little-endian.
    pub fn u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    /// Escreve um `u32` little-endian.
    pub fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    /// Escreve um `u64` little-endian.
    pub fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    /// Escreve um `i32` little-endian.
    pub fn i32(&mut self, v: i32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    /// Escreve um bloco de bytes precedido pelo seu tamanho (`u64`).
    pub fn bytes(&mut self, v: &[u8]) {
        self.u64(v.len() as u64);
        self.buf.extend_from_slice(v);
    }

    /// Escreve um bloco de bytes de tamanho fixo, sem prefixo de tamanho.
    ///
    /// Usado para a arena de memória, onde o tamanho é conhecido por construção e um
    /// prefixo de 8 bytes a cada 64 MiB seria desperdício de clareza.
    pub fn raw(&mut self, v: &[u8]) {
        self.buf.extend_from_slice(v);
    }

    /// Bytes escritos até agora.
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// Sempre `false`: o cabeçalho já ocupa bytes. Existe para satisfazer o lint.
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Consome o writer e devolve os bytes do snapshot.
    pub fn finish(self) -> Vec<u8> {
        self.buf
    }
}

impl Default for SnapshotWriter {
    fn default() -> Self {
        Self::new()
    }
}

/// Lê um snapshot produzido por [`SnapshotWriter`].
#[derive(Debug)]
pub struct SnapshotReader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> SnapshotReader<'a> {
    /// Valida o cabeçalho do container e posiciona a leitura na primeira seção.
    pub fn new(buf: &'a [u8]) -> Result<Self, SnapshotError> {
        if buf.len() < MAGIC.len() + 4 || &buf[..MAGIC.len()] != MAGIC {
            return Err(SnapshotError::BadMagic);
        }
        let mut r = Self {
            buf,
            pos: MAGIC.len(),
        };
        let found = r.u32()?;
        if found != FORMAT_VERSION {
            return Err(SnapshotError::BadFormatVersion {
                found,
                expected: FORMAT_VERSION,
            });
        }
        Ok(r)
    }

    /// Consome o cabeçalho da seção esperada e devolve a versão gravada.
    ///
    /// O subsistema decide o que fazer com uma versão antiga (migrar) — mas uma versão
    /// **mais nova** que `max_version` é sempre erro: não se adivinha o futuro.
    pub fn section(&mut self, tag: &'static str, max_version: u32) -> Result<u32, SnapshotError> {
        debug_assert_eq!(tag.len(), 4, "tag de seção precisa de 4 bytes: {tag:?}");
        let found = self.take(4)?;
        if found != tag.as_bytes() {
            return Err(SnapshotError::BadSection {
                expected: tag,
                found: String::from_utf8_lossy(found).into_owned(),
            });
        }
        let version = self.u32()?;
        if version > max_version {
            return Err(SnapshotError::SectionTooNew {
                section: tag,
                found: version,
                max: max_version,
            });
        }
        Ok(version)
    }

    /// Lê um `bool`.
    pub fn bool(&mut self) -> Result<bool, SnapshotError> {
        Ok(self.u8()? != 0)
    }

    /// Lê um `u8`.
    pub fn u8(&mut self) -> Result<u8, SnapshotError> {
        Ok(self.take(1)?[0])
    }

    /// Lê um `u16`.
    pub fn u16(&mut self) -> Result<u16, SnapshotError> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    /// Lê um `u32`.
    pub fn u32(&mut self) -> Result<u32, SnapshotError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// Lê um `u64`.
    pub fn u64(&mut self) -> Result<u64, SnapshotError> {
        let b = self.take(8)?;
        Ok(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    /// Lê um `i32`.
    pub fn i32(&mut self) -> Result<i32, SnapshotError> {
        let b = self.take(4)?;
        Ok(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// Lê um bloco escrito por [`SnapshotWriter::bytes`].
    pub fn bytes(&mut self) -> Result<&'a [u8], SnapshotError> {
        let len = self.u64()? as usize;
        self.take(len)
    }

    /// Lê `len` bytes crus, como escritos por [`SnapshotWriter::raw`].
    pub fn raw(&mut self, len: usize) -> Result<&'a [u8], SnapshotError> {
        self.take(len)
    }

    /// Bytes ainda não lidos.
    pub fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], SnapshotError> {
        let end = self.pos.checked_add(n).ok_or(SnapshotError::Truncated {
            pos: self.pos,
            needed: n,
        })?;
        if end > self.buf.len() {
            return Err(SnapshotError::Truncated {
                pos: self.pos,
                needed: end - self.buf.len(),
            });
        }
        let slice = &self.buf[self.pos..end];
        self.pos = end;
        Ok(slice)
    }
}

#[cfg(test)]
mod tests {
    use super::{Snapshot, SnapshotReader, SnapshotWriter};
    use crate::error::SnapshotError;

    #[derive(Debug, PartialEq, Eq)]
    struct Exemplo {
        contador: u64,
        ligado: bool,
        nome: Vec<u8>,
    }

    impl Snapshot for Exemplo {
        fn save(&self, out: &mut SnapshotWriter) {
            out.section("EXMP", 1);
            out.u64(self.contador);
            out.bool(self.ligado);
            out.bytes(&self.nome);
        }

        fn load(&mut self, inp: &mut SnapshotReader<'_>) -> Result<(), SnapshotError> {
            let _v = inp.section("EXMP", 1)?;
            self.contador = inp.u64()?;
            self.ligado = inp.bool()?;
            self.nome = inp.bytes()?.to_vec();
            Ok(())
        }
    }

    fn exemplo() -> Exemplo {
        Exemplo {
            contador: 0xDEAD_BEEF_CAFE_0001,
            ligado: true,
            nome: b"halo".to_vec(),
        }
    }

    #[test]
    fn ida_e_volta_preserva_o_estado() {
        let original = exemplo();
        let mut w = SnapshotWriter::new();
        original.save(&mut w);
        let bytes = w.finish();

        let mut restaurado = Exemplo {
            contador: 0,
            ligado: false,
            nome: Vec::new(),
        };
        let mut r = SnapshotReader::new(&bytes).expect("cabeçalho válido");
        restaurado.load(&mut r).expect("load");

        assert_eq!(original, restaurado);
        assert_eq!(r.remaining(), 0, "sobrou lixo no fim do snapshot");
    }

    #[test]
    fn salvar_duas_vezes_produz_bytes_identicos() {
        // Este é o teste que protege a Decisão 4.4: snapshot não determinístico
        // inviabiliza replay e comparação de regressão.
        let e = exemplo();
        let mut a = SnapshotWriter::new();
        e.save(&mut a);
        let mut b = SnapshotWriter::new();
        e.save(&mut b);
        assert_eq!(a.finish(), b.finish());
    }

    #[test]
    fn assinatura_errada_e_rejeitada() {
        let err = SnapshotReader::new(b"nada disso aqui").unwrap_err();
        assert!(matches!(err, SnapshotError::BadMagic), "veio {err:?}");
    }

    #[test]
    fn snapshot_truncado_e_rejeitado_sem_panico() {
        let mut w = SnapshotWriter::new();
        exemplo().save(&mut w);
        let bytes = w.finish();
        let cortado = &bytes[..bytes.len() - 3];

        let mut r = SnapshotReader::new(cortado).expect("cabeçalho válido");
        let mut destino = Exemplo {
            contador: 0,
            ligado: false,
            nome: Vec::new(),
        };
        let err = destino.load(&mut r).unwrap_err();
        assert!(
            matches!(err, SnapshotError::Truncated { .. }),
            "veio {err:?}"
        );
    }

    #[test]
    fn secao_inesperada_e_rejeitada() {
        let mut w = SnapshotWriter::new();
        w.section("OUTR", 1);
        w.u64(1);
        let bytes = w.finish();

        let mut r = SnapshotReader::new(&bytes).expect("cabeçalho válido");
        let err = r.section("EXMP", 1).unwrap_err();
        match err {
            SnapshotError::BadSection { expected, found } => {
                assert_eq!(expected, "EXMP");
                assert_eq!(found, "OUTR");
            }
            other => panic!("veio {other:?}"),
        }
    }

    #[test]
    fn secao_de_build_mais_nova_e_rejeitada() {
        let mut w = SnapshotWriter::new();
        w.section("EXMP", 99);
        let bytes = w.finish();

        let mut r = SnapshotReader::new(&bytes).expect("cabeçalho válido");
        let err = r.section("EXMP", 1).unwrap_err();
        assert!(
            matches!(
                err,
                SnapshotError::SectionTooNew {
                    found: 99,
                    max: 1,
                    ..
                }
            ),
            "veio {err:?}"
        );
    }
}
