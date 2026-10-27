//! Verificação externa da janela X11 do Obsidian, por outra conexão com o servidor.
//!
//! Equivalente Linux do `scripts/verify-window.ps1`: encontra a janela pelo
//! `WM_CLASS`, lê o conteúdo que o servidor tem para ela (`GetImage`) e envia o que um
//! usuário enviaria (`WM_DELETE_WINDOW`, teclas). Não compartilha estado com o
//! apresentador, então prova o que chegou ao servidor, não o que o processo acha que
//! enviou.

use xbox_core::{Error, Result};

use super::{Ambiente, Conexao, protocolo as p};

fn u16_le(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}

fn u32_le(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

/// Conexão de verificação.
#[derive(Debug)]
pub struct VerificadorX11 {
    conexao: Conexao,
    wm_protocols: u32,
    wm_delete: u32,
    keysyms: Vec<u32>,
    tempo: u32,
}

impl VerificadorX11 {
    /// Conecta ao display de `DISPLAY`, como o apresentador.
    pub fn conectar() -> Result<Self> {
        let mut conexao = Conexao::abrir(&Ambiente::do_processo())?;
        let wm_protocols = conexao.atomo("WM_PROTOCOLS", false)?;
        let wm_delete = conexao.atomo("WM_DELETE_WINDOW", false)?;
        let (min, max) = (conexao.setup.keycode_min, conexao.setup.keycode_max);
        let mapa = conexao.pedir(&p::get_keyboard_mapping(min, max - min + 1))?;
        let keysyms = p::keysyms_da_resposta(&mapa, usize::from(max - min) + 1)?;
        Ok(Self {
            conexao,
            wm_protocols,
            wm_delete,
            keysyms,
            tempo: 1,
        })
    }

    /// Janelas cujo `WM_CLASS` é o do Obsidian, em ordem de pilha.
    pub fn localizar(&mut self) -> Result<Vec<u32>> {
        let mut encontradas = Vec::new();
        let mut pendentes = vec![self.conexao.setup.raiz];
        while let Some(janela) = pendentes.pop() {
            // A janela pode sumir entre QueryTree e GetProperty (outro programa fechou).
            let Ok(arvore) = self.conexao.pedir(&p::query_tree(janela)) else {
                continue;
            };
            let filhos = u16_le(&arvore, 16) as usize;
            for k in 0..filhos {
                let filho = u32_le(&arvore, 32 + 4 * k);
                if self.classe(filho).as_deref() == Some(p::WM_CLASS) {
                    encontradas.push(filho);
                } else {
                    pendentes.push(filho);
                }
            }
        }
        Ok(encontradas)
    }

    fn classe(&mut self, janela: u32) -> Option<Vec<u8>> {
        let pedido = p::get_property(janela, p::ATOM_WM_CLASS, p::ATOM_STRING, 64);
        let resposta = self.conexao.pedir(&pedido).ok()?;
        let tamanho = u32_le(&resposta, 16) as usize;
        (resposta[1] == 8 && tamanho > 0)
            .then(|| resposta.get(32..32 + tamanho).unwrap_or_default().to_vec())
    }

    /// Conteúdo da janela em `B,G,R,A`, com alfa zerado (o servidor não guarda alfa num
    /// visual de 24 bits). Enquanto a janela não estiver visível o servidor responde
    /// `BadMatch`, e o chamador deve tentar de novo.
    pub fn capturar(&mut self, janela: u32, largura: u16, altura: u16) -> Result<Vec<u8>> {
        let mut bgra = Vec::with_capacity(usize::from(largura) * usize::from(altura) * 4);
        let linhas = p::linhas_por_faixa(u16::MAX, u32::from(largura))?;
        let mut y = 0u16;
        while y < altura {
            let faixa = (linhas as u16).min(altura - y);
            let resposta = self
                .conexao
                .pedir(&p::get_image(janela, y, largura, faixa))?;
            let esperado = usize::from(largura) * usize::from(faixa) * 4;
            let dados = resposta
                .get(32..32 + esperado)
                .ok_or_else(|| Error::format("GetImage devolveu menos bytes que a área"))?;
            for px in dados.as_chunks::<4>().0 {
                let [b, g, r] = if self.conexao.setup.imagem_lsb {
                    [px[0], px[1], px[2]]
                } else {
                    [px[3], px[2], px[1]]
                };
                bgra.extend_from_slice(&[b, g, r, 0]);
            }
            y += faixa;
        }
        Ok(bgra)
    }

    /// Pede para fechar como o gerenciador de janelas faz ao clicar no X.
    pub fn fechar(&mut self, janela: u32) -> Result<()> {
        let e = p::evento_fechar(janela, self.wm_protocols, self.wm_delete);
        self.conexao.enviar(&p::send_event(janela, 0, &e))?;
        self.conexao.pedir(&p::get_input_focus()).map(|_| ())
    }

    /// Pressiona e solta a tecla de `keysym` (por exemplo `0xFFC2` = F5).
    pub fn tecla(&mut self, janela: u32, keysym: u32) -> Result<()> {
        let indice = self
            .keysyms
            .iter()
            .position(|&k| k == keysym)
            .ok_or_else(|| Error::format(format!("keysym 0x{keysym:X} sem keycode no servidor")))?;
        let keycode = self.conexao.setup.keycode_min + indice as u8;
        let raiz = self.conexao.setup.raiz;
        for tipo in [p::KEY_PRESS, p::KEY_RELEASE] {
            let e = p::evento_tecla(tipo, keycode, self.tempo, raiz, janela);
            self.tempo += 1;
            self.conexao.enviar(&p::send_event(janela, 0, &e))?;
        }
        self.conexao.pedir(&p::get_input_focus()).map(|_| ())
    }
}

/// Keysyms dos atalhos, para a ferramenta não repetir números mágicos.
pub mod teclas {
    /// Espaço: pausar/retomar.
    pub const ESPACO: u32 = super::p::XK_SPACE;
    /// F5: salvar estado.
    pub const F5: u32 = super::p::XK_F5;
    /// F9: restaurar estado.
    pub const F9: u32 = super::p::XK_F9;
}

#[cfg(test)]
mod tests {
    //! Teste com servidor real, opt-in como o de mídia real (`OBSIDIAN_MIDIA_REAL`):
    //! o CI não tem display, e pular em silêncio esconderia regressões locais.

    use super::{VerificadorX11, teclas};
    use crate::display::{Apresentador, Evento, Quadro};
    use crate::x11::Janela;

    #[test]
    fn janela_real_mostra_o_quadro_e_recebe_fechar_e_atalhos() {
        if std::env::var_os("OBSIDIAN_X11_REAL").is_none() {
            eprintln!("defina OBSIDIAN_X11_REAL=1 numa sessão gráfica para este teste");
            return;
        }
        let (l, a) = (640u32, 480u32);
        let bytes: Vec<u8> = (0..l * a)
            .flat_map(|i| {
                let (x, y) = (i % l, i / l);
                [
                    (x % 256) as u8,
                    (y % 256) as u8,
                    ((x ^ y) % 256) as u8,
                    0xFF,
                ]
            })
            .collect();
        let quadro = Quadro::novo(l, a, bytes).unwrap();
        let mut janela = Janela::nova("Obsidian — teste X11", l, a).unwrap();
        let mut v = VerificadorX11::conectar().unwrap();
        let id = janela.id();
        let mut capturado = Vec::new();
        for _ in 0..200 {
            janela.apresentar(&quadro).unwrap();
            assert_eq!(janela.eventos().unwrap(), Evento::Nenhum);
            // BadMatch até o gerenciador de janelas mapear a janela (não visível).
            if v.localizar().unwrap().contains(&id)
                && let Ok(c) = v.capturar(id, l as u16, a as u16)
            {
                capturado = c;
                if capturado
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(quadro.bytes().as_chunks::<4>().0)
                    .all(|(c, q)| c[..3] == q[..3])
                {
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        let diferentes = capturado
            .as_chunks::<4>()
            .0
            .iter()
            .zip(quadro.bytes().as_chunks::<4>().0)
            .filter(|(c, q)| c[..3] != q[..3])
            .count();
        assert_eq!(capturado.len(), quadro.bytes().len());
        assert_eq!(
            diferentes, 0,
            "pixels RGB diferentes entre a janela e o quadro"
        );

        for k in [teclas::ESPACO, teclas::F5, teclas::F9] {
            v.tecla(id, k).unwrap();
        }
        let mut vistos = Vec::new();
        for _ in 0..200 {
            match janela.eventos().unwrap() {
                Evento::Nenhum if vistos.len() == 3 => break,
                Evento::Nenhum => std::thread::sleep(std::time::Duration::from_millis(10)),
                e => vistos.push(e),
            }
        }
        assert_eq!(
            vistos,
            [
                Evento::PausarRetomar,
                Evento::SalvarEstado,
                Evento::RestaurarEstado
            ]
        );
        v.fechar(id).unwrap();
        let mut fechou = false;
        for _ in 0..200 {
            if janela.eventos().unwrap() == Evento::Fechar {
                fechou = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(fechou, "WM_DELETE_WINDOW não chegou como Evento::Fechar");
    }
}
