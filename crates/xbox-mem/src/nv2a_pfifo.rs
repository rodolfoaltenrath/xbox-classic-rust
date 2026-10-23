//! PFIFO programável e consumidor de comandos (ADRs 0063–0065).
mod commands;
mod persistence;
use commands::CommandState;
pub use commands::{GpuFault, MethodRecord, StepReport};
use xbox_core::{SnapshotReader, SnapshotWriter, error::SnapshotError};

// Offsets absolutos NV2A observados até o primeiro GET do triangle, ADR-0063.
const REGISTERS: [u32; 37] = [
    0x2040, 0x2044, 0x204C, 0x2100, 0x2140, 0x2210, 0x2214, 0x2410, 0x2420, 0x2500, 0x2504, 0x2508,
    0x250C, 0x3000, 0x3050, 0x3058, 0x3200, 0x3204, 0x3210, 0x3220, 0x3224, 0x3228, 0x322C, 0x3230,
    0x3240, 0x3244, 0x3248, 0x324C, 0x3250, 0x3254, 0x3258, 0x3260, 0x3264, 0x3268, 0x326C, 0x3270,
    0x3280,
];
const USER: u32 = 0x0080_0000;
pub const DMA_PUT: u32 = 0x3240;
pub const DMA_GET: u32 = 0x3244;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PfifoProfile {
    LegacyUnmodeled,
    /// Controles programáveis; não afirma estado de reset físico ou avanço DMA.
    RegistersV1,
    CommandsV1,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nv2aPfifo {
    registers: Option<[u32; REGISTERS.len()]>,
    commands: Option<CommandState>,
}
impl Default for Nv2aPfifo {
    fn default() -> Self {
        Self {
            registers: Some([0; REGISTERS.len()]),
            commands: Some(CommandState::default()),
        }
    }
}
impl Nv2aPfifo {
    pub(crate) const fn legacy() -> Self {
        Self {
            registers: None,
            commands: None,
        }
    }
    pub const fn profile(&self) -> PfifoProfile {
        match self.registers {
            Some(_) => {
                if self.commands.is_some() {
                    PfifoProfile::CommandsV1
                } else {
                    PfifoProfile::RegistersV1
                }
            }
            None => PfifoProfile::LegacyUnmodeled,
        }
    }
    /// Inspeção do controle, sem efeitos nem cópia de RAMFC/PRAMIN.
    pub fn register(&self, offset: u32) -> Option<u32> {
        let index = REGISTERS.binary_search(&offset).ok()?;
        Some(self.registers.as_ref()?[index])
    }
    fn user_register(&self, offset: u32) -> Result<Option<u32>, &'static str> {
        if !(USER..0x0100_0000).contains(&offset) {
            return Ok(None);
        }
        let word = offset & !3;
        let mapped = match word & 0xFFFF {
            0x40 => DMA_PUT,
            0x44 => DMA_GET,
            _ => return Ok(None),
        };
        let channel = (word - USER) >> 16;
        if channel != 0
            || self.register(0x3204).is_none_or(|v| v & 0x11F != 0x100)
            || self.register(0x2504).is_none_or(|v| v & 1 == 0)
        {
            return Err("NV_USER exige canal DMA zero selecionado neste perfil PFIFO");
        }
        Ok(Some(mapped))
    }
    fn derived(&self, word: u32) -> Option<u32> {
        if self.commands.is_none() || !matches!(word, 0x2400 | 0x3214) {
            return None;
        }
        let drained = self.register(DMA_GET) == self.register(DMA_PUT)
            && self.register(0x3228).is_some_and(|v| v & 0x1FFC0000 == 0);
        Some(if drained { 0x10 } else { 0 })
    }
    pub(crate) fn validate_access(
        &self,
        offset: u32,
        len: usize,
        write: bool,
    ) -> Result<bool, &'static str> {
        if self.registers.is_none() || len == 0 {
            return Ok(false);
        }
        let word = offset & !3;
        let user = self.user_register(offset)?;
        let derived = self.derived(word).is_some();
        if derived && write {
            return Err("estado de fila PFIFO somente leitura");
        }
        let known = derived || user.is_some() || REGISTERS.binary_search(&word).is_ok();
        if !known {
            let start = u64::from(offset);
            let end = start.saturating_add(len as u64);
            if REGISTERS
                .iter()
                .any(|&r| start < u64::from(r) + 4 && u64::from(r) < end)
                || (start < u64::from(USER + 0x48) && u64::from(USER + 0x40) < end)
            {
                return Err("acesso atravessa controle PFIFO modelado");
            }
            return Ok(false);
        }
        if (write && (len != 4 || offset != word))
            || (!write && (!matches!(len, 1 | 2 | 4) || !offset.is_multiple_of(len as u32)))
            || (offset & 3) as usize + len > 4
        {
            return Err(
                "PFIFO exige store DWORD alinhado ou leitura BYTE/WORD/DWORD alinhada dentro do controle",
            );
        }
        Ok(true)
    }
    pub(crate) fn read(&self, offset: u32, len: usize) -> Result<Option<u32>, &'static str> {
        if !self.validate_access(offset, len, false)? {
            return Ok(None);
        }
        let word = self.user_register(offset)?.unwrap_or(offset & !3);
        Ok(self
            .register(word)
            .or_else(|| self.derived(word))
            .map(|v| v >> ((offset & 3) * 8)))
    }
    pub(crate) fn write(&mut self, offset: u32, value: u32) {
        let word = self
            .user_register(offset)
            .expect("acesso validado")
            .unwrap_or(offset);
        let index = REGISTERS.binary_search(&word).expect("controle validado");
        let registers = self.registers.as_mut().expect("perfil validado");
        if word == 0x2100 {
            registers[index] &= !value;
        }
        // W1C; sem fonte de IRQ PFIFO ainda.
        else {
            registers[index] = value;
        }
    }
    pub(crate) fn save(&self, out: &mut SnapshotWriter) {
        out.section("PFFO", 2);
        out.u32(if self.commands.is_some() {
            2
        } else {
            u32::from(self.registers.is_some())
        });
        out.u32(if self.registers.is_some() {
            REGISTERS.len() as u32
        } else {
            0
        });
        if let Some(registers) = self.registers {
            for value in registers {
                out.u32(value);
            }
        }
        if let Some(commands) = &self.commands {
            let mut data = SnapshotWriter::new();
            commands.save(&mut data);
            out.bytes(&data.finish());
        }
    }
    pub(crate) fn load(&mut self, inp: &mut SnapshotReader<'_>) -> Result<(), SnapshotError> {
        let bad = |what| SnapshotError::BadValue {
            section: "PFFO",
            what,
        };
        let version = inp.section("PFFO", 2)?;
        if !matches!(version, 1 | 2) {
            return Err(bad("versão PFIFO desconhecida"));
        }
        let profile = inp.u32()?;
        let count = inp.u32()?;
        let registers = match (profile, count) {
            (0, 0) => None,
            (1 | 2, n) if n == REGISTERS.len() as u32 && (profile != 2 || version == 2) => {
                let mut values = [0; REGISTERS.len()];
                for value in &mut values {
                    *value = inp.u32()?;
                }
                if values[REGISTERS.binary_search(&0x2100).expect("INTR_0")] != 0 {
                    return Err(bad("pendência PFIFO sem fonte de interrupção modelada"));
                }
                Some(values)
            }
            _ => return Err(bad("perfil ou contagem de controles PFIFO inválida")),
        };
        let commands = if profile == 2 {
            let bytes = inp.bytes()?;
            if bytes.len() > 160000 {
                return Err(bad("estado de comandos excessivo"));
            }
            Some(CommandState::load(&mut SnapshotReader::new(bytes)?)?)
        } else {
            None
        };
        self.registers = registers;
        self.commands = commands;
        Ok(())
    }
}
