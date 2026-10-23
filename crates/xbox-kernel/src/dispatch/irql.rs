//! IRQL explícito da única CPU guest do perfil HLE (ADR-0042).
//! Consulta de estado, sem controlador de interrupções ou transições implícitas.

use xbox_core::error::SnapshotError;
use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};

/// Níveis públicos nxdk que este perfil pode representar.
/// Os níveis 3–31 permitem representar DIRQL sem normalizar o byte do guest.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Irql {
    /// Contexto inicial de execução do título, sem IRQ/APC/DPC entregue.
    #[default]
    Passive = 0,
    Apc = 1,
    Dispatch = 2,
    Device3 = 3,
    Device4 = 4,
    Device5 = 5,
    Device6 = 6,
    Device7 = 7,
    Device8 = 8,
    Device9 = 9,
    Device10 = 10,
    Device11 = 11,
    Device12 = 12,
    Device13 = 13,
    Device14 = 14,
    Device15 = 15,
    Device16 = 16,
    Device17 = 17,
    Device18 = 18,
    Device19 = 19,
    Device20 = 20,
    Device21 = 21,
    Device22 = 22,
    Device23 = 23,
    Device24 = 24,
    Device25 = 25,
    Device26 = 26,
    Device27 = 27,
    Device28 = 28,
    Device29 = 29,
    Device30 = 30,
    Device31 = 31,
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
            3 => Some(Self::Device3),
            4 => Some(Self::Device4),
            5 => Some(Self::Device5),
            6 => Some(Self::Device6),
            7 => Some(Self::Device7),
            8 => Some(Self::Device8),
            9 => Some(Self::Device9),
            10 => Some(Self::Device10),
            11 => Some(Self::Device11),
            12 => Some(Self::Device12),
            13 => Some(Self::Device13),
            14 => Some(Self::Device14),
            15 => Some(Self::Device15),
            16 => Some(Self::Device16),
            17 => Some(Self::Device17),
            18 => Some(Self::Device18),
            19 => Some(Self::Device19),
            20 => Some(Self::Device20),
            21 => Some(Self::Device21),
            22 => Some(Self::Device22),
            23 => Some(Self::Device23),
            24 => Some(Self::Device24),
            25 => Some(Self::Device25),
            26 => Some(Self::Device26),
            27 => Some(Self::Device27),
            28 => Some(Self::Device28),
            29 => Some(Self::Device29),
            30 => Some(Self::Device30),
            31 => Some(Self::Device31),
            _ => None,
        }
    }
}

impl Snapshot for Irql {
    fn save(&self, out: &mut SnapshotWriter) {
        out.section("IRQL", 2);
        out.u8(self.level());
    }

    fn load(&mut self, inp: &mut SnapshotReader<'_>) -> Result<(), SnapshotError> {
        let bad = |what| SnapshotError::BadValue {
            section: "IRQL",
            what,
        };
        let version = inp.section("IRQL", 2)?;
        if version == 0 {
            return Err(bad("versão de IRQL desconhecida"));
        }
        let level = inp.u8()?;
        if version == 1 && level > 2 {
            return Err(bad("nível fora do perfil legado"));
        }
        *self =
            Self::from_level(level).ok_or_else(|| bad("nível de IRQL sem modelo no perfil HLE"))?;
        Ok(())
    }
}
