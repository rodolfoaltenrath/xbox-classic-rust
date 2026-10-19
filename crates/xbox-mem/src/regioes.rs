//! Classificação do espaço de endereçamento **físico** do Xbox.
//!
//! Dado um [`GuestPhys`], responde o que existe ali: RAM, um dispositivo, a flash, ou
//! nada. É a tradução `GuestPhys → ArenaOff` do ADR-0009, e o único lugar do projeto
//! que conhece o mapa de dispositivos.
//!
//! ## ⚠️ Este mapa não está validado contra hardware
//!
//! As faixas vêm de documentação pública do console. Não existe como validá-las com os
//! arquivos que temos: endereço de dispositivo só se confirma com código do jogo
//! rodando. A validação vem nas Fases 3 e 4, pelo mecanismo de
//! [`crate::dispositivos::Telemetria`]: todo acesso a faixa desconhecida é registrado
//! com endereço, operação, tamanho e contexto, para comparação com a documentação.
//!
//! Por isso **nenhum comportamento de hardware é inventado aqui**. Esta camada só
//! classifica; o que fazer com um acesso a dispositivo é decisão de
//! [`crate::dispositivos`], e na Fase 2 a decisão é "registrar e seguir".

use crate::addr::{ArenaOff, GuestPhys};
use crate::dispositivos::Dispositivo;

/// O que existe em um endereço físico.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Regiao {
    /// RAM unificada. O deslocamento já é o da arena.
    Ram(ArenaOff),
    /// Registradores de um dispositivo.
    Dispositivo {
        /// Qual dispositivo.
        id: Dispositivo,
        /// Deslocamento dentro da janela do dispositivo.
        deslocamento: u32,
    },
    /// Flash ROM. Não bootamos BIOS (ADR-0001), então é só leitura de 0xFF.
    Flash {
        /// Deslocamento dentro da imagem espelhada.
        deslocamento: u32,
    },
    /// Memória *tiled*: faixa reconhecida e **não implementada**.
    ///
    /// Não é espelho da RAM: tiling é transformação de endereço feita pela GPU, então
    /// tratar como alias corromperia textura de forma difícil de diagnosticar. A
    /// transformação correta é trabalho da Fase 5, que é quem define o formato.
    Tiled {
        /// Deslocamento dentro da janela.
        deslocamento: u32,
    },
    /// Nada mapeado aqui.
    Buraco,
}

/// Uma faixa do mapa físico.
#[derive(Debug, Clone, Copy)]
struct Faixa {
    inicio: u32,
    /// Tamanho da faixa. Faixas de dispositivo usam o tamanho da janela documentada.
    tamanho: u32,
    tipo: TipoDeFaixa,
}

#[derive(Debug, Clone, Copy)]
enum TipoDeFaixa {
    Dispositivo(Dispositivo),
    Flash,
    Tiled,
}

/// Faixas que não são RAM, na ordem em que são testadas.
///
/// ⚠️ Valores de documentação pública, pendentes de validação — ver o aviso do módulo.
const FAIXAS: &[Faixa] = &[
    Faixa {
        inicio: 0xF000_0000,
        tamanho: 0x0400_0000,
        tipo: TipoDeFaixa::Tiled,
    },
    Faixa {
        inicio: 0xFD00_0000,
        tamanho: 0x0100_0000,
        tipo: TipoDeFaixa::Dispositivo(Dispositivo::Nv2a),
    },
    Faixa {
        inicio: 0xFE80_0000,
        tamanho: 0x0008_0000,
        tipo: TipoDeFaixa::Dispositivo(Dispositivo::Apu),
    },
    Faixa {
        inicio: 0xFEC0_0000,
        tamanho: 0x0000_1000,
        tipo: TipoDeFaixa::Dispositivo(Dispositivo::Ac97),
    },
    Faixa {
        inicio: 0xFED0_0000,
        tamanho: 0x0000_1000,
        tipo: TipoDeFaixa::Dispositivo(Dispositivo::UsbOhci0),
    },
    Faixa {
        inicio: 0xFED0_8000,
        tamanho: 0x0000_1000,
        tipo: TipoDeFaixa::Dispositivo(Dispositivo::UsbOhci1),
    },
    Faixa {
        inicio: 0xFEF0_0000,
        tamanho: 0x0000_0400,
        tipo: TipoDeFaixa::Dispositivo(Dispositivo::Nic),
    },
    Faixa {
        inicio: 0xFF00_0000,
        tamanho: 0x0100_0000,
        tipo: TipoDeFaixa::Flash,
    },
];

/// O mapa físico de um console com uma quantidade de RAM.
#[derive(Debug, Clone, Copy)]
pub struct MapaFisico {
    ram: u32,
}

impl MapaFisico {
    /// Mapa para uma arena de `ram` bytes.
    pub const fn novo(ram: usize) -> Self {
        Self { ram: ram as u32 }
    }

    /// Quantidade de RAM que este mapa expõe.
    pub const fn ram(self) -> u32 {
        self.ram
    }

    /// O que existe neste endereço físico.
    pub fn classificar(self, pa: GuestPhys) -> Regiao {
        if pa.0 < self.ram {
            return Regiao::Ram(ArenaOff(pa.0));
        }
        for faixa in FAIXAS {
            // Comparação sem `inicio + tamanho`: a faixa da flash termina em
            // 0x100000000, que **transborda** `u32`. A primeira versão somava e o teste
            // `faixas_sao_coerentes` pegou. `pa - inicio` só é avaliado depois de
            // `pa >= inicio`, então não transborda.
            if pa.0 >= faixa.inicio && pa.0 - faixa.inicio < faixa.tamanho {
                let deslocamento = pa.0 - faixa.inicio;
                return match faixa.tipo {
                    TipoDeFaixa::Dispositivo(id) => Regiao::Dispositivo { id, deslocamento },
                    TipoDeFaixa::Flash => Regiao::Flash { deslocamento },
                    TipoDeFaixa::Tiled => Regiao::Tiled { deslocamento },
                };
            }
        }
        Regiao::Buraco
    }

    /// A faixa inteira `[pa, pa+len)` cai na mesma região contígua de RAM?
    ///
    /// É o que o DMA da GPU precisa saber antes de pedir uma fatia (Fase 5).
    pub fn faixa_e_ram(self, pa: GuestPhys, len: u32) -> bool {
        match pa.checked_add(len) {
            Some(fim) => pa.0 < self.ram && fim.0 <= self.ram,
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FAIXAS, MapaFisico, Regiao};
    use crate::addr::{ArenaOff, GuestPhys};
    use crate::dispositivos::Dispositivo;

    const RAM: usize = 64 * 1024 * 1024;

    fn mapa() -> MapaFisico {
        MapaFisico::novo(RAM)
    }

    #[test]
    fn ram_cobre_do_zero_ao_limite() {
        let m = mapa();
        assert_eq!(m.classificar(GuestPhys(0)), Regiao::Ram(ArenaOff(0)));
        assert_eq!(
            m.classificar(GuestPhys(0x03FF_FFFF)),
            Regiao::Ram(ArenaOff(0x03FF_FFFF))
        );
        // Um byte depois do fim da RAM já não é RAM.
        assert_eq!(m.classificar(GuestPhys(0x0400_0000)), Regiao::Buraco);
    }

    #[test]
    fn ram128_estende_a_faixa() {
        let m = MapaFisico::novo(128 * 1024 * 1024);
        assert_eq!(
            m.classificar(GuestPhys(0x0400_0000)),
            Regiao::Ram(ArenaOff(0x0400_0000))
        );
        assert_eq!(m.classificar(GuestPhys(0x0800_0000)), Regiao::Buraco);
    }

    #[test]
    fn dispositivos_nos_lugares_documentados() {
        let m = mapa();
        for (endereco, esperado, deslocamento) in [
            (0xFD00_0000u32, Dispositivo::Nv2a, 0u32),
            (0xFD00_2000, Dispositivo::Nv2a, 0x2000),
            (0xFDFF_FFFF, Dispositivo::Nv2a, 0x00FF_FFFF),
            (0xFE80_0000, Dispositivo::Apu, 0),
            (0xFEC0_0004, Dispositivo::Ac97, 4),
            (0xFED0_0000, Dispositivo::UsbOhci0, 0),
            (0xFED0_8000, Dispositivo::UsbOhci1, 0),
            (0xFEF0_0010, Dispositivo::Nic, 0x10),
        ] {
            assert_eq!(
                m.classificar(GuestPhys(endereco)),
                Regiao::Dispositivo {
                    id: esperado,
                    deslocamento
                },
                "endereço 0x{endereco:08X}"
            );
        }
    }

    #[test]
    fn flash_e_tiled_sao_reconhecidos() {
        let m = mapa();
        assert_eq!(
            m.classificar(GuestPhys(0xFF00_0000)),
            Regiao::Flash { deslocamento: 0 }
        );
        assert_eq!(
            m.classificar(GuestPhys(0xFFFF_FFFF)),
            Regiao::Flash {
                deslocamento: 0x00FF_FFFF
            }
        );
        assert_eq!(
            m.classificar(GuestPhys(0xF000_1000)),
            Regiao::Tiled {
                deslocamento: 0x1000
            }
        );
    }

    #[test]
    fn buracos_entre_dispositivos() {
        let m = mapa();
        for endereco in [
            0x0400_0000u32,
            0x7FFF_FFFF,
            0xFC00_0000,
            0xFE00_0000,
            0xFE88_0000,
            0xFEC0_1000,
            0xFED0_1000,
            0xFEF0_0400,
        ] {
            assert_eq!(
                m.classificar(GuestPhys(endereco)),
                Regiao::Buraco,
                "endereço 0x{endereco:08X} deveria ser buraco"
            );
        }
    }

    #[test]
    fn faixas_sao_coerentes() {
        // Contas em u64 de propósito: a faixa da flash termina em 0x100000000, que não
        // cabe em u32 — foi este teste que pegou a soma transbordando em
        // `classificar`.
        for (i, a) in FAIXAS.iter().enumerate() {
            assert!(a.tamanho > 0, "faixa {i} com tamanho zero");
            let (ai, af) = (
                u64::from(a.inicio),
                u64::from(a.inicio) + u64::from(a.tamanho),
            );
            assert!(af <= 0x1_0000_0000, "faixa {i} passa do espaço de 32 bits");
            for (j, b) in FAIXAS.iter().enumerate() {
                if i == j {
                    continue;
                }
                let (bi, bf) = (
                    u64::from(b.inicio),
                    u64::from(b.inicio) + u64::from(b.tamanho),
                );
                assert!(!(ai < bf && bi < af), "faixas {i} e {j} se sobrepõem");
            }
        }
    }

    #[test]
    fn faixa_de_dma_precisa_estar_inteira_na_ram() {
        let m = mapa();
        assert!(m.faixa_e_ram(GuestPhys(0), 4096));
        assert!(m.faixa_e_ram(GuestPhys(0x03FF_F000), 4096));
        assert!(
            !m.faixa_e_ram(GuestPhys(0x03FF_F000), 4097),
            "passa do fim da RAM"
        );
        assert!(!m.faixa_e_ram(GuestPhys(0xFD00_0000), 4));
        assert!(!m.faixa_e_ram(GuestPhys(0xFFFF_FFFC), 8), "transbordo");
    }
}
