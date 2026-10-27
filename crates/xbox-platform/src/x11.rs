//! Apresentador X11 para o Linux desktop (ADR-0086).
//!
//! Fala o protocolo X11 core diretamente sobre o socket Unix do servidor, sem libX11 e
//! sem `unsafe`: o recorte necessário (uma janela, `PutImage`, alguns eventos) cabe em
//! poucas requisições, e o apresentador Win32 já segue a mesma política de não puxar
//! dependências de sistema. No GNOME/KDE em Wayland a janela aparece pelo Xwayland.
//!
//! O que fica fora: Wayland nativo, DRM/KMS (Fase 10), display remoto por TCP e
//! visuais diferentes de `TrueColor` 24 bits. Cada um falha com o motivo, nunca com
//! uma janela que não mostra nada.

pub(crate) mod protocolo;
pub mod verificacao;

use std::collections::VecDeque;
use std::io::{ErrorKind, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use xbox_core::{Error, Result};

use crate::display::{Apresentador, Evento, Quadro};
use protocolo as p;

/// Variáveis de ambiente lidas: o chamador pode passar outras em teste.
#[derive(Debug, Clone, Default)]
pub(crate) struct Ambiente {
    pub(crate) display: Option<String>,
    pub(crate) xauthority: Option<PathBuf>,
    pub(crate) home: Option<PathBuf>,
}

impl Ambiente {
    pub(crate) fn do_processo() -> Self {
        Self {
            display: std::env::var("DISPLAY").ok().filter(|d| !d.is_empty()),
            xauthority: std::env::var_os("XAUTHORITY").map(PathBuf::from),
            home: std::env::var_os("HOME").map(PathBuf::from),
        }
    }
}

/// Conexão com o servidor X: envia requisições, separa respostas, erros e eventos.
pub(crate) struct Conexao {
    socket: UnixStream,
    entrada: Vec<u8>,
    /// Número de sequência da última requisição enviada (o servidor conta em 16 bits).
    sequencia: u16,
    eventos: VecDeque<[u8; 32]>,
    pub(crate) setup: p::Setup,
    proximo_id: u32,
}

impl Conexao {
    /// Conecta ao display de `ambiente`, autenticando com o cookie do Xauthority.
    pub(crate) fn abrir(ambiente: &Ambiente) -> Result<Self> {
        let display = ambiente.display.as_deref().ok_or_else(|| {
            Error::format(
                "DISPLAY não definido: a janela no Linux precisa de um servidor X \
                 (Xorg ou Xwayland); o modo sem servidor gráfico é a Fase 10",
            )
        })?;
        let numero = p::numero_do_display(display)?;
        let socket = conectar(numero)?;
        let cookie = caminho_xauthority(ambiente)
            .and_then(|c| std::fs::read(c).ok())
            .and_then(|b| p::ler_xauthority(&b).ok())
            .and_then(|e| p::escolher_cookie(&e, numero));
        let (nome, dados) = match &cookie {
            Some(c) => (p::MIT_MAGIC_COOKIE, c.as_slice()),
            None => (&[][..], &[][..]),
        };
        let mut conexao = Self {
            socket,
            entrada: Vec::new(),
            sequencia: 0,
            eventos: VecDeque::new(),
            setup: p::Setup {
                id_base: 0,
                id_mascara: 0,
                requisicao_maxima: 0,
                imagem_lsb: true,
                keycode_min: 0,
                keycode_max: 0,
                raiz: 0,
                visual: 0,
                profundidade: 0,
            },
            proximo_id: 1,
        };
        conexao
            .socket
            .write_all(&p::pedido_de_setup(nome, dados))
            .map_err(|e| Error::format(format!("X11: envio do setup falhou: {e}")))?;
        let mut cabecalho = [0u8; 8];
        conexao
            .socket
            .read_exact(&mut cabecalho)
            .map_err(|e| Error::format(format!("X11: servidor fechou durante o setup: {e}")))?;
        let mut resposta = cabecalho.to_vec();
        resposta.resize(p::tamanho_do_setup(cabecalho), 0);
        conexao
            .socket
            .read_exact(&mut resposta[8..])
            .map_err(|e| Error::format(format!("X11: setup truncado: {e}")))?;
        let tela = display
            .rsplit_once('.')
            .filter(|(antes, _)| antes.contains(':'))
            .and_then(|(_, t)| t.parse().ok())
            .unwrap_or(0);
        conexao.setup = p::ler_setup(&resposta, tela)?;
        Ok(conexao)
    }

    pub(crate) fn novo_id(&mut self) -> u32 {
        let id = p::id_de_recurso(&self.setup, self.proximo_id);
        self.proximo_id += 1;
        id
    }

    /// Envia uma requisição e devolve seu número de sequência.
    pub(crate) fn enviar(&mut self, requisicao: &[u8]) -> Result<u16> {
        self.socket
            .write_all(requisicao)
            .map_err(|e| Error::format(format!("X11: conexão perdida ao enviar: {e}")))?;
        self.sequencia = self.sequencia.wrapping_add(1);
        Ok(self.sequencia)
    }

    /// Envia e espera a resposta. Eventos que chegarem antes ficam na fila.
    pub(crate) fn pedir(&mut self, requisicao: &[u8]) -> Result<Vec<u8>> {
        let sequencia = self.enviar(requisicao)?;
        loop {
            match self.proximo_pacote(true)? {
                Some(p::Pacote::Resposta {
                    sequencia: s,
                    dados,
                }) if s == sequencia => {
                    return Ok(dados);
                }
                Some(p::Pacote::Resposta { .. }) | None => {}
                Some(p::Pacote::Evento(e)) => self.eventos.push_back(e),
                Some(p::Pacote::Erro {
                    codigo,
                    valor,
                    opcode,
                    ..
                }) => {
                    return Err(Error::format(p::Pacote::descrever_erro(
                        codigo, valor, opcode,
                    )));
                }
            }
        }
    }

    /// Lê o que já chegou sem bloquear e devolve os eventos pendentes.
    pub(crate) fn drenar(&mut self) -> Result<VecDeque<[u8; 32]>> {
        while let Some(pacote) = self.proximo_pacote(false)? {
            match pacote {
                p::Pacote::Evento(e) => self.eventos.push_back(e),
                p::Pacote::Resposta { .. } => {}
                p::Pacote::Erro {
                    codigo,
                    valor,
                    opcode,
                    ..
                } => {
                    return Err(Error::format(p::Pacote::descrever_erro(
                        codigo, valor, opcode,
                    )));
                }
            }
        }
        Ok(std::mem::take(&mut self.eventos))
    }

    fn proximo_pacote(&mut self, bloquear: bool) -> Result<Option<p::Pacote>> {
        loop {
            if let Some(pacote) = p::extrair_pacote(&mut self.entrada) {
                return Ok(Some(pacote));
            }
            let mut bloco = [0u8; 4096];
            self.socket
                .set_nonblocking(!bloquear)
                .map_err(|e| Error::format(format!("X11: socket: {e}")))?;
            let lido = self.socket.read(&mut bloco);
            // Requisições seguintes (PutImage grande) precisam de escrita bloqueante.
            self.socket
                .set_nonblocking(false)
                .map_err(|e| Error::format(format!("X11: socket: {e}")))?;
            match lido {
                Ok(0) => return Err(Error::format("X11: o servidor encerrou a conexão")),
                Ok(n) => self.entrada.extend_from_slice(&bloco[..n]),
                Err(e) if e.kind() == ErrorKind::WouldBlock => return Ok(None),
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => return Err(Error::format(format!("X11: leitura falhou: {e}"))),
            }
        }
    }

    pub(crate) fn atomo(&mut self, nome: &str, so_se_existir: bool) -> Result<u32> {
        let resposta = self.pedir(&p::intern_atom(nome, so_se_existir))?;
        Ok(p::atomo_da_resposta(&resposta))
    }
}

impl std::fmt::Debug for Conexao {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Conexao")
            .field("setup", &self.setup)
            .field("sequencia", &self.sequencia)
            .finish_non_exhaustive()
    }
}

fn caminho_xauthority(ambiente: &Ambiente) -> Option<PathBuf> {
    ambiente
        .xauthority
        .clone()
        .or_else(|| ambiente.home.as_ref().map(|h| h.join(".Xauthority")))
}

/// Socket do display local: o caminho em `/tmp/.X11-unix` e, se ele não existir
/// (contêineres sem esse diretório), o nome abstrato equivalente que Xorg e Xwayland
/// também escutam.
fn conectar(numero: u32) -> Result<UnixStream> {
    let caminho = format!("/tmp/.X11-unix/X{numero}");
    match UnixStream::connect(&caminho) {
        Ok(s) => Ok(s),
        Err(primeiro) => {
            use std::os::linux::net::SocketAddrExt;
            std::os::unix::net::SocketAddr::from_abstract_name(caminho.as_bytes())
                .and_then(|a| UnixStream::connect_addr(&a))
                .map_err(|_| {
                    Error::format(format!(
                        "não foi possível conectar ao servidor X em {caminho}: {primeiro}"
                    ))
                })
        }
    }
}

/// Janela X11 que apresenta quadros.
pub(crate) struct Janela {
    conexao: Conexao,
    id_x: u32,
    gc: u32,
    largura: u16,
    altura: u16,
    linhas_por_faixa: u32,
    /// Último quadro no formato do servidor, reaproveitado entre quadros e reenviado
    /// quando o servidor pede repinte (Expose).
    pixels: Vec<u8>,
    interpretador: p::Interpretador,
    utf8_string: u32,
    net_wm_name: u32,
    titulo: String,
}

impl Janela {
    /// Cria uma janela de `largura × altura` pixels e a mostra.
    pub(crate) fn nova(titulo: &str, largura: u32, altura: u32) -> Result<Self> {
        Self::nova_em(&Ambiente::do_processo(), titulo, largura, altura)
    }

    pub(crate) fn nova_em(
        ambiente: &Ambiente,
        titulo: &str,
        largura: u32,
        altura: u32,
    ) -> Result<Self> {
        if largura == 0 || altura == 0 {
            return Err(Error::format("janela de dimensão nula"));
        }
        let largura_x = u16::try_from(largura)
            .map_err(|_| Error::format("largura da janela não cabe no protocolo X"))?;
        let altura_x = u16::try_from(altura)
            .map_err(|_| Error::format("altura da janela não cabe no protocolo X"))?;
        let mut c = Conexao::abrir(ambiente)?;
        let linhas_por_faixa = p::linhas_por_faixa(c.setup.requisicao_maxima, largura)?;

        let wm_protocols = c.atomo("WM_PROTOCOLS", false)?;
        let wm_delete = c.atomo("WM_DELETE_WINDOW", false)?;
        let utf8_string = c.atomo("UTF8_STRING", false)?;
        let net_wm_name = c.atomo("_NET_WM_NAME", false)?;
        let quantidade = c.setup.keycode_max - c.setup.keycode_min + 1;
        let mapa = c.pedir(&p::get_keyboard_mapping(c.setup.keycode_min, quantidade))?;
        let keysyms = p::keysyms_da_resposta(&mapa, quantidade as usize)?;

        let janela = c.novo_id();
        let gc = c.novo_id();
        let eventos = p::MASCARA_KEY_PRESS
            | p::MASCARA_KEY_RELEASE
            | p::MASCARA_EXPOSURE
            | p::MASCARA_STRUCTURE_NOTIFY;
        let (raiz, profundidade, visual) = (c.setup.raiz, c.setup.profundidade, c.setup.visual);
        c.enviar(&p::create_window(
            janela,
            raiz,
            largura_x,
            altura_x,
            profundidade,
            visual,
            eventos,
        ))?;
        c.enviar(&p::change_property(
            janela,
            wm_protocols,
            p::ATOM_ATOM,
            32,
            &wm_delete.to_le_bytes(),
        ))?;
        c.enviar(&p::change_property(
            janela,
            p::ATOM_WM_NORMAL_HINTS,
            p::ATOM_WM_SIZE_HINTS,
            32,
            &p::hints_de_tamanho_fixo(largura_x, altura_x),
        ))?;
        c.enviar(&p::change_property(
            janela,
            p::ATOM_WM_CLASS,
            p::ATOM_STRING,
            8,
            p::WM_CLASS,
        ))?;
        c.enviar(&p::create_gc(gc, janela))?;
        let mut j = Self {
            interpretador: p::Interpretador::novo(
                janela,
                wm_protocols,
                wm_delete,
                c.setup.keycode_min,
                keysyms,
            ),
            conexao: c,
            id_x: janela,
            gc,
            largura: largura_x,
            altura: altura_x,
            linhas_por_faixa,
            pixels: Vec::new(),
            utf8_string,
            net_wm_name,
            titulo: String::new(),
        };
        j.definir_titulo(titulo)?;
        j.conexao.enviar(&p::um_recurso(p::MAP_WINDOW, janela))?;
        // Ida e volta: qualquer erro das requisições acima (BadMatch, BadAlloc) aparece
        // aqui, na criação, e não no primeiro quadro.
        j.conexao.pedir(&p::get_input_focus())?;
        Ok(j)
    }

    /// Identificador X da janela, para os testes conferirem a verificação externa.
    #[cfg(test)]
    pub(crate) const fn id(&self) -> u32 {
        self.id_x
    }

    fn enviar_pixels(&mut self) -> Result<()> {
        if self.pixels.is_empty() {
            return Ok(());
        }
        let bytes_por_linha = self.largura as usize * 4;
        let mut y = 0u32;
        while y < u32::from(self.altura) {
            let linhas = self.linhas_por_faixa.min(u32::from(self.altura) - y);
            let inicio = y as usize * bytes_por_linha;
            let fim = inicio + linhas as usize * bytes_por_linha;
            let requisicao = p::put_image(
                self.id_x,
                self.gc,
                self.largura,
                y as u16,
                linhas as u16,
                self.conexao.setup.profundidade,
                &self.pixels[inicio..fim],
            );
            self.conexao.enviar(&requisicao)?;
            y += linhas;
        }
        Ok(())
    }
}

impl Apresentador for Janela {
    fn definir_titulo(&mut self, titulo: &str) -> Result<()> {
        // O modo contínuo redefine o título a cada volta do laço: só envia mudanças.
        if titulo == self.titulo {
            return Ok(());
        }
        titulo.clone_into(&mut self.titulo);
        self.conexao.enviar(&p::change_property(
            self.id_x,
            self.net_wm_name,
            self.utf8_string,
            8,
            titulo.as_bytes(),
        ))?;
        // WM_NAME é Latin-1: o que não couber vira '?', em vez de bytes UTF-8 mal lidos.
        let latin1: Vec<u8> = titulo
            .chars()
            .map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?'))
            .collect();
        self.conexao.enviar(&p::change_property(
            self.id_x,
            p::ATOM_WM_NAME,
            p::ATOM_STRING,
            8,
            &latin1,
        ))?;
        Ok(())
    }

    fn apresentar(&mut self, quadro: &Quadro) -> Result<()> {
        if quadro.largura() != u32::from(self.largura) || quadro.altura() != u32::from(self.altura)
        {
            return Err(Error::format(format!(
                "quadro {}x{} numa janela X de {}x{}",
                quadro.largura(),
                quadro.altura(),
                self.largura,
                self.altura
            )));
        }
        p::converter_pixels(
            quadro.bytes(),
            &mut self.pixels,
            self.conexao.setup.imagem_lsb,
        );
        self.enviar_pixels()
    }

    fn eventos(&mut self) -> Result<Evento> {
        for e in self.conexao.drenar()? {
            self.interpretador.consumir(&e);
        }
        if self.interpretador.tomar_repintar() {
            self.enviar_pixels()?;
        }
        Ok(self.interpretador.proximo())
    }
}

impl Drop for Janela {
    fn drop(&mut self) {
        // Erros aqui são irrelevantes: fechar o socket libera tudo no servidor.
        let _ = self.conexao.enviar(&p::um_recurso(p::FREE_GC, self.gc));
        let _ = self
            .conexao
            .enviar(&p::um_recurso(p::DESTROY_WINDOW, self.id_x));
    }
}

impl std::fmt::Debug for Janela {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Janela")
            .field("id_x", &self.id_x)
            .field("largura", &self.largura)
            .field("altura", &self.altura)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::{Ambiente, Janela};

    #[test]
    fn sem_display_o_erro_cita_a_variavel_e_a_fase_sem_servidor() {
        let erro = Janela::nova_em(&Ambiente::default(), "x", 640, 480)
            .unwrap_err()
            .to_string();
        assert!(
            erro.contains("DISPLAY") && erro.contains("Fase 10"),
            "{erro}"
        );
    }

    #[test]
    fn display_remoto_e_inexistente_falham_com_o_motivo() {
        let remoto = Ambiente {
            display: Some("outro-host:10.0".into()),
            ..Ambiente::default()
        };
        let erro = Janela::nova_em(&remoto, "x", 640, 480)
            .unwrap_err()
            .to_string();
        assert!(erro.contains("remoto"), "{erro}");
        let ausente = Ambiente {
            display: Some(":4093".into()),
            ..Ambiente::default()
        };
        let erro = Janela::nova_em(&ausente, "x", 640, 480)
            .unwrap_err()
            .to_string();
        assert!(erro.contains("/tmp/.X11-unix/X4093"), "{erro}");
    }

    #[test]
    fn dimensao_nula_e_recusada_antes_de_conectar() {
        assert!(
            Janela::nova_em(&Ambiente::default(), "x", 0, 480)
                .unwrap_err()
                .to_string()
                .contains("nula")
        );
    }
}
