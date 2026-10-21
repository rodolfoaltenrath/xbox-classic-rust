//! Alocador de frames físicos de 4 KiB.
//!
//! Quem decide onde na RAM uma página vai morar. O kernel HLE (Fase 4) usa isto por
//! baixo de `MmAllocateContiguousMemory` e companhia; na Fase 2 ele serve ao loader de
//! XBE, que precisa de frames para as seções do título.
//!
//! Bitmap simples, com busca linear. É o suficiente: 64 MiB são 16.384 frames, o
//! bitmap tem 2 KiB e cabe em cache. Otimizar antes de medir é o erro que o ADR-0010
//! corrigiu.

use xbox_core::error::SnapshotError;
use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_core::{Error, Result};

use crate::addr::GuestPhys;

/// Bits por palavra do bitmap.
const BITS: u32 = 64;

/// Alocador de frames de 4 KiB sobre a RAM.
#[derive(Debug, Clone)]
pub struct AlocadorDeFrames {
    bitmap: Vec<u64>,
    frames: u32,
    usados: u32,
}

impl AlocadorDeFrames {
    /// Alocador para uma RAM de `ram` bytes.
    pub fn novo(ram: usize) -> Self {
        let frames = (ram / 4096) as u32;
        Self {
            bitmap: vec![0; frames.div_ceil(BITS) as usize],
            frames,
            usados: 0,
        }
    }

    /// Quantidade total de frames.
    pub const fn total(&self) -> u32 {
        self.frames
    }

    /// Frames em uso.
    pub const fn usados(&self) -> u32 {
        self.usados
    }

    /// Frames livres.
    pub const fn livres(&self) -> u32 {
        self.frames - self.usados
    }

    /// O frame está em uso?
    pub fn em_uso(&self, frame: u32) -> bool {
        if frame >= self.frames {
            return true; // fora da RAM conta como indisponível
        }
        let (palavra, bit) = (frame / BITS, frame % BITS);
        self.bitmap[palavra as usize] & (1 << bit) != 0
    }

    /// Aloca um frame.
    pub fn alocar(&mut self) -> Option<GuestPhys> {
        self.alocar_contiguo(1)
    }

    /// Aloca `n` frames contíguos.
    ///
    /// Contiguidade física é requisito real: é o que `MmAllocateContiguousMemory`
    /// promete, e é o que a GPU precisa para fazer DMA de um buffer inteiro.
    pub fn alocar_contiguo(&mut self, n: u32) -> Option<GuestPhys> {
        if n == 0 || n > self.frames {
            return None;
        }
        let mut inicio = 0u32;
        while inicio + n <= self.frames {
            if let Some(ocupado) = (inicio..inicio + n).find(|&f| self.em_uso(f)) {
                // Achou um ocupado: pula para depois dele.
                inicio = ocupado + 1;
            } else {
                for f in inicio..inicio + n {
                    self.marcar(f, true);
                }
                self.usados += n;
                return Some(GuestPhys::do_frame(inicio));
            }
        }
        None
    }

    /// Reserva uma faixa específica, para carregar algo em endereço físico fixo.
    ///
    /// Erra se qualquer frame da faixa já estiver em uso — em vez de sobrescrever em
    /// silêncio, que esconderia um bug de layout do loader.
    pub fn reservar(&mut self, pa: GuestPhys, n: u32) -> Result<()> {
        if pa.deslocamento() != 0 {
            return Err(Error::format(format!("{pa} não está alinhado em página")));
        }
        let inicio = pa.frame();
        let fim = inicio
            .checked_add(n)
            .ok_or_else(|| Error::format("faixa de frames transborda"))?;
        if fim > self.frames {
            return Err(Error::format(format!(
                "faixa de {n} frames em {pa} passa do fim da RAM ({} frames)",
                self.frames
            )));
        }
        if let Some(f) = (inicio..fim).find(|&f| self.em_uso(f)) {
            return Err(Error::format(format!(
                "frame {f} ({}) já está em uso",
                GuestPhys::do_frame(f)
            )));
        }
        for f in inicio..fim {
            self.marcar(f, true);
        }
        self.usados += n;
        Ok(())
    }

    /// Libera `n` frames a partir de `pa`.
    pub fn liberar(&mut self, pa: GuestPhys, n: u32) {
        let inicio = pa.frame();
        for f in inicio..inicio.saturating_add(n).min(self.frames) {
            if self.em_uso(f) {
                self.marcar(f, false);
                self.usados -= 1;
            }
        }
    }

    fn marcar(&mut self, frame: u32, usado: bool) {
        let (palavra, bit) = ((frame / BITS) as usize, frame % BITS);
        if usado {
            self.bitmap[palavra] |= 1 << bit;
        } else {
            self.bitmap[palavra] &= !(1 << bit);
        }
    }
}

impl Snapshot for AlocadorDeFrames {
    fn save(&self, out: &mut SnapshotWriter) {
        out.section("FRM0", 1);
        out.u32(self.frames);
        out.u32(self.usados);
        out.u64(self.bitmap.len() as u64);
        for palavra in &self.bitmap {
            out.u64(*palavra);
        }
    }

    fn load(&mut self, inp: &mut SnapshotReader<'_>) -> std::result::Result<(), SnapshotError> {
        if inp.section("FRM0", 1)? != 1 {
            return Err(SnapshotError::BadValue {
                section: "FRM0",
                what: "versão de frames desconhecida",
            });
        }
        let frames = inp.u32()?;
        if frames != self.frames {
            return Err(SnapshotError::BadValue {
                section: "FRM0",
                what: "quantidade de frames diferente do build atual",
            });
        }
        let usados = inp.u32()?;
        let n = inp.u64()? as usize;
        if n != self.bitmap.len() {
            return Err(SnapshotError::BadValue {
                section: "FRM0",
                what: "tamanho de bitmap inconsistente",
            });
        }
        let mut bitmap = Vec::with_capacity(n);
        for _ in 0..n {
            bitmap.push(inp.u64()?);
        }
        let count: u32 = bitmap.iter().map(|word| word.count_ones()).sum();
        let padding = frames % BITS;
        if usados > frames
            || count != usados
            || (padding != 0 && bitmap.last().is_some_and(|word| word >> padding != 0))
        {
            return Err(SnapshotError::BadValue {
                section: "FRM0",
                what: "bitmap e contagem de frames inconsistentes",
            });
        }
        self.bitmap = bitmap;
        self.usados = usados;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::AlocadorDeFrames;
    use crate::addr::GuestPhys;
    use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};

    fn alocador() -> AlocadorDeFrames {
        AlocadorDeFrames::novo(1024 * 1024) // 256 frames
    }

    #[test]
    fn aloca_e_libera() {
        let mut a = alocador();
        assert_eq!(a.total(), 256);
        assert_eq!(a.livres(), 256);

        let p = a.alocar().unwrap();
        assert_eq!(p, GuestPhys(0));
        assert_eq!(a.usados(), 1);
        assert!(a.em_uso(0));

        a.liberar(p, 1);
        assert_eq!(a.usados(), 0);
        assert!(!a.em_uso(0));
    }

    #[test]
    fn alocacoes_nao_se_sobrepoem() {
        let mut a = alocador();
        let p1 = a.alocar_contiguo(3).unwrap();
        let p2 = a.alocar_contiguo(2).unwrap();
        assert_eq!(p1, GuestPhys(0));
        assert_eq!(p2, GuestPhys(3 * 4096));
        assert_eq!(a.usados(), 5);
    }

    #[test]
    fn contiguidade_e_respeitada_apos_liberacao() {
        let mut a = alocador();
        let _p0 = a.alocar().unwrap(); // frame 0
        let p1 = a.alocar().unwrap(); // frame 1
        let _p2 = a.alocar().unwrap(); // frame 2
        a.liberar(p1, 1); // abre um buraco de 1 frame

        // Dois contíguos não cabem no buraco, então vão para depois do 2.
        let p = a.alocar_contiguo(2).unwrap();
        assert_eq!(p, GuestPhys(3 * 4096));
        // Mas um sozinho reaproveita o buraco.
        assert_eq!(a.alocar().unwrap(), GuestPhys(4096));
    }

    #[test]
    fn esgotamento_devolve_none() {
        let mut a = AlocadorDeFrames::novo(4 * 4096);
        assert!(a.alocar_contiguo(4).is_some());
        assert!(a.alocar().is_none());
        assert!(a.alocar_contiguo(5).is_none(), "mais que a RAM inteira");
        assert_eq!(a.livres(), 0);
    }

    #[test]
    fn reservar_faixa_fixa() {
        let mut a = alocador();
        a.reservar(GuestPhys(0x1_0000), 4).unwrap();
        assert!(a.em_uso(0x10));
        assert!(a.em_uso(0x13));
        assert!(!a.em_uso(0x14));

        // Reservar de novo tem de falhar, não sobrescrever.
        assert!(a.reservar(GuestPhys(0x1_0000), 1).is_err());
        // Desalinhado falha.
        assert!(a.reservar(GuestPhys(0x1_0001), 1).is_err());
        // Fora da RAM falha.
        assert!(a.reservar(GuestPhys(0x100_0000), 1).is_err());
    }

    #[test]
    fn frame_fora_da_ram_conta_como_indisponivel() {
        let a = alocador();
        assert!(a.em_uso(256), "frame além do fim não pode parecer livre");
        assert!(a.em_uso(u32::MAX));
    }

    #[test]
    fn snapshot_recusa_contagem_padding_e_truncamento_sem_alterar_alocador() {
        let mut a = AlocadorDeFrames::novo(3 * 4096);
        a.alocar().unwrap();
        let save = |a: &AlocadorDeFrames| {
            let mut out = SnapshotWriter::new();
            a.save(&mut out);
            out.finish()
        };
        let original = save(&a);
        for (used, bitmap) in [(4u32, 1u64), (0, 1), (1, 8)] {
            let mut out = SnapshotWriter::new();
            out.section("FRM0", 1);
            out.u32(3);
            out.u32(used);
            out.u64(1);
            out.u64(bitmap);
            assert!(
                a.load(&mut SnapshotReader::new(&out.finish()).unwrap())
                    .is_err()
            );
            assert_eq!(save(&a), original);
        }
        assert!(
            a.load(&mut SnapshotReader::new(&original[..original.len() - 1]).unwrap())
                .is_err()
        );
        assert_eq!(save(&a), original);
    }

    #[test]
    fn snapshot_ida_e_volta() {
        let mut a = alocador();
        a.alocar_contiguo(5).unwrap();
        a.reservar(GuestPhys(0x2_0000), 2).unwrap();

        let mut w = SnapshotWriter::new();
        a.save(&mut w);
        let bytes = w.finish();

        let mut b = alocador();
        let mut r = SnapshotReader::new(&bytes).unwrap();
        b.load(&mut r).unwrap();

        assert_eq!(b.usados(), a.usados());
        for f in 0..b.total() {
            assert_eq!(b.em_uso(f), a.em_uso(f), "frame {f}");
        }

        let mut w2 = SnapshotWriter::new();
        b.save(&mut w2);
        assert_eq!(bytes, w2.finish());
    }
}
