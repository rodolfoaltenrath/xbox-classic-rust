//! Apresentação de quadros: levar o framebuffer do guest até uma superfície do host.
//!
//! Este módulo define a **fronteira** e nada de sistema operacional. A implementação por
//! SO vive em submódulos próprios (`windows`), e o resto do projeto só enxerga
//! [`Apresentador`] — o que cumpre a regra do ROADMAP §2 de que nenhum
//! `#[cfg(target_os)]` aparece fora de `xbox-platform`.
//!
//! ## O que a imagem apresentada **é**, e o que **não é**
//!
//! O chamador copia os bytes `B,G,R,A` do framebuffer e aplica a paleta RGB8 do
//! DAC quando o perfil da sessão a modela (ADRs 0079–0080). Snapshots legados
//! conservam a apresentação bruta. A imagem não é a saída analógica do console:
//!
//! | Efeito | Estado |
//! |---|---|
//! | rampa de gamma do DAC | RGB8 aplicado à cópia; limites do perfil público em P21 |
//! | filtro de flicker / soften do encoder | só pedidos lógicos registrados (ADR-0050) |
//! | entrelaçamento, overscan e proporção de pixel do sinal de TV | não modelados |
//!
//! Isto é **provisório e declarado**. Um título que dependa desses efeitos para a imagem
//! correta — filtros do encoder, por exemplo — vai aparecer diferente aqui. A
//! pendência está no ROADMAP com a condição de saída; ela não é resolvida escondendo-a
//! atrás de uma imagem que parece certa.
//!
//! ## Posse e threads
//!
//! [`Quadro`] **possui** seus bytes. A arena do guest é `Send` e não `Sync` (ADR-0011),
//! então nenhuma referência à RAM guest atravessa para código de janela: o chamador copia
//! as linhas visíveis para um `Quadro` e o entrega ao apresentador. O custo é uma cópia de
//! 1,2 MB por quadro, e a vantagem é que o apresentador não precisa saber que existe uma
//! arena, nem o que é pitch, nem endereço físico.

use std::fmt;

use xbox_core::Result;

/// Bytes por pixel do formato `A8R8G8B8` do guest, em ordem de memória `B,G,R,A`.
pub const BYTES_POR_PIXEL: usize = 4;

/// Um quadro pronto para apresentar: linhas visíveis, sem padding de pitch.
#[derive(Clone, PartialEq, Eq)]
pub struct Quadro {
    largura: u32,
    altura: u32,
    bytes: Vec<u8>,
}

impl Quadro {
    /// Monta um quadro a partir de `altura` linhas de `largura × 4` bytes cada.
    ///
    /// Erra se o tamanho não bater: um quadro com bytes a menos faria o apresentador ler
    /// além do buffer, e o apresentador Win32 passa esse buffer direto ao sistema.
    pub fn novo(largura: u32, altura: u32, bytes: Vec<u8>) -> Result<Self> {
        let esperado = (largura as usize)
            .checked_mul(altura as usize)
            .and_then(|p| p.checked_mul(BYTES_POR_PIXEL))
            .ok_or_else(|| xbox_core::Error::format("dimensões do quadro transbordam"))?;
        if largura == 0 || altura == 0 || bytes.len() != esperado {
            return Err(xbox_core::Error::format(format!(
                "quadro {largura}x{altura} exige {esperado} bytes, recebeu {}",
                bytes.len()
            )));
        }
        Ok(Self {
            largura,
            altura,
            bytes,
        })
    }

    /// Largura em pixels.
    pub const fn largura(&self) -> u32 {
        self.largura
    }

    /// Altura em pixels.
    pub const fn altura(&self) -> u32 {
        self.altura
    }

    /// Todos os bytes, `B,G,R,A`, linha a linha, sem padding.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Aplica a tabela RGB à cópia BGRA; alfa e memória de origem são preservados.
    /// A plataforma não conhece o dispositivo nem toma emprestada a arena guest.
    pub fn aplicar_rampa_rgb(&mut self, palette: &[[u8; 3]; 256]) {
        for pixel in self.bytes.as_chunks_mut::<BYTES_POR_PIXEL>().0 {
            pixel[0] = palette[usize::from(pixel[0])][2];
            pixel[1] = palette[usize::from(pixel[1])][1];
            pixel[2] = palette[usize::from(pixel[2])][0];
        }
    }

    /// Os quatro bytes `B,G,R,A` de um pixel, ou `None` fora do quadro.
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.largura || y >= self.altura {
            return None;
        }
        let i = (y as usize * self.largura as usize + x as usize) * BYTES_POR_PIXEL;
        self.bytes.get(i..i + BYTES_POR_PIXEL)?.try_into().ok()
    }

    /// Quantos pixels têm algum canal de cor diferente de zero.
    ///
    /// Útil como evidência objetiva de que algo foi desenhado, sem comparar imagens.
    pub fn pixels_coloridos(&self) -> usize {
        let (pixels, _) = self.bytes.as_chunks::<BYTES_POR_PIXEL>();
        pixels
            .iter()
            .filter(|p| p[0] != 0 || p[1] != 0 || p[2] != 0)
            .count()
    }
}

// `Debug` manual: um quadro de 1,2 MB impresso byte a byte tornaria qualquer falha de
// teste ilegível.
impl fmt::Debug for Quadro {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Quadro")
            .field("largura", &self.largura)
            .field("altura", &self.altura)
            .field("pixels_coloridos", &self.pixels_coloridos())
            .finish_non_exhaustive()
    }
}

/// O que o usuário fez na superfície desde a última consulta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Evento {
    /// Nada que exija reação.
    Nenhum,
    /// O usuário pediu para fechar. O emulador deve encerrar a execução de forma
    /// controlada — é assim que o host interrompe um título que não retorna sozinho.
    Fechar,
    /// Espaço: alterna pausa da sessão no host; não é input do Xbox.
    PausarRetomar,
    /// F5: grava uma nova geração de estado completo.
    SalvarEstado,
    /// F9: restaura a última geração salva nesta execução.
    RestaurarEstado,
}

/// Uma superfície onde quadros aparecem.
pub trait Apresentador: std::fmt::Debug {
    /// Atualiza o título da superfície (estado/atalhos do programa).
    fn definir_titulo(&mut self, titulo: &str) -> Result<()>;
    /// Mostra um quadro, substituindo o anterior.
    fn apresentar(&mut self, quadro: &Quadro) -> Result<()>;

    /// Processa a fila de eventos da superfície e informa o que importa.
    ///
    /// Precisa ser chamado com frequência: numa janela, é o que a mantém responsiva e o
    /// que detecta o pedido de fechar.
    fn eventos(&mut self) -> Result<Evento>;
}

/// Apresentador sem superfície: guarda os quadros em memória.
///
/// Existe para testar tudo o que fica acima da janela — recorte do framebuffer, formato
/// dos bytes, ritmo da apresentação — **sem** exigir display, o que permite rodar no CI e
/// em máquina sem sessão gráfica. Também serve de apresentador quando o sistema não tem
/// janela implementada.
#[derive(Debug, Default)]
pub struct Gravador {
    titulo: String,
    quadros: Vec<Quadro>,
    fechar_depois_de: Option<usize>,
}

impl Gravador {
    /// Gravador vazio, que nunca pede para fechar.
    pub fn novo() -> Self {
        Self::default()
    }

    /// Pede para fechar depois de `n` quadros apresentados. Simula o usuário fechando a
    /// janela, para testar a interrupção controlada da execução.
    pub fn fechar_depois_de(n: usize) -> Self {
        Self {
            titulo: String::new(),
            quadros: Vec::new(),
            fechar_depois_de: Some(n),
        }
    }

    /// Quadros apresentados até agora, do mais antigo ao mais recente.
    pub fn quadros(&self) -> &[Quadro] {
        &self.quadros
    }
}

impl Apresentador for Gravador {
    fn definir_titulo(&mut self, titulo: &str) -> Result<()> {
        titulo.clone_into(&mut self.titulo);
        Ok(())
    }
    fn apresentar(&mut self, quadro: &Quadro) -> Result<()> {
        self.quadros.push(quadro.clone());
        Ok(())
    }

    fn eventos(&mut self) -> Result<Evento> {
        Ok(match self.fechar_depois_de {
            Some(n) if self.quadros.len() >= n => Evento::Fechar,
            _ => Evento::Nenhum,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Apresentador, Evento, Gravador, Quadro};

    fn quadro(largura: u32, altura: u32, preenchimento: u8) -> Quadro {
        Quadro::novo(
            largura,
            altura,
            vec![preenchimento; (largura * altura * 4) as usize],
        )
        .unwrap()
    }

    #[test]
    fn rampa_rgb_preserva_identidade_e_alfa_e_seleciona_canais_independentes() {
        let bytes: Vec<u8> = (0..=255u8)
            .flat_map(|v| [v, 255 - v, v / 2, v ^ 0xA5])
            .collect();
        let mut q = Quadro::novo(256, 1, bytes.clone()).unwrap();
        q.aplicar_rampa_rgb(&std::array::from_fn(|i| [i as u8; 3]));
        assert_eq!(q.bytes(), bytes);
        q.aplicar_rampa_rgb(&std::array::from_fn(|i| {
            [255 - i as u8, i as u8 / 2, i as u8 ^ 0x55]
        }));
        for (source, result) in bytes
            .as_chunks::<4>()
            .0
            .iter()
            .zip(q.bytes().as_chunks::<4>().0)
        {
            assert_eq!(
                result,
                &[source[0] ^ 0x55, source[1] / 2, 255 - source[2], source[3]]
            );
        }
    }

    #[test]
    fn quadro_exige_bytes_exatos() {
        assert!(Quadro::novo(2, 2, vec![0; 16]).is_ok());
        for tamanho in [0usize, 15, 17, 64] {
            assert!(
                Quadro::novo(2, 2, vec![0; tamanho]).is_err(),
                "aceitou {tamanho} bytes para 2x2"
            );
        }
    }

    #[test]
    fn quadro_recusa_dimensao_nula_e_transbordo() {
        assert!(Quadro::novo(0, 4, vec![]).is_err());
        assert!(Quadro::novo(4, 0, vec![]).is_err());
        // Largura × altura × 4 transborda `usize`: precisa dar erro, não pânico.
        assert!(Quadro::novo(u32::MAX, u32::MAX, vec![0; 16]).is_err());
    }

    #[test]
    fn pixel_devolve_bgra_na_posicao_certa_e_none_fora() {
        let mut bytes = vec![0u8; 3 * 2 * 4];
        // Pixel (2,1): último do quadro 3x2, índice (1*3+2)*4 = 20.
        bytes[20..24].copy_from_slice(&[1, 2, 3, 4]);
        let q = Quadro::novo(3, 2, bytes).unwrap();
        assert_eq!(q.pixel(2, 1), Some([1, 2, 3, 4]));
        assert_eq!(q.pixel(0, 0), Some([0, 0, 0, 0]));
        assert_eq!(q.pixel(3, 0), None);
        assert_eq!(q.pixel(0, 2), None);
        assert_eq!(q.pixel(u32::MAX, u32::MAX), None);
    }

    #[test]
    fn pixels_coloridos_ignora_o_canal_alfa() {
        // Só o alfa ligado não é tinta: preto opaco é fundo.
        let q = quadro(4, 4, 0);
        assert_eq!(q.pixels_coloridos(), 0);
        let mut bytes = vec![0u8; 4 * 4];
        bytes[3] = 0xFF;
        bytes[4..8].copy_from_slice(&[0, 0, 9, 0]);
        assert_eq!(Quadro::novo(2, 2, bytes).unwrap().pixels_coloridos(), 1);
    }

    #[test]
    fn debug_nao_despeja_os_bytes() {
        let texto = format!("{:?}", quadro(640, 480, 0xFF));
        assert!(
            texto.len() < 200,
            "Debug imprimiu o quadro inteiro: {} bytes",
            texto.len()
        );
        assert!(texto.contains("640"));
    }

    #[test]
    fn gravador_guarda_quadros_em_ordem() {
        let mut g = Gravador::novo();
        g.apresentar(&quadro(2, 2, 1)).unwrap();
        g.apresentar(&quadro(2, 2, 2)).unwrap();
        assert_eq!(g.quadros().len(), 2);
        assert_eq!(g.quadros()[0].pixel(0, 0), Some([1, 1, 1, 1]));
        assert_eq!(g.quadros()[1].pixel(0, 0), Some([2, 2, 2, 2]));
    }

    #[test]
    fn gravador_simula_o_usuario_fechando_a_janela() {
        let mut g = Gravador::fechar_depois_de(2);
        assert_eq!(g.eventos().unwrap(), Evento::Nenhum);
        g.apresentar(&quadro(2, 2, 0)).unwrap();
        assert_eq!(g.eventos().unwrap(), Evento::Nenhum);
        g.apresentar(&quadro(2, 2, 0)).unwrap();
        assert_eq!(g.eventos().unwrap(), Evento::Fechar);
    }

    #[test]
    fn gravador_padrao_nunca_pede_para_fechar() {
        let mut g = Gravador::novo();
        for _ in 0..10 {
            g.apresentar(&quadro(2, 2, 0)).unwrap();
            assert_eq!(g.eventos().unwrap(), Evento::Nenhum);
        }
    }
}
