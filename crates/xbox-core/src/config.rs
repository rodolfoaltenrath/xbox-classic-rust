//! Configuração do emulador.
//!
//! Formato TOML, lido de `obsidian.toml`. Todo campo tem default, então um arquivo
//! vazio (ou ausente) é válido e produz a configuração padrão — o emulador precisa
//! funcionar na primeira execução sem o usuário editar nada.
//!
//! Aqui, ao contrário de [`crate::snapshot`], `serde` é a ferramenta certa: o formato
//! é de leitura humana, evolui devagar e não precisa de determinismo byte a byte.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Nome do arquivo de configuração procurado ao lado do executável.
pub const CONFIG_FILE_NAME: &str = "obsidian.toml";

/// Configuração completa do emulador.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Sistema emulado (RAM, região, idioma).
    pub system: SystemConfig,
    /// Vídeo e apresentação.
    pub video: VideoConfig,
    /// Áudio.
    pub audio: AudioConfig,
    /// Caminhos de biblioteca e de dados.
    pub paths: PathsConfig,
    /// Opções de desenvolvimento e depuração.
    pub debug: DebugConfig,
}

impl Config {
    /// Lê a configuração de um arquivo TOML.
    ///
    /// Um arquivo **ausente** devolve a configuração padrão: ausência não é erro.
    /// Um arquivo **presente e inválido** é erro — falhar calado sobre uma config que
    /// o usuário escreveu é pior que não iniciar.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        match std::fs::read_to_string(path) {
            Ok(texto) => {
                let cfg: Self = toml::from_str(&texto)
                    .map_err(|e| Error::config(format!("{}: {e}", path.display())))?;
                cfg.validate()?;
                Ok(cfg)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(Error::io_path(path, e)),
        }
    }

    /// Escreve a configuração em TOML.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        let texto = toml::to_string_pretty(self)
            .map_err(|e| Error::config(format!("falha ao serializar config: {e}")))?;
        std::fs::write(path, texto).map_err(|e| Error::io_path(path, e))
    }

    /// Verifica combinações inválidas que o sistema de tipos não impede.
    pub fn validate(&self) -> Result<()> {
        if !matches!(self.system.ram_mib, 64 | 128) {
            return Err(Error::config(format!(
                "system.ram_mib = {} — o Xbox tem 64 MiB (retail) ou 128 MiB (dev kit)",
                self.system.ram_mib
            )));
        }
        if !(1..=4).contains(&self.video.internal_scale) {
            return Err(Error::config(format!(
                "video.internal_scale = {} — use 1 a 4",
                self.video.internal_scale
            )));
        }
        if !(1..=200).contains(&self.audio.latency_ms) {
            return Err(Error::config(format!(
                "audio.latency_ms = {} — use 1 a 200",
                self.audio.latency_ms
            )));
        }
        Ok(())
    }
}

/// Parâmetros do console emulado.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SystemConfig {
    /// RAM unificada em MiB: 64 (retail) ou 128 (dev kit).
    pub ram_mib: u32,
    /// Região gravada na EEPROM.
    pub region: Region,
    /// Idioma gravado na EEPROM.
    pub language: Language,
    /// Backend de execução da CPU do guest.
    pub cpu_backend: CpuBackend,
}

impl Default for SystemConfig {
    fn default() -> Self {
        Self {
            ram_mib: 64,
            region: Region::Ntsc,
            language: Language::Portuguese,
            cpu_backend: CpuBackend::Auto,
        }
    }
}

/// Região do console (afeta vblank e checagem de mídia).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Region {
    /// América do Norte / 60 Hz.
    #[default]
    Ntsc,
    /// Japão / 60 Hz.
    NtscJ,
    /// Europa / 50 Hz.
    Pal,
}

impl Region {
    /// Taxa de vblank da região, em milihertz.
    pub const fn vblank_millihz(self) -> u64 {
        match self {
            Self::Ntsc | Self::NtscJ => crate::time::VBLANK_NTSC_MHZ,
            Self::Pal => crate::time::VBLANK_PAL_MHZ,
        }
    }
}

/// Idioma do dashboard/EEPROM.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[allow(missing_docs)]
pub enum Language {
    #[default]
    English,
    Japanese,
    German,
    French,
    Spanish,
    Italian,
    Korean,
    Chinese,
    Portuguese,
}

/// Qual backend de CPU usar (ROADMAP §4.3).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CpuBackend {
    /// Escolhe o melhor disponível no host: hypervisor, senão JIT, senão interpretador.
    #[default]
    Auto,
    /// Interpretador — lento, para depuração e como oráculo de teste.
    Interpreter,
    /// Recompilador dinâmico — o caminho de produção.
    Jit,
    /// Virtualização (KVM/WHPX) — experimental.
    Hypervisor,
}

/// Vídeo e apresentação.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct VideoConfig {
    /// Multiplicador de resolução interna (1 = 640x480 nativo).
    pub internal_scale: u32,
    /// Modo de sincronização.
    pub sync: SyncMode,
    /// Manter a proporção 4:3 do console em telas widescreen.
    pub keep_aspect: bool,
    /// Filtro bilinear ao escalar a imagem final.
    pub linear_filter: bool,
    /// Tela cheia ao iniciar.
    pub fullscreen: bool,
}

impl Default for VideoConfig {
    fn default() -> Self {
        Self {
            internal_scale: 1,
            sync: SyncMode::default(),
            keep_aspect: true,
            linear_filter: true,
            fullscreen: false,
        }
    }
}

/// Quem dita o ritmo da emulação (ROADMAP §7.9).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SyncMode {
    /// Vsync do host — padrão no desktop.
    #[default]
    HostVsync,
    /// O consumo de áudio dita o ritmo — padrão no appliance, elimina crackle.
    AudioMaster,
    /// Sem limite, para benchmark.
    Unlimited,
}

/// Áudio.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AudioConfig {
    /// Latência alvo do buffer de saída, em milissegundos.
    pub latency_ms: u32,
    /// Habilita a emulação do APU (desligar só serve para isolar bug).
    pub enabled: bool,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            latency_ms: 24,
            enabled: true,
        }
    }
}

/// Caminhos de dados.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PathsConfig {
    /// Pastas varridas pela biblioteca em busca de jogos.
    pub game_dirs: Vec<PathBuf>,
    /// Onde ficam saves, EEPROM e HD virtual. Vazio = ao lado do executável.
    pub data_dir: Option<PathBuf>,
}

/// Opções de desenvolvimento.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DebugConfig {
    /// Loga toda chamada de ordinal do kernel (ruidoso, mas é ouro na Fase 4).
    pub log_kernel_calls: bool,
    /// Loga acessos a MMIO não implementado.
    pub log_unknown_mmio: bool,
    /// Abre o GDB stub nesta porta, se `Some` (requer feature `dbg-gdb`).
    pub gdb_port: Option<u16>,
}

#[cfg(test)]
mod tests {
    use super::{Config, CpuBackend, Region, SyncMode};

    #[test]
    fn config_vazia_e_valida_e_igual_ao_default() {
        let cfg: Config = toml::from_str("").expect("TOML vazio é válido");
        assert_eq!(cfg, Config::default());
        cfg.validate().expect("default deve ser válida");
    }

    #[test]
    fn ida_e_volta_por_toml() {
        let mut original = Config::default();
        original.system.cpu_backend = CpuBackend::Interpreter;
        original.system.region = Region::Pal;
        original.video.internal_scale = 3;
        original.video.sync = SyncMode::AudioMaster;
        original.paths.game_dirs.push("D:/jogos".into());

        let texto = toml::to_string_pretty(&original).expect("serializar");
        let volta: Config = toml::from_str(&texto).expect("desserializar");
        assert_eq!(original, volta);
    }

    #[test]
    fn campo_desconhecido_e_erro() {
        // Protege contra typo silencioso em opção de configuração.
        let r: Result<Config, _> = toml::from_str("[video]\nfulscreen = true\n");
        assert!(r.is_err());
    }

    #[test]
    fn valores_fora_de_faixa_sao_recusados() {
        let mut cfg = Config::default();
        cfg.system.ram_mib = 96;
        assert!(cfg.validate().is_err());

        let mut cfg = Config::default();
        cfg.video.internal_scale = 9;
        assert!(cfg.validate().is_err());

        let mut cfg = Config::default();
        cfg.audio.latency_ms = 0;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn arquivo_ausente_devolve_default() {
        let cfg = Config::load("nao/existe/obsidian.toml").expect("ausência não é erro");
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn regiao_define_a_taxa_de_vblank() {
        assert_eq!(Region::Ntsc.vblank_millihz(), 59_940);
        assert_eq!(Region::Pal.vblank_millihz(), 50_000);
    }
}
