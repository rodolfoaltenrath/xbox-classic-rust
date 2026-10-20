//! Linha de base local: acessos seguros alinhados, stride de uma cache line.
use std::{hint::black_box, time::Instant};
use xbox_mem::{Arena, Bus, GuestPhys, GuestVirt, Perm};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    for (size, iterations) in [(64 * 1024, 2048u32), (64 * 1024 * 1024, 4u32)] {
        let mut bus = Bus::com_arena(Arena::com_tamanho(size)?)?;
        bus.mapear(
            GuestVirt(0x10000),
            GuestPhys(0),
            (size / 4096) as u32,
            Perm::RW,
        )?;
        // Materializar páginas e aquecer antes da medição.
        for offset in (0..size).step_by(4096) {
            bus.escrever_u32(GuestVirt(0x10000 + offset as u32), offset as u32)?;
        }
        let accesses = f64::from((size / 64) as u32) * f64::from(iterations);
        let start = Instant::now();
        let mut checksum = 0u32;
        for _ in 0..iterations {
            for offset in (0..size).step_by(64) {
                let bytes = bus.arena().bytes();
                let offset = black_box(offset);
                checksum = checksum.wrapping_add(u32::from_le_bytes([
                    bytes[offset],
                    bytes[offset + 1],
                    bytes[offset + 2],
                    bytes[offset + 3],
                ]));
            }
        }
        black_box(checksum);
        let direct = start.elapsed().as_secs_f64() * 1e9 / accesses;
        let start = Instant::now();
        let mut checksum_bus = 0u32;
        for _ in 0..iterations {
            for offset in (0..size).step_by(64) {
                checksum_bus = checksum_bus
                    .wrapping_add(bus.ler_u32(GuestVirt(black_box(0x10000 + offset as u32)))?);
            }
        }
        black_box(checksum_bus);
        assert_eq!(checksum, checksum_bus);
        let checked = start.elapsed().as_secs_f64() * 1e9 / accesses;
        println!(
            "RAM={size} bytes, {accesses:.0} leituras: arena={direct:.2} ns, bus={checked:.2} ns, razão={:.2}x",
            checked / direct
        );
    }
    Ok(())
}
