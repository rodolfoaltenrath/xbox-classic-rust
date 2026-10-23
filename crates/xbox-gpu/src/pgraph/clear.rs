use super::{Memory, Surface};
#[derive(Debug, Clone, Copy)]
struct Job {
    base: u32,
    pitch: u32,
    x0: u32,
    x1: u32,
    y0: u32,
    y1: u32,
    value: u32,
    mask: u32,
}
pub(super) fn descriptor(
    mem: &impl Memory,
    instance: u32,
    write: bool,
) -> Result<(u32, u32), &'static str> {
    if instance & 15 != 0 || instance >= 0x100000 {
        return Err("instância de surface inválida");
    }
    let flags = mem.read_instance(instance)?;
    let limit = mem.read_instance(instance + 4)?;
    let frame = mem.read_instance(instance + 8)?;
    if !(flags & 0xFFF == 0x3D || flags & 0xFFF == if write { 3 } else { 2 })
        || !matches!((flags >> 16) & 3, 0 | 2)
    {
        return Err("surface exige DMA de escrita linear UMA");
    }
    let base = (frame & 0xFFFFF000) | (flags >> 20);
    if base & 3 != 0 {
        return Err("base de surface desalinhada");
    }
    Ok((base, limit))
}
fn job(
    surface: Surface,
    mem: &impl Memory,
    depth: bool,
    flags: u32,
    rect: [u32; 4],
) -> Result<Job, &'static str> {
    let [x0, x1, y0, y1] = rect;
    let (instance, offset, pitch, value, mask) = if depth {
        (
            surface.zeta_dma,
            surface.zeta_offset,
            surface.pitch >> 16,
            surface.z_clear,
            (if flags & 1 != 0 { 0xFFFFFF00 } else { 0 }) | (if flags & 2 != 0 { 0xFF } else { 0 }),
        )
    } else {
        let mut mask = 0;
        for (bit, channel) in [
            (0x10, 0x00FF0000),
            (0x20, 0x0000FF00),
            (0x40, 0x000000FF),
            (0x80, 0xFF000000),
        ] {
            if flags & bit != 0 {
                mask |= channel;
            }
        }
        (
            surface.color_dma,
            surface.color_offset,
            surface.pitch & 0xFFFF,
            surface.color_clear,
            mask,
        )
    };
    if pitch & 3 != 0
        || u64::from(pitch) < u64::from(surface.horizontal >> 16) * 4
        || offset & 3 != 0
    {
        return Err("pitch/offset de surface inválido");
    }
    let (base, limit) = descriptor(mem, instance, true)?;
    let start = u64::from(offset) + u64::from(y0) * u64::from(pitch) + u64::from(x0) * 4;
    let end = u64::from(offset) + u64::from(y1) * u64::from(pitch) + u64::from(x1) * 4 + 4;
    if end > u64::from(limit) + 1
        || u64::from(base) + end > mem.ram_len()
        || u64::from(base) + end > u64::from(u32::MAX) + 1
    {
        return Err("clear ultrapassa limite DMA ou RAM");
    }
    if start >= end {
        return Err("faixa de clear inválida");
    }
    Ok(Job {
        base: base.checked_add(offset).ok_or("base de clear transborda")?,
        pitch,
        x0,
        x1,
        y0,
        y1,
        value,
        mask,
    })
}
pub(super) fn apply(
    surface: Surface,
    flags: u32,
    mem: &mut impl Memory,
    capacity: u64,
) -> Result<u64, &'static str> {
    if flags & !0xF3 != 0 {
        return Err("flags de clear sem modelo");
    }
    if flags == 0 {
        return Ok(0);
    }
    if surface.format & 0xFFFF != 0x128 {
        return Err("clear exige A8R8G8B8/Z24S8 linear, uma amostra");
    }
    let width = surface.horizontal >> 16;
    let height = surface.vertical >> 16;
    if surface.horizontal & 0xFFFF != 0
        || surface.vertical & 0xFFFF != 0
        || width == 0
        || height == 0
        || width > 4096
        || height > 4096
    {
        return Err("dimensões/origem de surface fora do perfil");
    }
    let x0 = surface.rect_h & 0xFFFF;
    let x1 = surface.rect_h >> 16;
    let y0 = surface.rect_v & 0xFFFF;
    let y1 = surface.rect_v >> 16;
    if x0 > x1 || y0 > y1 || x0 >= width || y0 >= height {
        return Ok(0);
    }
    let rect = [x0, x1.min(width - 1), y0, y1.min(height - 1)];
    let area = u64::from(rect[1] - rect[0] + 1) * u64::from(rect[3] - rect[2] + 1);
    if area > 4 * 1024 * 1024 {
        return Err("clear excede orçamento de pixels do perfil");
    }
    let mut jobs = Vec::with_capacity(2);
    if flags & 0xF0 != 0 {
        jobs.push(job(surface, mem, false, flags, rect)?);
    }
    if flags & 3 != 0 {
        jobs.push(job(surface, mem, true, flags, rect)?);
    }
    let pixels = area * jobs.len() as u64;
    if pixels > capacity {
        return Err("contador de pixels esgotado");
    }
    // Todas as superfícies/linhas já validadas antes do primeiro store.
    for j in jobs {
        for y in j.y0..=j.y1 {
            for x in j.x0..=j.x1 {
                let address = j.base + y * j.pitch + x * 4;
                let old = if j.mask == u32::MAX {
                    0
                } else {
                    mem.read_ram(address)?
                };
                mem.write_ram(address, (old & !j.mask) | (j.value & j.mask))?;
            }
        }
    }
    Ok(pixels)
}

/// Valida toda a surface linear antes de buscar ou alterar um pixel do raster.
pub(super) fn target(
    surface: Surface,
    depth: bool,
    mem: &impl Memory,
) -> Result<(u32, u32, u32, u32), &'static str> {
    let w = surface.horizontal >> 16;
    let h = surface.vertical >> 16;
    if surface.format & 0xFFFF != 0x128
        || surface.horizontal & 0xFFFF != 0
        || surface.vertical & 0xFFFF != 0
        || w == 0
        || h == 0
        || w > 4096
        || h > 4096
        || u64::from(w) * u64::from(h) > 4 * 1024 * 1024
    {
        return Err("raster exige surface BGRA8/Z24S8 linear limitada, origem zero");
    }
    let j = job(surface, mem, depth, 0xF3, [0, w - 1, 0, h - 1])?;
    Ok((j.base, j.pitch, w, h))
}
