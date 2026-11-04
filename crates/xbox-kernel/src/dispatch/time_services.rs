//! Serviços de tempo que só dependem do Clock da sessão ou de aritmética de calendário
//! (ADR-0101): `KeQueryPerformanceCounter`, `KeQueryPerformanceFrequency`,
//! `RtlTimeToTimeFields` e `RtlTimeFieldsToTime`.
//!
//! O contador de performance e a frequência derivam do **mesmo Clock único** do RDTSC e do
//! `KeTickCount`, na frequência de 3,375 MHz do relógio ACPI do console. Essa constante é a
//! que o Cxbx-Reloaded fixado usa para as duas funções (`XBOX_ACPI_FREQUENCY = 3375000`) e a
//! que o projeto já tinha em `xbox_core::time::PERF_COUNTER_HZ`; o nxdk fixado só declara
//! `ULONGLONG f(void)`.
//!
//! As conversões de calendário são o calendário gregoriano proléptico contado em intervalos
//! de 100 ns desde 1601-01-01 UTC, que é a definição pública de `LARGE_INTEGER` de tempo do
//! NT; o `xboxkrnl.h` fixado documenta apenas os campos de `TIME_FIELDS`.

use xbox_core::time::{Clock, PERF_COUNTER_HZ};
use xbox_mem::{Acesso, Bus, GuestVirt};

use super::{Atendimento, Erro, Resultado, ram_range};
use crate::imports::Call;

const TICKS_PER_MS: i64 = 10_000;
const TICKS_PER_SECOND: i64 = 10_000_000;
const TICKS_PER_DAY: i64 = 864_000_000_000;
/// Dia da semana de 1601-01-01 (segunda-feira), com 0 = domingo como no `TIME_FIELDS`.
const EPOCH_WEEKDAY: i64 = 1;

/// Campos de `TIME_FIELDS`, na ordem do struct: ano, mês, dia, hora, minuto, segundo,
/// milissegundos e dia da semana.
pub type Fields = [i16; 8];

const fn is_leap(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

const fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if is_leap(year) => 29,
        _ => 28,
    }
}

/// Dias desde 1601-01-01 até o dia 1 do mês, com `year >= 1601`.
const fn days_before(year: i64, month: i64) -> i64 {
    let y = year - 1601;
    // Anos completos: 365 por ano mais os bissextos em [1601, year). O primeiro bissexto
    // (1604) tem deslocamento 3, o primeiro século não bissexto (1700) tem 99 e o primeiro
    // múltiplo de 400 (2000) tem 399; por isso as contagens são y/4, y/100 e y/400.
    let mut days = y * 365 + y / 4 - y / 100 + y / 400;
    let mut m = 1;
    while m < month {
        days += days_in_month(year, m);
        m += 1;
    }
    days
}

/// `RtlTimeFieldsToTime`: `None` se algum campo estiver fora de faixa. O dia da semana é
/// ignorado, como no NT.
pub fn fields_to_ticks(fields: Fields) -> Option<i64> {
    let [year, month, day, hour, minute, second, ms, _weekday] = fields.map(i64::from);
    if year < 1601
        || !(1..=12).contains(&month)
        || !(1..=days_in_month(year, month)).contains(&day)
        || !(0..24).contains(&hour)
        || !(0..60).contains(&minute)
        || !(0..60).contains(&second)
        || !(0..1000).contains(&ms)
    {
        return None;
    }
    let days = days_before(year, month) + day - 1;
    days.checked_mul(TICKS_PER_DAY)?.checked_add(
        hour * 3600 * TICKS_PER_SECOND
            + minute * 60 * TICKS_PER_SECOND
            + second * TICKS_PER_SECOND
            + ms * TICKS_PER_MS,
    )
}

/// `RtlTimeToTimeFields`: `None` para tempo negativo ou que não cabe em um `SHORT` de ano.
pub fn ticks_to_fields(ticks: i64) -> Option<Fields> {
    if ticks < 0 {
        return None;
    }
    let days = ticks / TICKS_PER_DAY;
    let in_day = ticks % TICKS_PER_DAY;
    // Ano: parte de uma estimativa e ajusta, sem depender de divisões aproximadas.
    let mut year = 1601 + days * 400 / 146_097;
    while days_before(year + 1, 1) <= days {
        year += 1;
    }
    while days_before(year, 1) > days {
        year -= 1;
    }
    let mut rest = days - days_before(year, 1);
    let mut month = 1;
    while rest >= days_in_month(year, month) {
        rest -= days_in_month(year, month);
        month += 1;
    }
    let fields = [
        year,
        month,
        rest + 1,
        in_day / (3600 * TICKS_PER_SECOND),
        in_day / (60 * TICKS_PER_SECOND) % 60,
        in_day / TICKS_PER_SECOND % 60,
        in_day / TICKS_PER_MS % 1000,
        (days + EPOCH_WEEKDAY) % 7,
    ];
    let mut out = [0i16; 8];
    for (slot, value) in out.iter_mut().zip(fields) {
        *slot = i16::try_from(value).ok()?;
    }
    Some(out)
}

fn invalid(call: &Call, detail: &str) -> Erro {
    Erro::ArgumentoInvalido {
        nome: call.export.name,
        ordinal: call.export.ordinal,
        detalhe: detail.to_owned(),
    }
}

impl super::Dispatcher {
    /// `ULONGLONG KeQueryPerformanceFrequency(void)`: o valor de 64 bits sai em EDX:EAX.
    pub(super) fn ke_query_performance_frequency(&mut self) -> Atendimento {
        self.return_u64(PERF_COUNTER_HZ)
    }

    /// `ULONGLONG KeQueryPerformanceCounter(void)`: o contador do Clock da sessão em 3,375 MHz.
    pub(super) fn ke_query_performance_counter(&mut self, clock: Option<&Clock>) -> Resultado {
        let clock = clock.ok_or(Erro::ContextoTemporal {
            detalhe: "a chamada exige o Clock explícito da sessão",
        })?;
        Ok(self.return_u64(clock.perf_counter()))
    }

    /// Devolve um valor de 64 bits: a palavra baixa no `EAX`, a alta no `EDX` (que a sessão
    /// aplica junto com o retorno).
    fn return_u64(&mut self, value: u64) -> Atendimento {
        self.return_high = Some((value >> 32) as u32);
        Atendimento { eax: value as u32 }
    }

    /// Só a sessão chama: a palavra alta do retorno de 64 bits do último serviço, se houve.
    pub fn take_return_high(&mut self) -> Option<u32> {
        self.return_high.take()
    }

    /// `VOID RtlTimeToTimeFields(PLARGE_INTEGER Time, PTIME_FIELDS TimeFields)`
    pub(super) fn rtl_time_to_time_fields(bus: &mut Bus, call: &Call) -> Resultado {
        let time = call.argument(bus, 0)?;
        let output = call.argument(bus, 1)?;
        if !ram_range(bus, time, 8, Acesso::Leitura) || !ram_range(bus, output, 16, Acesso::Escrita)
        {
            return Err(invalid(call, "Time e TimeFields devem estar em RAM guest"));
        }
        let ticks = bus.ler_u64(GuestVirt(time))?.cast_signed();
        let Some(fields) = ticks_to_fields(ticks) else {
            return Err(invalid(
                call,
                "tempo negativo ou com ano fora de um SHORT: sem contrato verificado",
            ));
        };
        let mut bytes = [0u8; 16];
        for (index, field) in fields.iter().enumerate() {
            bytes[index * 2..index * 2 + 2].copy_from_slice(&field.to_le_bytes());
        }
        bus.escrever_bytes(GuestVirt(output), &bytes)?;
        Ok(Atendimento { eax: 0 }) // VOID
    }

    /// `BOOLEAN RtlTimeFieldsToTime(PTIME_FIELDS TimeFields, PLARGE_INTEGER Time)`
    ///
    /// Campos fora de faixa devolvem FALSE e **não** escrevem a saída.
    pub(super) fn rtl_time_fields_to_time(bus: &mut Bus, call: &Call) -> Resultado {
        let input = call.argument(bus, 0)?;
        let output = call.argument(bus, 1)?;
        if !ram_range(bus, input, 16, Acesso::Leitura)
            || !ram_range(bus, output, 8, Acesso::Escrita)
        {
            return Err(invalid(call, "TimeFields e Time devem estar em RAM guest"));
        }
        let mut fields = [0i16; 8];
        for (index, field) in fields.iter_mut().enumerate() {
            *field = bus
                .ler_u16(GuestVirt(input + index as u32 * 2))?
                .cast_signed();
        }
        let Some(ticks) = fields_to_ticks(fields) else {
            return Ok(Atendimento { eax: 0 });
        };
        bus.escrever_bytes(GuestVirt(output), &ticks.to_le_bytes())?;
        Ok(Atendimento { eax: 1 })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(year: i16, month: i16, day: i16, hour: i16, min: i16, sec: i16, ms: i16) -> Fields {
        // O dia da semana é ignorado na ida e calculado na volta.
        [year, month, day, hour, min, sec, ms, 0]
    }

    #[test]
    fn datas_conhecidas() {
        // 1601-01-01 00:00:00 é o zero do FILETIME, segunda-feira.
        assert_eq!(fields_to_ticks(at(1601, 1, 1, 0, 0, 0, 0)), Some(0));
        assert_eq!(ticks_to_fields(0), Some([1601, 1, 1, 0, 0, 0, 0, 1]));
        // 1970-01-01 é 116 444 736 000 000 000 intervalos depois (época Unix), quinta-feira.
        assert_eq!(
            fields_to_ticks(at(1970, 1, 1, 0, 0, 0, 0)),
            Some(116_444_736_000_000_000)
        );
        assert_eq!(
            ticks_to_fields(116_444_736_000_000_000),
            Some([1970, 1, 1, 0, 0, 0, 0, 4])
        );
        // O perfil sintético do projeto: 2000-01-01, sábado.
        assert_eq!(
            fields_to_ticks(at(2000, 1, 1, 0, 0, 0, 0)),
            Some(125_911_584_000_000_000)
        );
        assert_eq!(
            ticks_to_fields(125_911_584_000_000_000),
            Some([2000, 1, 1, 0, 0, 0, 0, 6])
        );
        // Frações: 2000-03-01 12:34:56.789.
        let t = fields_to_ticks(at(2000, 3, 1, 12, 34, 56, 789)).unwrap();
        assert_eq!(ticks_to_fields(t), Some([2000, 3, 1, 12, 34, 56, 789, 3]));
    }

    #[test]
    fn bissextos_e_limites_de_campo() {
        assert!(
            fields_to_ticks(at(2000, 2, 29, 0, 0, 0, 0)).is_some(),
            "400 em 400"
        );
        assert!(
            fields_to_ticks(at(1900, 2, 29, 0, 0, 0, 0)).is_none(),
            "século não bissexto"
        );
        assert!(fields_to_ticks(at(2004, 2, 29, 0, 0, 0, 0)).is_some());
        assert!(fields_to_ticks(at(2100, 2, 29, 0, 0, 0, 0)).is_none());
        for ruim in [
            at(1600, 1, 1, 0, 0, 0, 0),
            at(2000, 0, 1, 0, 0, 0, 0),
            at(2000, 13, 1, 0, 0, 0, 0),
            at(2000, 4, 31, 0, 0, 0, 0),
            at(2000, 1, 0, 0, 0, 0, 0),
            at(2000, 1, 1, 24, 0, 0, 0),
            at(2000, 1, 1, 0, 60, 0, 0),
            at(2000, 1, 1, 0, 0, 60, 0),
            at(2000, 1, 1, 0, 0, 0, 1000),
            at(2000, 1, 1, -1, 0, 0, 0),
        ] {
            assert_eq!(fields_to_ticks(ruim), None, "{ruim:?}");
        }
    }

    #[test]
    fn ida_e_volta_em_todo_o_intervalo_e_o_dia_da_semana_avanca() {
        let mut ticks = 0i64;
        let mut anterior_semana = None;
        // Um dia por passo, de 1601 a ~2400, e depois saltos grandes até o limite.
        while ticks < 300 * 365 * TICKS_PER_DAY + 400 * 365 * TICKS_PER_DAY / 2 {
            let fields = ticks_to_fields(ticks).unwrap();
            assert_eq!(fields_to_ticks(fields), Some(ticks), "{fields:?}");
            if let Some(semana) = anterior_semana {
                assert_eq!(i64::from(fields[7]), (semana + 1) % 7, "{fields:?}");
            }
            anterior_semana = Some(i64::from(fields[7]));
            ticks += TICKS_PER_DAY;
        }
        for ticks in [
            i64::MAX / 2,
            i64::MAX - TICKS_PER_DAY,
            123_456_789_012_345_678,
        ] {
            let fields = ticks_to_fields(ticks).unwrap();
            assert_eq!(fields_to_ticks(fields), Some(ticks - ticks % TICKS_PER_MS));
        }
    }

    #[test]
    fn tempo_negativo_nao_tem_conversao() {
        assert_eq!(ticks_to_fields(-1), None);
        assert_eq!(ticks_to_fields(i64::MIN), None);
    }
}
