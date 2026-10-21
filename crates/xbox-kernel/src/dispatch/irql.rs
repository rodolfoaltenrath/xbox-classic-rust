//! IRQL explícito da única CPU guest do perfil HLE (ADR-0042).
//! Consulta de estado, sem controlador de interrupções ou transições implícitas.

use xbox_core::error::SnapshotError;
use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};

/// Níveis públicos nxdk que este perfil pode representar.
/// DIRQLs e demais níveis exigem ampliar o modelo de interrupções.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum Irql {
    /// Contexto inicial de execução do título, sem IRQ/APC/DPC entregue.
    #[default]
    Passive = 0,
    Apc = 1,
    Dispatch = 2,
}

impl Irql {
    /// Valor KIRQL (UCHAR) público do nível representado.
    pub const fn level(self) -> u8 {
        self as u8
    }

    /// Não mascarar nem normalizar níveis ainda sem representação neste perfil.
    pub const fn from_level(level: u8) -> Option<Self> {
        match level {
            0 => Some(Self::Passive),
            1 => Some(Self::Apc),
            2 => Some(Self::Dispatch),
            _ => None,
        }
    }
}

impl Snapshot for Irql {
    fn save(&self, out: &mut SnapshotWriter) {
        out.section("IRQL", 1);
        out.u8(self.level());
    }

    fn load(&mut self, inp: &mut SnapshotReader<'_>) -> Result<(), SnapshotError> {
        let bad = |what| SnapshotError::BadValue {
            section: "IRQL",
            what,
        };
        if inp.section("IRQL", 1)? != 1 {
            return Err(bad("versão de IRQL desconhecida"));
        }
        let level = inp.u8()?;
        *self =
            Self::from_level(level).ok_or_else(|| bad("nível de IRQL sem modelo no perfil HLE"))?;
        Ok(())
    }
}
