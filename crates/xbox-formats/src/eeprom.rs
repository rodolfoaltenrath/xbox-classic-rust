//! EEPROM do console: 256 bytes com região, idioma, vídeo, áudio e identidade.
//!
//! O jogo nunca lê a EEPROM diretamente — ele chama `ExQueryNonVolatileSetting`, e do
//! outro lado dessa chamada vai estar o **nosso** kernel (ADR-0001). Por isso a
//! EEPROM, aqui, é sobretudo uma estrutura de configuração persistente que nós mesmos
//! geramos: região NTSC ou PAL, idioma, 480p/widescreen, modo de áudio.
//!
//! ## Três seções
//!
//! | Faixa | Conteúdo | Nosso tratamento |
//! |---|---|---|
//! | `0x00..0x30` | hash HMAC-SHA1 + bloco cifrado (confounder, chave do HD, região) | lido cru; **não decifrado** nesta fase |
//! | `0x30..0x60` | configuração de fábrica: serial, MAC, chave online, padrão de vídeo | lido e escrito |
//! | `0x60..0xC0` | configuração do usuário: fuso, idioma, flags de vídeo e áudio, controle parental, rede | lido e escrito |
//!
//! ## O que ainda não fazemos: decifrar
//!
//! O bloco `0x14..0x30` de uma EEPROM real é cifrado com RC4, com chave derivada de
//! HMAC-SHA1 — e a chave depende da versão do kernel do console. Precisaríamos de RC4,
//! SHA-1 e HMAC, que são justamente as funções `Xc*` que o kernel HLE vai implementar
//! na Fase 4. Então:
//!
//! - **ler um dump real:** os campos em claro (fábrica e usuário) funcionam; a região e
//!   a chave do HD ficam indisponíveis até a Fase 4;
//! - **gerar uma EEPROM nossa:** escrevemos o bloco em claro. Um console de verdade
//!   recusaria; o nosso kernel não, e não existe caminho em que isso chegue a hardware.
//!
//! ## ⚠️ Conformidade
//!
//! Os deslocamentos abaixo vêm de documentação pública e **não foram verificados contra
//! um dump real** — ao contrário do XBE e do XDVDFS, que foram. Esta é a parte menos
//! verificada do crate, e está registrada como item aberto em `docs/compat/formatos.md`.
//! O risco é contido: nada fora do nosso próprio kernel consome esses campos.

use xbox_core::{Error, Result};

use crate::bytes::{latin1, slice_at, u32_at};

/// Tamanho de uma EEPROM.
pub const TAMANHO: usize = 256;

/// Início do bloco cifrado.
pub const OFF_BLOCO_CIFRADO: usize = 0x14;
/// Fim do bloco cifrado (exclusivo).
pub const OFF_FIM_CIFRADO: usize = 0x30;

// Configuração de fábrica.
const OFF_CHECKSUM_FABRICA: usize = 0x30;
const OFF_SERIAL: usize = 0x34;
const OFF_MAC: usize = 0x40;
const OFF_CHAVE_ONLINE: usize = 0x48;
const OFF_PADRAO_VIDEO: usize = 0x58;

// Configuração do usuário.
const OFF_CHECKSUM_USUARIO: usize = 0x60;
const OFF_FUSO_BIAS: usize = 0x64;
const OFF_IDIOMA: usize = 0x90;
const OFF_FLAGS_VIDEO: usize = 0x94;
const OFF_FLAGS_AUDIO: usize = 0x98;
const OFF_CONTROLE_JOGOS: usize = 0x9C;
const OFF_CONTROLE_SENHA: usize = 0xA0;
const OFF_CONTROLE_FILMES: usize = 0xA4;
const OFF_IP: usize = 0xA8;
const OFF_DNS: usize = 0xAC;
const OFF_GATEWAY: usize = 0xB0;
const OFF_MASCARA: usize = 0xB4;
const OFF_FLAGS_DIVERSAS: usize = 0xB8;
const OFF_REGIAO_DVD: usize = 0xBC;

/// Padrão de vídeo gravado na EEPROM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadraoDeVideo {
    /// NTSC americano, 60 Hz.
    NtscM,
    /// NTSC japonês, 60 Hz.
    NtscJ,
    /// PAL europeu, 50 Hz.
    PalI,
    /// Valor que não reconhecemos, preservado como está.
    Desconhecido(u32),
}

impl PadraoDeVideo {
    /// Valor de NTSC-M.
    pub const VALOR_NTSC_M: u32 = 0x0040_0100;
    /// Valor de NTSC-J.
    pub const VALOR_NTSC_J: u32 = 0x0040_0200;
    /// Valor de PAL-I.
    pub const VALOR_PAL_I: u32 = 0x0080_0300;

    /// Interpreta o valor cru.
    pub const fn de_valor(v: u32) -> Self {
        match v {
            Self::VALOR_NTSC_M => Self::NtscM,
            Self::VALOR_NTSC_J => Self::NtscJ,
            Self::VALOR_PAL_I => Self::PalI,
            outro => Self::Desconhecido(outro),
        }
    }

    /// Valor cru.
    pub const fn valor(self) -> u32 {
        match self {
            Self::NtscM => Self::VALOR_NTSC_M,
            Self::NtscJ => Self::VALOR_NTSC_J,
            Self::PalI => Self::VALOR_PAL_I,
            Self::Desconhecido(v) => v,
        }
    }

    /// Taxa de vblank correspondente, em milihertz.
    pub const fn vblank_millihz(self) -> u64 {
        match self {
            Self::PalI => 50_000,
            _ => 59_940,
        }
    }

    /// Nome legível.
    pub const fn nome(self) -> &'static str {
        match self {
            Self::NtscM => "NTSC-M",
            Self::NtscJ => "NTSC-J",
            Self::PalI => "PAL-I",
            Self::Desconhecido(_) => "desconhecido",
        }
    }
}

/// Idioma do dashboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Idioma {
    Nenhum,
    Ingles,
    Japones,
    Alemao,
    Frances,
    Espanhol,
    Italiano,
    Coreano,
    Chines,
    Portugues,
    /// Valor fora da tabela conhecida.
    Desconhecido(u32),
}

impl Idioma {
    /// Interpreta o valor cru.
    pub const fn de_valor(v: u32) -> Self {
        match v {
            0 => Self::Nenhum,
            1 => Self::Ingles,
            2 => Self::Japones,
            3 => Self::Alemao,
            4 => Self::Frances,
            5 => Self::Espanhol,
            6 => Self::Italiano,
            7 => Self::Coreano,
            8 => Self::Chines,
            9 => Self::Portugues,
            outro => Self::Desconhecido(outro),
        }
    }

    /// Valor cru.
    pub const fn valor(self) -> u32 {
        match self {
            Self::Nenhum => 0,
            Self::Ingles => 1,
            Self::Japones => 2,
            Self::Alemao => 3,
            Self::Frances => 4,
            Self::Espanhol => 5,
            Self::Italiano => 6,
            Self::Coreano => 7,
            Self::Chines => 8,
            Self::Portugues => 9,
            Self::Desconhecido(v) => v,
        }
    }
}

/// Flags de vídeo (resoluções aceitas e proporção).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlagsDeVideo(pub u32);

impl FlagsDeVideo {
    /// Tela widescreen 16:9.
    pub const WIDESCREEN: u32 = 0x0001_0000;
    /// Aceita 480p.
    pub const MODO_480P: u32 = 0x0008_0000;
    /// Aceita 720p.
    pub const MODO_720P: u32 = 0x0002_0000;
    /// Aceita 1080i.
    pub const MODO_1080I: u32 = 0x0004_0000;
    /// Saída em letterbox.
    pub const LETTERBOX: u32 = 0x0010_0000;

    /// Testa um bit.
    pub const fn tem(self, bit: u32) -> bool {
        self.0 & bit != 0
    }

    /// Nomes dos bits ligados.
    pub fn nomes(self) -> Vec<&'static str> {
        let mut v = Vec::new();
        for (bit, nome) in [
            (Self::WIDESCREEN, "widescreen"),
            (Self::LETTERBOX, "letterbox"),
            (Self::MODO_480P, "480p"),
            (Self::MODO_720P, "720p"),
            (Self::MODO_1080I, "1080i"),
        ] {
            if self.tem(bit) {
                v.push(nome);
            }
        }
        v
    }
}

/// EEPROM decodificada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Eeprom {
    /// Os 256 bytes como estão.
    pub cru: [u8; TAMANHO],
    /// Número de série de fábrica (12 caracteres).
    pub serial: String,
    /// Endereço MAC.
    pub mac: [u8; 6],
    /// Padrão de vídeo.
    pub padrao_de_video: PadraoDeVideo,
    /// Idioma do dashboard.
    pub idioma: Idioma,
    /// Flags de vídeo.
    pub flags_de_video: FlagsDeVideo,
    /// Flags de áudio.
    pub flags_de_audio: u32,
    /// Região de DVD.
    pub regiao_dvd: u32,
    /// Deslocamento de fuso horário, em minutos.
    pub fuso_bias: u32,
}

impl Eeprom {
    /// Decodifica os 256 bytes.
    pub fn parse(dados: &[u8]) -> Result<Self> {
        if dados.len() != TAMANHO {
            return Err(Error::format(format!(
                "EEPROM tem {} bytes; esperado exatamente {TAMANHO}",
                dados.len()
            )));
        }
        let mut cru = [0u8; TAMANHO];
        cru.copy_from_slice(dados);

        let mut mac = [0u8; 6];
        mac.copy_from_slice(slice_at(dados, OFF_MAC, 6)?);

        Ok(Self {
            cru,
            serial: latin1(slice_at(dados, OFF_SERIAL, 12)?),
            mac,
            padrao_de_video: PadraoDeVideo::de_valor(u32_at(dados, OFF_PADRAO_VIDEO)?),
            idioma: Idioma::de_valor(u32_at(dados, OFF_IDIOMA)?),
            flags_de_video: FlagsDeVideo(u32_at(dados, OFF_FLAGS_VIDEO)?),
            flags_de_audio: u32_at(dados, OFF_FLAGS_AUDIO)?,
            regiao_dvd: u32_at(dados, OFF_REGIAO_DVD)?,
            fuso_bias: u32_at(dados, OFF_FUSO_BIAS)?,
        })
    }

    /// Bloco cifrado, como está no arquivo.
    ///
    /// Em uma EEPROM gerada por nós este bloco está **em claro**; em um dump real ele
    /// está cifrado e só será legível quando o `Xc*` da Fase 4 existir.
    pub fn bloco_cifrado(&self) -> &[u8] {
        &self.cru[OFF_BLOCO_CIFRADO..OFF_FIM_CIFRADO]
    }

    /// Os dois checksums que sabemos calcular conferem?
    ///
    /// Em EEPROM gerada por nós, sim. Em dump real, serve como pista de que a nossa
    /// fórmula de checksum (ou os deslocamentos) está certa — e é por isso que o
    /// resultado é informativo, não fatal.
    pub fn checksums_conferem(&self) -> bool {
        let fabrica = u32::from_le_bytes([
            self.cru[OFF_CHECKSUM_FABRICA],
            self.cru[OFF_CHECKSUM_FABRICA + 1],
            self.cru[OFF_CHECKSUM_FABRICA + 2],
            self.cru[OFF_CHECKSUM_FABRICA + 3],
        ]);
        let usuario = u32::from_le_bytes([
            self.cru[OFF_CHECKSUM_USUARIO],
            self.cru[OFF_CHECKSUM_USUARIO + 1],
            self.cru[OFF_CHECKSUM_USUARIO + 2],
            self.cru[OFF_CHECKSUM_USUARIO + 3],
        ]);
        fabrica == checksum(&self.cru[OFF_CHECKSUM_FABRICA + 4..OFF_CHECKSUM_USUARIO])
            && usuario == checksum(&self.cru[OFF_CHECKSUM_USUARIO + 4..TAMANHO])
    }

    /// Serializa de volta para 256 bytes, recalculando os checksums.
    pub fn bytes(&self) -> [u8; TAMANHO] {
        let mut saida = self.cru;
        escrever_u32(&mut saida, OFF_PADRAO_VIDEO, self.padrao_de_video.valor());
        escrever_u32(&mut saida, OFF_IDIOMA, self.idioma.valor());
        escrever_u32(&mut saida, OFF_FLAGS_VIDEO, self.flags_de_video.0);
        escrever_u32(&mut saida, OFF_FLAGS_AUDIO, self.flags_de_audio);
        escrever_u32(&mut saida, OFF_REGIAO_DVD, self.regiao_dvd);
        escrever_u32(&mut saida, OFF_FUSO_BIAS, self.fuso_bias);

        let serial = self.serial.as_bytes();
        for i in 0..12 {
            saida[OFF_SERIAL + i] = serial.get(i).copied().unwrap_or(0);
        }
        saida[OFF_MAC..OFF_MAC + 6].copy_from_slice(&self.mac);

        let c1 = checksum(&saida[OFF_CHECKSUM_FABRICA + 4..OFF_CHECKSUM_USUARIO]);
        escrever_u32(&mut saida, OFF_CHECKSUM_FABRICA, c1);
        let c2 = checksum(&saida[OFF_CHECKSUM_USUARIO + 4..TAMANHO]);
        escrever_u32(&mut saida, OFF_CHECKSUM_USUARIO, c2);
        saida
    }

    /// Gera uma EEPROM sintética coerente.
    ///
    /// É o que o emulador usa na primeira execução: ninguém deveria precisar de um dump
    /// de console para jogar.
    pub fn sintetica(padrao: PadraoDeVideo, idioma: Idioma) -> Self {
        let mut cru = [0u8; TAMANHO];

        // Bloco "cifrado" em claro: confounder, chave de HD e região.
        // Valores fixos e obviamente sintéticos — não imitam um console real.
        for i in 0..8 {
            cru[OFF_BLOCO_CIFRADO + i] = 0x4F; // 'O', de Obsidian
        }
        for i in 0..16 {
            cru[OFF_BLOCO_CIFRADO + 8 + i] = 0x30 + i as u8;
        }
        let regiao_xbe = match padrao {
            PadraoDeVideo::NtscJ => 2,
            PadraoDeVideo::PalI => 4,
            _ => 1,
        };
        escrever_u32(&mut cru, 0x2C, regiao_xbe);

        // Fábrica.
        let serial = b"OBSIDIAN0001";
        cru[OFF_SERIAL..OFF_SERIAL + 12].copy_from_slice(serial);
        // MAC com o bit "local" ligado e OUI zerado: não colide com fabricante real.
        cru[OFF_MAC..OFF_MAC + 6].copy_from_slice(&[0x02, 0x00, 0x00, 0x0B, 0x51, 0xD1]);
        for i in 0..16 {
            cru[OFF_CHAVE_ONLINE + i] = 0;
        }
        escrever_u32(&mut cru, OFF_PADRAO_VIDEO, padrao.valor());

        // Usuário.
        escrever_u32(&mut cru, OFF_FUSO_BIAS, 180); // UTC-3, em minutos
        escrever_u32(&mut cru, OFF_IDIOMA, idioma.valor());
        escrever_u32(&mut cru, OFF_FLAGS_VIDEO, FlagsDeVideo::MODO_480P);
        escrever_u32(&mut cru, OFF_FLAGS_AUDIO, 0); // estéreo
        escrever_u32(&mut cru, OFF_CONTROLE_JOGOS, 0);
        escrever_u32(&mut cru, OFF_CONTROLE_SENHA, 0);
        escrever_u32(&mut cru, OFF_CONTROLE_FILMES, 0);
        escrever_u32(&mut cru, OFF_IP, 0);
        escrever_u32(&mut cru, OFF_DNS, 0);
        escrever_u32(&mut cru, OFF_GATEWAY, 0);
        escrever_u32(&mut cru, OFF_MASCARA, 0);
        escrever_u32(&mut cru, OFF_FLAGS_DIVERSAS, 0);
        escrever_u32(
            &mut cru,
            OFF_REGIAO_DVD,
            // Região 1 (América) para NTSC-M; 2 para Japão e Europa.
            match padrao {
                PadraoDeVideo::NtscJ | PadraoDeVideo::PalI => 2,
                _ => 1,
            },
        );

        // `parse` dos bytes já com checksum, para que a estrutura e o cru concordem.
        let parcial = Self {
            cru,
            serial: String::from_utf8_lossy(serial).into_owned(),
            mac: [0x02, 0x00, 0x00, 0x0B, 0x51, 0xD1],
            padrao_de_video: padrao,
            idioma,
            flags_de_video: FlagsDeVideo(FlagsDeVideo::MODO_480P),
            flags_de_audio: 0,
            regiao_dvd: if matches!(padrao, PadraoDeVideo::NtscM) {
                1
            } else {
                2
            },
            fuso_bias: 180,
        };
        let bytes = parcial.bytes();
        Self {
            cru: bytes,
            ..parcial
        }
    }
}

fn escrever_u32(buf: &mut [u8], off: usize, v: u32) {
    buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

/// Checksum de uma seção da EEPROM.
///
/// ⚠️ **Fórmula a validar.** A documentação pública descreve uma soma de palavras de
/// 32 bits com propagação de carry e complemento no fim; esta é a nossa leitura dela.
/// Como o único consumidor é o nosso kernel, uma divergência aqui não quebra jogo
/// nenhum — mas também não detectaria corrupção em um dump real, e é por isso que
/// [`Eeprom::checksums_conferem`] é informativo e não fatal.
pub fn checksum(dados: &[u8]) -> u32 {
    let mut alto: u32 = 0;
    let mut baixo: u32 = 0;
    for palavra in dados.chunks(4) {
        let mut bytes = [0u8; 4];
        bytes[..palavra.len()].copy_from_slice(palavra);
        let v = u32::from_le_bytes(bytes);
        let soma = u64::from(baixo) + u64::from(v);
        baixo = soma as u32;
        alto = alto.wrapping_add((soma >> 32) as u32);
    }
    !alto.wrapping_add(baixo)
}

#[cfg(test)]
mod tests {
    use super::{Eeprom, FlagsDeVideo, Idioma, PadraoDeVideo, TAMANHO, checksum};

    #[test]
    fn sintetica_ida_e_volta() {
        let e = Eeprom::sintetica(PadraoDeVideo::NtscM, Idioma::Portugues);
        let bytes = e.bytes();
        let voltou = Eeprom::parse(&bytes).expect("deve parsear");
        assert_eq!(voltou.padrao_de_video, PadraoDeVideo::NtscM);
        assert_eq!(voltou.idioma, Idioma::Portugues);
        assert_eq!(voltou.serial, "OBSIDIAN0001");
        assert_eq!(
            voltou.mac[0] & 0x02,
            0x02,
            "MAC deve ter o bit local ligado"
        );
        assert!(voltou.flags_de_video.tem(FlagsDeVideo::MODO_480P));
    }

    #[test]
    fn sintetica_tem_checksums_coerentes() {
        for padrao in [
            PadraoDeVideo::NtscM,
            PadraoDeVideo::NtscJ,
            PadraoDeVideo::PalI,
        ] {
            let e = Eeprom::sintetica(padrao, Idioma::Ingles);
            assert!(
                e.checksums_conferem(),
                "{} ficou com checksum errado",
                padrao.nome()
            );
            let voltou = Eeprom::parse(&e.bytes()).unwrap();
            assert!(voltou.checksums_conferem());
        }
    }

    #[test]
    fn padrao_define_vblank() {
        assert_eq!(PadraoDeVideo::NtscM.vblank_millihz(), 59_940);
        assert_eq!(PadraoDeVideo::NtscJ.vblank_millihz(), 59_940);
        assert_eq!(PadraoDeVideo::PalI.vblank_millihz(), 50_000);
    }

    #[test]
    fn valor_desconhecido_e_preservado() {
        let p = PadraoDeVideo::de_valor(0xDEAD_BEEF);
        assert_eq!(p, PadraoDeVideo::Desconhecido(0xDEAD_BEEF));
        assert_eq!(p.valor(), 0xDEAD_BEEF, "valor cru não pode ser perdido");

        let i = Idioma::de_valor(99);
        assert_eq!(i.valor(), 99);
    }

    #[test]
    fn tamanho_errado_e_recusado() {
        assert!(Eeprom::parse(&[]).is_err());
        assert!(Eeprom::parse(&[0u8; 255]).is_err());
        assert!(Eeprom::parse(&[0u8; 257]).is_err());
        assert!(Eeprom::parse(&[0u8; TAMANHO]).is_ok());
    }

    #[test]
    fn eeprom_de_lixo_nao_causa_panico() {
        // Qualquer sequência de 256 bytes é "válida" estruturalmente: a EEPROM não tem
        // assinatura. O que não pode é entrar em pânico nem perder informação.
        let mut semente = 0x1234_5678u32;
        for _ in 0..500 {
            let mut lixo = [0u8; TAMANHO];
            for b in &mut lixo {
                semente = semente.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                *b = (semente >> 16) as u8;
            }
            let e = Eeprom::parse(&lixo).expect("256 bytes sempre parseiam");
            let _ = e.checksums_conferem();
            let _ = e.bloco_cifrado();
            // O que não mexemos tem que voltar igual.
            let bytes = e.bytes();
            assert_eq!(
                &bytes[0x14..0x30],
                &lixo[0x14..0x30],
                "bloco cifrado foi alterado"
            );
        }
    }

    #[test]
    fn checksum_e_estavel_e_sensivel() {
        let dados = [1u8, 2, 3, 4, 5, 6, 7, 8];
        let a = checksum(&dados);
        assert_eq!(a, checksum(&dados), "checksum precisa ser determinístico");
        let mut outro = dados;
        outro[3] = 9;
        assert_ne!(
            a,
            checksum(&outro),
            "mudança de byte precisa mudar o checksum"
        );
    }
}
