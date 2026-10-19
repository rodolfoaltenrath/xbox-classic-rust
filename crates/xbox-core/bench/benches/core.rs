//! Benchmarks do núcleo.
//!
//! Alvo da Fase 0: ter a infraestrutura de medição funcionando antes de existir algo
//! para otimizar. O agendador é medido porque ele roda milhões de vezes por segundo —
//! ROADMAP §10 exige número antes e depois em qualquer mudança de performance.

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use xbox_core::snapshot::{Snapshot, SnapshotWriter};
use xbox_core::time::{Clock, EventKind, Scheduler};

fn bench_scheduler(c: &mut Criterion) {
    c.bench_function("scheduler: agenda e consome 1000 eventos", |b| {
        b.iter(|| {
            let mut s = Scheduler::new();
            for i in 0..1000u32 {
                s.schedule_in(u64::from(i) * 7, EventKind::Custom(i));
            }
            let mut n = 0u32;
            while let Some((_, k)) = s.run_until(100_000) {
                n += u32::from(matches!(k, EventKind::Custom(_)));
            }
            black_box(n)
        });
    });
}

fn bench_clock(c: &mut Criterion) {
    c.bench_function("clock: conversoes de dominio", |b| {
        let mut clock = Clock::new();
        clock.advance(733_333_333);
        b.iter(|| {
            black_box(clock.perf_counter() + clock.tick_count_ms() + clock.audio_samples())
        });
    });
}

fn bench_snapshot(c: &mut Criterion) {
    let mut s = Scheduler::new();
    for i in 0..256u32 {
        s.schedule_in(u64::from(i) * 13, EventKind::KernelTimer(i));
    }
    c.bench_function("snapshot: salvar agendador com 256 eventos", |b| {
        b.iter(|| {
            let mut w = SnapshotWriter::new();
            s.save(&mut w);
            black_box(w.len())
        });
    });
}

criterion_group!(benches, bench_scheduler, bench_clock, bench_snapshot);
criterion_main!(benches);
