//! Memória de instância PRAMIN, perfil independente provisório (ADR-0060/P14).
//!
//! A janela de 1 MiB segue a referência pública xemu fixada no ADR. Sua relação
//! física com a RAM UMA ainda não é conhecida: isto não duplica o framebuffer
//! nem transforma a reserva CMEM do kernel em alias presumido de PRAMIN.
use xbox_core::{SnapshotReader, SnapshotWriter, error::SnapshotError};

pub const PRAMIN_BASE: u32 = 0x0070_0000;
pub const PRAMIN_SIZE: usize = 0x0010_0000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PraminProfile {
    /// BUS0 v1–v4: leituras zero e stores descartados com telemetria.
    LegacyUnmodeled,
    /// Armazenamento local do dispositivo; não afirma alias com RAM física.
    IndependentV1,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nv2aPramin {
    data: Option<Box<[u8]>>,
}
impl Default for Nv2aPramin {
    fn default() -> Self {
        Self {
            data: Some(vec![0; PRAMIN_SIZE].into_boxed_slice()),
        }
    }
}
impl Nv2aPramin {
    pub(crate) const fn legacy() -> Self {
        Self { data: None }
    }
    pub const fn profile(&self) -> PraminProfile {
        match self.data {
            Some(_) => PraminProfile::IndependentV1,
            None => PraminProfile::LegacyUnmodeled,
        }
    }
    /// Offset no NV2A, leitura alinhada de 32 bits. Compartilhada por aliases CPU
    /// e pelo futuro consumidor PFIFO; não executa comandos nem avança `DMA_GET`.
    pub fn read(&self, offset: u32) -> Option<u32> {
        let index = offset.checked_sub(PRAMIN_BASE)? as usize;
        if !index.is_multiple_of(4) {
            return None;
        }
        let bytes = self.data.as_ref()?.get(index..index.checked_add(4)?)?;
        Some(u32::from_le_bytes(bytes.try_into().expect("quatro bytes")))
    }
    pub(crate) fn validate_access(&self, offset: u32, len: usize) -> Result<bool, &'static str> {
        if self.data.is_none() || len == 0 {
            return Ok(false);
        }
        let start = u64::from(offset);
        let end = start.saturating_add(len as u64);
        if start >= u64::from(PRAMIN_BASE) + PRAMIN_SIZE as u64 || end <= u64::from(PRAMIN_BASE) {
            return Ok(false);
        }
        if offset >= PRAMIN_BASE
            && offset.is_multiple_of(4)
            && len == 4
            && end <= u64::from(PRAMIN_BASE) + PRAMIN_SIZE as u64
        {
            Ok(true)
        } else {
            Err("PRAMIN exige acesso alinhado de 32 bits dentro da janela (ADR-0060)")
        }
    }
    pub(crate) fn write(&mut self, offset: u32, value: u32) {
        let index = (offset - PRAMIN_BASE) as usize;
        self.data.as_mut().expect("perfil validado")[index..index + 4]
            .copy_from_slice(&value.to_le_bytes());
    }
    pub(crate) fn save(&self, out: &mut SnapshotWriter) {
        out.section("PRAM", 1);
        out.u32(u32::from(self.data.is_some()));
        out.bytes(self.data.as_deref().unwrap_or_default());
    }
    pub(crate) fn load(&mut self, inp: &mut SnapshotReader<'_>) -> Result<(), SnapshotError> {
        let bad = |what| SnapshotError::BadValue {
            section: "PRAM",
            what,
        };
        if inp.section("PRAM", 1)? != 1 {
            return Err(bad("versão desconhecida"));
        }
        let profile = inp.u32()?;
        if profile > 1 {
            return Err(bad("perfil desconhecido"));
        }
        let bytes = inp.bytes()?;
        let data = match profile {
            0 if bytes.is_empty() => None,
            1 if bytes.len() == PRAMIN_SIZE => Some(bytes.into()),
            _ => return Err(bad("tamanho incompatível com o perfil PRAMIN")),
        };
        self.data = data;
        Ok(())
    }
}
