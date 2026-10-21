//! Referência UTC guest no power-on (ADR-0041). Só o Clock da sessão avança.

use xbox_core::error::SnapshotError;
use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_core::time::{CPU_HZ, Clock};
use xbox_mem::{Acesso, Bus, GuestVirt};

use super::{Atendimento, Erro, Resultado, ram_range};
use crate::imports::Call;

/// Data absoluta em intervalos de 100 ns desde 1601-01-01 UTC.
/// Não representa o RTC físico nem uma data extraída da EEPROM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemTimeReference {
    boot_ticks: u64,
}

impl Default for SystemTimeReference {
    fn default() -> Self {
        // Perfil sintético: 2000-01-01 00:00:00 UTC. Independente do host.
        Self {
            boot_ticks: 125_911_584_000_000_000,
        }
    }
}

impl SystemTimeReference {
    /// Configuração explícita no domínio não negativo de `LARGE_INTEGER`.
    pub const fn from_boot_ticks(ticks: i64) -> Option<Self> {
        if ticks < 0 {
            None
        } else {
            Some(Self {
                boot_ticks: ticks.cast_unsigned(),
            })
        }
    }

    pub const fn boot_ticks(self) -> u64 {
        self.boot_ticks
    }

    /// Conversão inteira sem perda intermediária por nanossegundos ou u32.
    /// Retorna None se a soma não couber no `LARGE_INTEGER` positivo.
    pub fn ticks_at(self, clock: &Clock) -> Option<u64> {
        let elapsed = (u128::from(clock.cycles()) * 10_000_000 / u128::from(CPU_HZ)) as u64;
        self.boot_ticks
            .checked_add(elapsed)
            .filter(|&ticks| i64::try_from(ticks).is_ok())
    }

    pub(super) fn query(self, bus: &mut Bus, call: &Call, clock: Option<&Clock>) -> Resultado {
        let clock = clock.ok_or(Erro::ContextoTemporal {
            detalhe: "a chamada exige o Clock explícito da sessão",
        })?;
        let ticks = self.ticks_at(clock).ok_or(Erro::ContextoTemporal {
            detalhe: "referência UTC mais tempo decorrido excede LARGE_INTEGER",
        })?;
        let output = call.argument(bus, 0)?;
        if !ram_range(bus, output, 8, Acesso::Escrita) {
            return Err(Erro::ArgumentoInvalido {
                nome: call.export.name,
                ordinal: call.export.ordinal,
                detalhe: "CurrentTime deve cobrir oito bytes em RAM guest gravável".to_owned(),
            });
        }
        bus.escrever_bytes(GuestVirt(output), &ticks.to_le_bytes())?;
        Ok(Atendimento { eax: 0 }) // VOID: não é NTSTATUS.
    }
}

impl Snapshot for SystemTimeReference {
    fn save(&self, out: &mut SnapshotWriter) {
        out.section("STIM", 1);
        out.u64(self.boot_ticks);
    }

    fn load(&mut self, inp: &mut SnapshotReader<'_>) -> Result<(), SnapshotError> {
        let bad = |what| SnapshotError::BadValue {
            section: "STIM",
            what,
        };
        if inp.section("STIM", 1)? != 1 {
            return Err(bad("versão de referência temporal desconhecida"));
        }
        let ticks = inp.u64()?;
        if ticks > i64::MAX as u64 {
            return Err(bad("referência temporal fora de LARGE_INTEGER positivo"));
        }
        *self = Self { boot_ticks: ticks };
        Ok(())
    }
}
