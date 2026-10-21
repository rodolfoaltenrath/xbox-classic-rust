//! Relógio virtual e agendador de eventos.
//!
//! Implementa o modelo de tempo descrito no ROADMAP §7.9: existe **um único relógio**,
//! contado em ciclos da CPU do guest, e todos os outros domínios de tempo
//! (`RDTSC`, contador de performance, tick de 1 ms, vblank, clock de áudio) são
//! derivados dele.
//!
//! ## Por que não usar o relógio do host
//!
//! Um emulador que agenda eventos por tempo de parede (`Instant::now()`) não é
//! determinístico: dois replays da mesma entrada divergem. Isso inviabiliza save
//! state confiável, replay de regressão e comparação de bug report — ou seja, as três
//! ferramentas que mais economizam tempo depois. Daí a regra: **nada no núcleo de
//! emulação lê o relógio do host.** O host só entra na hora de *apresentar* o frame e
//! de alimentar o áudio — responsabilidade do orquestrador, não deste módulo.
//!
//! ## Eventos são identificadores, não closures
//!
//! [`EventKind`] é um enum simples, sem `Box<dyn FnMut>`. Isso é intencional: uma fila
//! de closures não pode ser serializada, e serialização é requisito (§4.4). Quem
//! consome o evento decide o que fazer com ele.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::error::SnapshotError;
use crate::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};

/// Frequência da CPU do Xbox: Pentium III a 733⅓ MHz.
pub const CPU_HZ: u64 = 733_333_333;

/// Frequência do contador de performance do Xbox (`KeQueryPerformanceFrequency`).
pub const PERF_COUNTER_HZ: u64 = 3_375_000;

/// Taxa de amostragem do APU.
pub const AUDIO_HZ: u64 = 48_000;

/// Taxa de vblank em NTSC, em milihertz (59,94 Hz).
pub const VBLANK_NTSC_MHZ: u64 = 59_940;

/// Taxa de vblank em PAL, em milihertz (50 Hz).
pub const VBLANK_PAL_MHZ: u64 = 50_000;

/// Contagem de ciclos da CPU do guest.
///
/// `u64` a 733 MHz transborda em ~797 anos de tempo emulado; não é uma preocupação.
pub type Cycles = u64;

/// Relógio virtual do console, em ciclos da CPU do guest.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Clock {
    cycles: Cycles,
}

impl Clock {
    /// Relógio zerado (instante do power-on).
    pub const fn new() -> Self {
        Self { cycles: 0 }
    }

    /// Ciclos decorridos desde o power-on.
    pub const fn cycles(self) -> Cycles {
        self.cycles
    }

    /// Avança o relógio.
    pub const fn advance(&mut self, delta: Cycles) {
        self.cycles += delta;
    }

    /// Posiciona o relógio em um instante absoluto.
    ///
    /// Em debug, acusa tentativa de voltar no tempo — sintoma clássico de bug de
    /// agendamento.
    pub fn set(&mut self, cycles: Cycles) {
        debug_assert!(
            cycles >= self.cycles,
            "relógio andando para trás: {} -> {cycles}",
            self.cycles
        );
        self.cycles = cycles;
    }

    /// Valor de `RDTSC` visto pelo guest.
    pub const fn tsc(self) -> u64 {
        self.cycles
    }

    /// Valor de `KeQueryPerformanceCounter` (3,375 MHz).
    pub const fn perf_counter(self) -> u64 {
        scale(self.cycles, PERF_COUNTER_HZ, CPU_HZ)
    }

    /// Valor de `KeTickCount` (milissegundos desde o boot).
    pub const fn tick_count_ms(self) -> u64 {
        scale(self.cycles, 1_000, CPU_HZ)
    }

    /// Amostras de áudio que o APU já deveria ter consumido.
    pub const fn audio_samples(self) -> u64 {
        scale(self.cycles, AUDIO_HZ, CPU_HZ)
    }

    /// Nanossegundos de tempo emulado decorridos.
    pub const fn nanos(self) -> u64 {
        scale(self.cycles, 1_000_000_000, CPU_HZ)
    }

    /// Converte milissegundos em ciclos, **arredondando para cima**.
    ///
    /// Duração pedida arredonda para cima; tempo decorrido arredonda para baixo. Essa
    /// assimetria é deliberada: um `KeDelayExecutionThread(16ms)` que acordasse em
    /// 15,999 ms seria um bug observável pelo guest — timer nunca vence cedo. Como
    /// efeito colateral, `ms -> ciclos -> ms` é estável.
    pub const fn ms_to_cycles(ms: u64) -> Cycles {
        scale_ceil(ms, CPU_HZ, 1_000)
    }

    /// Converte microssegundos em ciclos, arredondando para cima.
    pub const fn us_to_cycles(us: u64) -> Cycles {
        scale_ceil(us, CPU_HZ, 1_000_000)
    }

    /// Converte unidades de 100 ns (`LARGE_INTEGER` do kernel) em ciclos, arredondando
    /// **para cima**, ou `None` se o resultado não couber em `u64`.
    ///
    /// Versão verificada, ao contrário de [`Self::ms_to_cycles`]: aquelas recebem
    /// constantes do próprio emulador, e `scale_ceil` trunca em silêncio com `as u64`.
    /// Esta recebe o intervalo de um `Sleep` do guest — entrada não confiável — e um
    /// valor enorme não pode virar um intervalo curto por truncamento.
    pub const fn hundred_ns_to_cycles(ticks: u64) -> Option<Cycles> {
        let n = ticks as u128 * CPU_HZ as u128;
        let ciclos = n.div_ceil(10_000_000);
        if ciclos > u64::MAX as u128 {
            None
        } else {
            Some(ciclos as u64)
        }
    }

    /// Duração de um frame de vídeo, em ciclos, para a taxa dada em milihertz.
    ///
    /// Use [`VBLANK_NTSC_MHZ`] ou [`VBLANK_PAL_MHZ`].
    pub const fn frame_cycles(rate_millihz: u64) -> Cycles {
        scale(CPU_HZ, 1_000, rate_millihz)
    }
}

/// Multiplica e divide em 128 bits para não perder precisão nem transbordar.
///
/// Arredonda para baixo: é a semântica de "quanto tempo já passou".
const fn scale(value: u64, mul: u64, div: u64) -> u64 {
    ((value as u128 * mul as u128) / div as u128) as u64
}

/// Como [`scale`], mas arredondando para cima: semântica de "duração pedida".
const fn scale_ceil(value: u64, mul: u64, div: u64) -> u64 {
    let n = value as u128 * mul as u128;
    let d = div as u128;
    (n.div_ceil(d)) as u64
}

impl Snapshot for Clock {
    fn save(&self, out: &mut SnapshotWriter) {
        out.section("CLK0", 1);
        out.u64(self.cycles);
    }

    fn load(&mut self, inp: &mut SnapshotReader<'_>) -> Result<(), SnapshotError> {
        let _v = inp.section("CLK0", 1)?;
        self.cycles = inp.u64()?;
        Ok(())
    }
}

/// O que venceu na fila de eventos.
///
/// Mantido pequeno, `Copy` e sem alocação para poder ser serializado e comparado.
/// Variantes novas são adicionadas conforme as fases avançam; o importante é que a
/// codificação em (tag, payload) permaneça estável para snapshots antigos.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventKind {
    /// Começo do intervalo de vblank — dispara a IRQ de vblank no guest.
    VblankStart,
    /// Fim do intervalo de vblank.
    VblankEnd,
    /// O APU consumiu um buffer e precisa de mais amostras.
    AudioBuffer,
    /// O quantum da thread atual do guest expirou (scheduler do kernel, Fase 4).
    ThreadQuantum,
    /// Um timer do kernel venceu (`KeSetTimerEx`), identificado pelo handle interno.
    KernelTimer(u32),
    /// Um notifier/semáforo da GPU precisa ser sinalizado (Fase 5).
    GpuNotifier(u32),
    /// Evento de uso livre para experimentos e testes.
    Custom(u32),
}

impl EventKind {
    const TAG_VBLANK_START: u8 = 1;
    const TAG_VBLANK_END: u8 = 2;
    const TAG_AUDIO_BUFFER: u8 = 3;
    const TAG_THREAD_QUANTUM: u8 = 4;
    const TAG_KERNEL_TIMER: u8 = 5;
    const TAG_GPU_NOTIFIER: u8 = 6;
    const TAG_CUSTOM: u8 = 7;

    const fn parts(self) -> (u8, u32) {
        match self {
            Self::VblankStart => (Self::TAG_VBLANK_START, 0),
            Self::VblankEnd => (Self::TAG_VBLANK_END, 0),
            Self::AudioBuffer => (Self::TAG_AUDIO_BUFFER, 0),
            Self::ThreadQuantum => (Self::TAG_THREAD_QUANTUM, 0),
            Self::KernelTimer(id) => (Self::TAG_KERNEL_TIMER, id),
            Self::GpuNotifier(id) => (Self::TAG_GPU_NOTIFIER, id),
            Self::Custom(id) => (Self::TAG_CUSTOM, id),
        }
    }

    const fn from_parts(tag: u8, payload: u32) -> Option<Self> {
        Some(match tag {
            Self::TAG_VBLANK_START => Self::VblankStart,
            Self::TAG_VBLANK_END => Self::VblankEnd,
            Self::TAG_AUDIO_BUFFER => Self::AudioBuffer,
            Self::TAG_THREAD_QUANTUM => Self::ThreadQuantum,
            Self::TAG_KERNEL_TIMER => Self::KernelTimer(payload),
            Self::TAG_GPU_NOTIFIER => Self::GpuNotifier(payload),
            Self::TAG_CUSTOM => Self::Custom(payload),
            _ => return None,
        })
    }
}

/// Um evento agendado.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Scheduled {
    deadline: Cycles,
    /// Número de inserção, para desempate determinístico entre eventos com o mesmo
    /// prazo. Sem isso, a ordem viria da estrutura interna do heap — que é estável,
    /// mas não é *garantida*, e portanto não serve para determinismo.
    seq: u64,
    kind: EventKind,
}

impl Ord for Scheduled {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.deadline
            .cmp(&other.deadline)
            .then(self.seq.cmp(&other.seq))
    }
}

impl PartialOrd for Scheduled {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Fila de eventos por prazo, com o relógio virtual.
///
/// O laço de emulação segue o padrão:
///
/// ```
/// # use xbox_core::time::{Clock, EventKind, Scheduler};
/// let mut sched = Scheduler::new();
/// sched.schedule_in(Clock::ms_to_cycles(16), EventKind::VblankStart);
///
/// let limite = sched.now() + Clock::ms_to_cycles(100);
/// while let Some((instante, evento)) = sched.run_until(limite) {
///     // trata o evento; o relógio já está posicionado em `instante`
///     let _ = (instante, evento);
/// }
/// assert_eq!(sched.now(), limite);
/// ```
#[derive(Debug, Default)]
pub struct Scheduler {
    clock: Clock,
    queue: BinaryHeap<Reverse<Scheduled>>,
    next_seq: u64,
}

impl Scheduler {
    /// Agendador vazio com o relógio zerado.
    pub fn new() -> Self {
        Self {
            clock: Clock::new(),
            queue: BinaryHeap::with_capacity(64),
            next_seq: 0,
        }
    }

    /// Instante atual, em ciclos.
    pub const fn now(&self) -> Cycles {
        self.clock.cycles()
    }

    /// O relógio virtual, para converter entre domínios de tempo.
    pub const fn clock(&self) -> Clock {
        self.clock
    }

    /// Agenda um evento para um instante absoluto.
    ///
    /// Um prazo já vencido é agendado para "agora", não descartado.
    pub fn schedule_at(&mut self, deadline: Cycles, kind: EventKind) {
        let deadline = deadline.max(self.clock.cycles());
        self.queue.push(Reverse(Scheduled {
            deadline,
            seq: self.next_seq,
            kind,
        }));
        self.next_seq += 1;
    }

    /// Agenda um evento para `delta` ciclos no futuro.
    pub fn schedule_in(&mut self, delta: Cycles, kind: EventKind) {
        self.schedule_at(self.clock.cycles().saturating_add(delta), kind);
    }

    /// Remove todos os eventos de um tipo. Devolve quantos foram removidos.
    pub fn cancel(&mut self, kind: EventKind) -> usize {
        let antes = self.queue.len();
        let restantes: Vec<_> = self
            .queue
            .drain()
            .filter(|Reverse(e)| e.kind != kind)
            .collect();
        self.queue.extend(restantes);
        antes - self.queue.len()
    }

    /// Prazo do próximo evento, se houver.
    pub fn next_deadline(&self) -> Option<Cycles> {
        self.queue.peek().map(|Reverse(e)| e.deadline)
    }

    /// Quantidade de eventos pendentes.
    pub fn pending(&self) -> usize {
        self.queue.len()
    }

    /// Avança o relógio até o próximo evento que vence em `limit` ou antes.
    ///
    /// Devolve `Some((instante, evento))` com o relógio já posicionado no instante do
    /// evento. Quando nenhum evento vence até `limit`, posiciona o relógio exatamente
    /// em `limit` e devolve `None` — ou seja, o laço `while let` consome todos os
    /// eventos da janela e termina com o tempo correto.
    pub fn run_until(&mut self, limit: Cycles) -> Option<(Cycles, EventKind)> {
        match self.queue.peek() {
            Some(Reverse(e)) if e.deadline <= limit => {
                let Reverse(e) = self.queue.pop()?;
                self.clock.set(e.deadline);
                Some((e.deadline, e.kind))
            }
            _ => {
                if limit > self.clock.cycles() {
                    self.clock.set(limit);
                }
                None
            }
        }
    }
}

impl Snapshot for Scheduler {
    fn save(&self, out: &mut SnapshotWriter) {
        out.section("SCH0", 1);
        self.clock.save(out);
        out.u64(self.next_seq);

        // Ordenado para que o snapshot não dependa do layout interno do heap:
        // dois agendadores com o mesmo estado lógico produzem bytes iguais.
        let mut eventos: Vec<Scheduled> = self.queue.iter().map(|Reverse(e)| *e).collect();
        eventos.sort_unstable();
        out.u64(eventos.len() as u64);
        for e in eventos {
            let (tag, payload) = e.kind.parts();
            out.u64(e.deadline);
            out.u64(e.seq);
            out.u8(tag);
            out.u32(payload);
        }
    }

    fn load(&mut self, inp: &mut SnapshotReader<'_>) -> Result<(), SnapshotError> {
        let _v = inp.section("SCH0", 1)?;
        self.clock.load(inp)?;
        self.next_seq = inp.u64()?;

        let n = inp.u64()? as usize;
        self.queue.clear();
        self.queue.reserve(n);
        for _ in 0..n {
            let deadline = inp.u64()?;
            let seq = inp.u64()?;
            let tag = inp.u8()?;
            let payload = inp.u32()?;
            let kind = EventKind::from_parts(tag, payload).ok_or(SnapshotError::BadValue {
                section: "SCH0",
                what: "tag de EventKind desconhecida",
            })?;
            self.queue.push(Reverse(Scheduled {
                deadline,
                seq,
                kind,
            }));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{AUDIO_HZ, CPU_HZ, Clock, EventKind, PERF_COUNTER_HZ, Scheduler, VBLANK_NTSC_MHZ};
    use crate::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};

    #[test]
    fn um_segundo_de_ciclos_bate_com_os_outros_dominios() {
        let mut c = Clock::new();
        c.advance(CPU_HZ);
        assert_eq!(c.perf_counter(), PERF_COUNTER_HZ);
        assert_eq!(c.tick_count_ms(), 1_000);
        assert_eq!(c.audio_samples(), AUDIO_HZ);
        assert_eq!(c.nanos(), 1_000_000_000);
    }

    #[test]
    fn conversoes_de_ida_e_volta_sao_coerentes() {
        for ms in [1u64, 5, 16, 33, 100, 1_000] {
            let mut c = Clock::new();
            c.advance(Clock::ms_to_cycles(ms));
            assert_eq!(c.tick_count_ms(), ms, "ida e volta falhou para {ms} ms");
        }
        assert_eq!(Clock::us_to_cycles(1_000), Clock::ms_to_cycles(1));
    }

    #[test]
    fn cem_ns_para_ciclos_bate_com_um_segundo_e_arredonda_para_cima() {
        // 10_000_000 unidades de 100 ns = 1 s = CPU_HZ ciclos, exato.
        assert_eq!(Clock::hundred_ns_to_cycles(10_000_000), Some(CPU_HZ));
        assert_eq!(Clock::hundred_ns_to_cycles(0), Some(0));
        // O `Sleep(2000)` do hello: 2000 ms = 20_000_000 unidades.
        assert_eq!(Clock::hundred_ns_to_cycles(20_000_000), Some(2 * CPU_HZ));
        // 1 unidade = 73,3333 ciclos: nunca vence cedo, então vai para 74.
        assert_eq!(Clock::hundred_ns_to_cycles(1), Some(74));
        // Consistente com a conversão por milissegundo (1 ms = 10_000 unidades).
        assert_eq!(
            Clock::hundred_ns_to_cycles(10_000),
            Some(Clock::ms_to_cycles(1))
        );
    }

    #[test]
    fn cem_ns_para_ciclos_recusa_overflow_em_vez_de_truncar() {
        // `scale_ceil` faria `as u64` e devolveria um intervalo curto e errado.
        let limite = u64::MAX / CPU_HZ * 10_000_000;
        assert!(Clock::hundred_ns_to_cycles(limite).is_some());
        assert_eq!(Clock::hundred_ns_to_cycles(u64::MAX), None);
        assert_eq!(Clock::hundred_ns_to_cycles(u64::MAX / 2), None);
    }

    #[test]
    fn duracao_pedida_nunca_vence_cedo() {
        // Um timer de 16 ms não pode vencer em 15,999 ms: o guest veria o relógio
        // andar menos do que pediu. Daí `ms_to_cycles` arredondar para cima.
        for ms in 1u64..200 {
            let ciclos = Clock::ms_to_cycles(ms);
            let mut c = Clock::new();
            c.advance(ciclos);
            assert!(
                c.nanos() >= ms * 1_000_000,
                "{ms} ms venceu cedo: {} ns",
                c.nanos()
            );
        }
    }

    #[test]
    fn frame_ntsc_dura_cerca_de_16_7_ms() {
        let f = Clock::frame_cycles(VBLANK_NTSC_MHZ);
        let mut c = Clock::new();
        c.advance(f);
        // 1/59,94 Hz = 16,683 ms
        assert_eq!(c.tick_count_ms(), 16);
        assert!(
            (16_600..=16_700).contains(&(c.nanos() / 1_000)),
            "{} us",
            c.nanos() / 1_000
        );
    }

    #[test]
    fn eventos_vencem_em_ordem_de_prazo() {
        let mut s = Scheduler::new();
        s.schedule_in(300, EventKind::VblankEnd);
        s.schedule_in(100, EventKind::VblankStart);
        s.schedule_in(200, EventKind::AudioBuffer);

        let mut vistos = Vec::new();
        while let Some((t, k)) = s.run_until(1_000) {
            vistos.push((t, k));
        }
        assert_eq!(
            vistos,
            vec![
                (100, EventKind::VblankStart),
                (200, EventKind::AudioBuffer),
                (300, EventKind::VblankEnd),
            ]
        );
        assert_eq!(
            s.now(),
            1_000,
            "o relógio deve terminar no limite da janela"
        );
    }

    #[test]
    fn prazos_iguais_vencem_na_ordem_de_insercao() {
        // É isto que torna o agendador determinístico (ROADMAP §4.4).
        let mut s = Scheduler::new();
        for i in 0..8 {
            s.schedule_at(500, EventKind::Custom(i));
        }
        let mut ordem = Vec::new();
        while let Some((_, k)) = s.run_until(1_000) {
            ordem.push(k);
        }
        let esperado: Vec<_> = (0..8).map(EventKind::Custom).collect();
        assert_eq!(ordem, esperado);
    }

    #[test]
    fn run_until_nao_passa_do_limite() {
        let mut s = Scheduler::new();
        s.schedule_in(5_000, EventKind::VblankStart);
        assert!(s.run_until(1_000).is_none());
        assert_eq!(s.now(), 1_000);
        assert_eq!(s.pending(), 1);
        assert_eq!(s.next_deadline(), Some(5_000));
    }

    #[test]
    fn cancelar_remove_apenas_o_tipo_pedido() {
        let mut s = Scheduler::new();
        s.schedule_in(10, EventKind::KernelTimer(1));
        s.schedule_in(20, EventKind::KernelTimer(2));
        s.schedule_in(30, EventKind::KernelTimer(1));
        s.schedule_in(40, EventKind::AudioBuffer);

        assert_eq!(s.cancel(EventKind::KernelTimer(1)), 2);
        assert_eq!(s.pending(), 2);
        assert_eq!(s.next_deadline(), Some(20));
    }

    #[test]
    fn snapshot_do_agendador_e_deterministico_e_restaura_a_ordem() {
        let mut s = Scheduler::new();
        s.schedule_in(300, EventKind::GpuNotifier(7));
        s.schedule_in(100, EventKind::VblankStart);
        s.schedule_in(100, EventKind::AudioBuffer);
        let _ = s.run_until(50);

        let mut w1 = SnapshotWriter::new();
        s.save(&mut w1);
        let bytes = w1.finish();

        let mut restaurado = Scheduler::new();
        let mut r = SnapshotReader::new(&bytes).expect("cabeçalho");
        restaurado.load(&mut r).expect("load");

        assert_eq!(restaurado.now(), s.now());
        assert_eq!(restaurado.pending(), s.pending());

        let mut w2 = SnapshotWriter::new();
        restaurado.save(&mut w2);
        assert_eq!(
            bytes,
            w2.finish(),
            "snapshot não é estável após ida e volta"
        );

        let mut ordem_a = Vec::new();
        while let Some((t, k)) = s.run_until(1_000) {
            ordem_a.push((t, k));
        }
        let mut ordem_b = Vec::new();
        while let Some((t, k)) = restaurado.run_until(1_000) {
            ordem_b.push((t, k));
        }
        assert_eq!(ordem_a, ordem_b);
    }
}
