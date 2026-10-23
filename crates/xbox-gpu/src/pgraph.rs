//! PGRAPH serial: traps de software e surfaces lineares (ADRs 0066/0067).
mod clear;
mod persistence;

pub const PGRAPH_ERROR: u32 = 1 << 20;
pub const PMC_PGRAPH_PENDING: u32 = 1 << 12;
/// Adaptador DMA: offsets de instância são relativos à PRAMIN; RAM usa endereço físico.
/// A RAM é linear, estável e exclusiva durante a chamada; DWORDs alinhados em faixa
/// precisam ser legíveis/graváveis. Nenhuma implementação pode acessar MMIO como RAM.
pub trait Memory {
    fn ram_len(&self) -> u64;
    fn read_instance(&self, offset: u32) -> Result<u32, &'static str>;
    fn read_ram(&self, physical: u32) -> Result<u32, &'static str>;
    fn write_ram(&mut self, physical: u32, value: u32) -> Result<(), &'static str>;
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PgraphProfile {
    LegacyUnmodeled,
    ClearV1,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClearStats {
    pub calls: u64,
    pub pixels: u64,
    pub first_color: Option<u32>,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Surface {
    notifier_dma: u32,
    color_dma: u32,
    zeta_dma: u32,
    horizontal: u32,
    vertical: u32,
    format: u32,
    pitch: u32,
    color_offset: u32,
    zeta_offset: u32,
    z_clear: u32,
    color_clear: u32,
    rect_h: u32,
    rect_v: u32,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct State {
    pending: u32,
    enabled: u32,
    trapped_address: u32,
    trapped_data: u32,
    ctx_switch1: u32,
    fifo: u32,
    scanout: Option<u32>,
    surface: Surface,
    stats: ClearStats,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nv2aPgraph {
    state: Option<State>,
}
impl Default for Nv2aPgraph {
    fn default() -> Self {
        Self {
            state: Some(State::default()),
        }
    }
}
impl Nv2aPgraph {
    pub const fn legacy() -> Self {
        Self { state: None }
    }
    pub const fn profile(&self) -> PgraphProfile {
        if self.state.is_some() {
            PgraphProfile::ClearV1
        } else {
            PgraphProfile::LegacyUnmodeled
        }
    }
    pub fn scanout(&self) -> Option<u32> {
        self.state.as_ref().and_then(|s| s.scanout)
    }
    pub fn stats(&self) -> ClearStats {
        self.state
            .as_ref()
            .map_or(ClearStats::default(), |s| s.stats)
    }
    pub fn irq_pending(&self) -> bool {
        self.state
            .as_ref()
            .is_some_and(|s| s.pending & s.enabled != 0)
    }
    pub fn ready(&self) -> bool {
        self.state
            .as_ref()
            .is_none_or(|s| s.pending == 0 && s.fifo & 1 != 0)
    }
    /// Setter HLE de `AvSetDisplayMode`, sem promover o perfil legado.
    pub fn display_start(&mut self, address: u32, ram_len: u64) -> Result<(), &'static str> {
        if let Some(s) = &mut self.state {
            validate_start(address, ram_len)?;
            s.scanout = Some(address);
        }
        Ok(())
    }
    pub fn models(&self, class: u32, method: u16) -> bool {
        self.state.is_some()
            && class == 0x97
            && matches!(
                method,
                0x100
                    | 0x110
                    | 0x180
                    | 0x194
                    | 0x198
                    | 0x200
                    | 0x204
                    | 0x208
                    | 0x20C
                    | 0x210
                    | 0x214
                    | 0x1D8C
                    | 0x1D90
                    | 0x1D94
                    | 0x1D98
                    | 0x1D9C
            )
    }
    /// `resolved` já passou pela RAMHT; o trap não interpreta códigos da biblioteca guest.
    pub fn execute(
        &mut self,
        class: u32,
        sub: u8,
        method: u16,
        raw: u32,
        resolved: u32,
        memory: &mut impl Memory,
    ) -> Result<(), &'static str> {
        let Some(s) = &mut self.state else {
            return Ok(());
        };
        if class != 0x97 {
            return Ok(());
        }
        match method {
            0x100 if raw != 0 => {
                if s.pending != 0 {
                    return Err("novo trap PGRAPH antes de ACK");
                }
                s.ctx_switch1 = (s.ctx_switch1 & !0xFFF) | class;
                s.trapped_address = (u32::from(sub) << 16) | u32::from(method);
                s.trapped_data = raw;
                s.pending = PGRAPH_ERROR;
            }
            0x194 => s.surface.color_dma = resolved,
            0x198 => s.surface.zeta_dma = resolved,
            0x180 => s.surface.notifier_dma = resolved,
            0x200 => s.surface.horizontal = raw,
            0x204 => s.surface.vertical = raw,
            0x208 => s.surface.format = raw,
            0x20C => s.surface.pitch = raw,
            0x210 => s.surface.color_offset = raw,
            0x214 => s.surface.zeta_offset = raw,
            0x1D8C => s.surface.z_clear = raw,
            0x1D90 => s.surface.color_clear = raw,
            0x1D98 => s.surface.rect_h = raw,
            0x1D9C => s.surface.rect_v = raw,
            0x1D94 => {
                let calls = s
                    .stats
                    .calls
                    .checked_add(1)
                    .ok_or("contador clear esgotado")?;
                let pixels = clear::apply(s.surface, raw, memory, u64::MAX - s.stats.pixels)?;
                s.stats.calls = calls;
                s.stats.pixels += pixels;
                if raw & 0xF0 != 0 && s.stats.first_color.is_none() {
                    s.stats.first_color = Some(s.surface.color_clear);
                }
            }
            _ => (),
        }
        Ok(())
    }
    fn mmio_word(&self, word: u32) -> bool {
        self.state.is_some()
            && matches!(
                word,
                0x400100
                    | 0x400108
                    | 0x400140
                    | 0x40014C
                    | 0x400700
                    | 0x400704
                    | 0x400708
                    | 0x40070C
                    | 0x400720
                    | 0x401A88
                    | 0x40186C
                    | 0x600800
            )
    }
    pub fn validate_access(
        &self,
        offset: u32,
        len: usize,
        write: bool,
    ) -> Result<bool, &'static str> {
        if self.state.is_none() || len == 0 {
            return Ok(false);
        }
        let word = offset & !3;
        if !self.mmio_word(word) {
            let end = u64::from(offset).saturating_add(len as u64);
            for r in [
                0x400100u32,
                0x400108,
                0x400140,
                0x40014C,
                0x400700,
                0x400704,
                0x400708,
                0x40070C,
                0x400720,
                0x401A88,
                0x40186C,
                0x600800,
            ] {
                if u64::from(offset) < u64::from(r) + 4 && u64::from(r) < end {
                    return Err("acesso cruza registrador PGRAPH modelado");
                }
            }
            return Ok(false);
        }
        if len != 4 || word != offset {
            return Err("PGRAPH/scanout exige DWORD alinhado");
        }
        if write && matches!(word, 0x400108 | 0x400700 | 0x400704 | 0x400708 | 0x40070C) {
            return Err("registrador de trap/status PGRAPH somente leitura");
        }
        Ok(true)
    }
    pub fn validate_value(
        &self,
        offset: u32,
        value: u32,
        ram_len: u64,
    ) -> Result<(), &'static str> {
        if self.state.is_some() && offset == 0x600800 {
            validate_start(value, ram_len)?;
        }
        Ok(())
    }
    pub fn read(&self, offset: u32, len: usize) -> Result<Option<u32>, &'static str> {
        if !self.validate_access(offset, len, false)? {
            return Ok(None);
        }
        let s = self.state.as_ref().expect("perfil");
        Ok(Some(match offset {
            0x400100 => s.pending,
            0x400108 => u32::from(s.pending != 0),
            0x400140 => s.enabled,
            0x40014C => s.ctx_switch1,
            0x400700 | 0x40070C => 0,
            0x400704 => s.trapped_address,
            0x400708 => s.trapped_data,
            0x400720 => s.fifo,
            0x401A88 => s.surface.z_clear,
            0x40186C => s.surface.color_clear,
            0x600800 => s.scanout.unwrap_or(0),
            _ => unreachable!("validado"),
        }))
    }
    pub fn write(&mut self, offset: u32, value: u32) {
        let s = self.state.as_mut().expect("perfil validado");
        match offset {
            0x400100 => s.pending &= !value,
            0x400140 => s.enabled = value,
            0x40014C => s.ctx_switch1 = value,
            0x400720 => s.fifo = value,
            0x401A88 => s.surface.z_clear = value,
            0x40186C => s.surface.color_clear = value,
            0x600800 => s.scanout = Some(value),
            _ => unreachable!("store validado"),
        }
    }
    pub fn validate_memory(&self, ram_len: u64) -> Result<(), &'static str> {
        if let Some(at) = self.scanout() {
            validate_start(at, ram_len)?;
        }
        Ok(())
    }
}
fn validate_start(value: u32, ram_len: u64) -> Result<(), &'static str> {
    if value & 3 != 0 || u64::from(value) + 4 > ram_len {
        return Err("scanout fora de RAM linear alinhada");
    }
    Ok(())
}
