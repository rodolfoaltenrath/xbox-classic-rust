//! Janela Win32 para apresentar quadros, com as funções do sistema declaradas à mão.
//!
//! Sem `windows-sys` (ADR-0008): o toolchain `windows-gnu` do host de desenvolvimento
//! não linka crates que dependem dele, mas `libuser32.a` e `libgdi32.a` vêm no MinGW
//! empacotado do rustup, ao lado da `libkernel32.a` que a arena já usa. Declarar o punhado
//! de funções necessárias é menos dependência, não mais.
//!
//! ## Todo o `unsafe` do módulo, e por que é seguro
//!
//! | Bloco | Invariante |
//! |---|---|
//! | chamadas de criação (`RegisterClassExW`, `CreateWindowExW`, ...) | argumentos vêm de literais e de locais vivos na chamada; falha devolve nulo/zero e é tratada |
//! | `SetWindowLongPtrW`/`GetWindowLongPtrW` | o valor guardado é o endereço de um [`Estado`] dentro de um `Box` que o [`Janela`] possui e **só solta depois** de destruir a janela |
//! | `StretchDIBits` | `bytes` vem de um [`Quadro`], cujo construtor garante `largura × altura × 4` bytes; o cabeçalho descreve exatamente essas dimensões |
//! | `wnd_proc` | só lê o `Estado` por ponteiro obtido do `GWLP_USERDATA`, nulo até `nova` o configurar |
//!
//! A `wnd_proc` não toca em estado global mutável: o único canal entre ela e o
//! apresentador é o `Estado` da janela, e ele só contém dois booleanos (`Cell`), porque
//! tudo acontece na thread que criou a janela.

use std::cell::Cell;
use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicU32, Ordering};

use xbox_core::{Error, Result};

use crate::display::{Apresentador, Evento, Quadro};

// ─── Declarações do Win32 ────────────────────────────────────────────────────────

/// Numera as classes de janela deste processo: duas janelas não podem disputar o registro
/// do mesmo nome de classe.
static CONTADOR: AtomicU32 = AtomicU32::new(0);

type Hwnd = *mut c_void;
type Hinstance = *mut c_void;
type Hdc = *mut c_void;
type Wparam = usize;
type Lparam = isize;
type Lresult = isize;
type Atom = u16;

const WM_CLOSE: u32 = 0x0010;
const WM_ERASEBKGND: u32 = 0x0014;
const PM_REMOVE: u32 = 0x0001;
const WS_OVERLAPPED: u32 = 0x0000_0000;
const WS_CAPTION: u32 = 0x00C0_0000;
const WS_SYSMENU: u32 = 0x0008_0000;
const WS_MINIMIZEBOX: u32 = 0x0002_0000;
const CW_USEDEFAULT: i32 = i32::MIN;
const SW_SHOWNORMAL: i32 = 1;
const SRCCOPY: u32 = 0x00CC_0020;
const DIB_RGB_COLORS: u32 = 0;
const BI_RGB: u32 = 0;
const GWLP_USERDATA: i32 = -21;
const IDC_ARROW: usize = 32512;
const CS_OWNDC: u32 = 0x0020;

#[repr(C)]
struct WndClassExW {
    cb_size: u32,
    style: u32,
    lpfn_wnd_proc: unsafe extern "system" fn(Hwnd, u32, Wparam, Lparam) -> Lresult,
    cb_cls_extra: i32,
    cb_wnd_extra: i32,
    h_instance: Hinstance,
    h_icon: *mut c_void,
    h_cursor: *mut c_void,
    hbr_background: *mut c_void,
    lpsz_menu_name: *const u16,
    lpsz_class_name: *const u16,
    h_icon_sm: *mut c_void,
}

#[repr(C)]
struct Point {
    x: i32,
    y: i32,
}

#[repr(C)]
struct Msg {
    hwnd: Hwnd,
    message: u32,
    w_param: Wparam,
    l_param: Lparam,
    time: u32,
    pt: Point,
}

#[repr(C)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

// Os nomes dos campos seguem o `BITMAPINFOHEADER` da API do Windows, que é como a
// documentação da Microsoft os escreve: o prefixo `bi` é do tipo original, não ruído.
#[allow(clippy::struct_field_names)]
#[repr(C)]
struct BitmapInfoHeader {
    bi_size: u32,
    bi_width: i32,
    bi_height: i32,
    bi_planes: u16,
    bi_bit_count: u16,
    bi_compression: u32,
    bi_size_image: u32,
    bi_x_pels_per_meter: i32,
    bi_y_pels_per_meter: i32,
    bi_clr_used: u32,
    bi_clr_important: u32,
}

#[link(name = "user32")]
unsafe extern "system" {
    fn RegisterClassExW(classe: *const WndClassExW) -> Atom;
    fn UnregisterClassW(nome: *const u16, instancia: Hinstance) -> i32;
    fn CreateWindowExW(
        estilo_ex: u32,
        classe: *const u16,
        titulo: *const u16,
        estilo: u32,
        x: i32,
        y: i32,
        largura: i32,
        altura: i32,
        pai: Hwnd,
        menu: *mut c_void,
        instancia: Hinstance,
        param: *mut c_void,
    ) -> Hwnd;
    fn DestroyWindow(janela: Hwnd) -> i32;
    fn ShowWindow(janela: Hwnd, modo: i32) -> i32;
    fn PeekMessageW(msg: *mut Msg, janela: Hwnd, min: u32, max: u32, remover: u32) -> i32;
    fn TranslateMessage(msg: *const Msg) -> i32;
    fn DispatchMessageW(msg: *const Msg) -> Lresult;
    fn DefWindowProcW(janela: Hwnd, msg: u32, w: Wparam, l: Lparam) -> Lresult;
    fn GetDC(janela: Hwnd) -> Hdc;
    fn ReleaseDC(janela: Hwnd, dc: Hdc) -> i32;
    fn GetClientRect(janela: Hwnd, rect: *mut Rect) -> i32;
    fn AdjustWindowRect(rect: *mut Rect, estilo: u32, menu: i32) -> i32;
    fn LoadCursorW(instancia: Hinstance, nome: *const u16) -> *mut c_void;
    fn SetWindowLongPtrW(janela: Hwnd, indice: i32, valor: isize) -> isize;
    fn GetWindowLongPtrW(janela: Hwnd, indice: i32) -> isize;
}

#[link(name = "gdi32")]
unsafe extern "system" {
    fn StretchDIBits(
        dc: Hdc,
        x_dest: i32,
        y_dest: i32,
        largura_dest: i32,
        altura_dest: i32,
        x_origem: i32,
        y_origem: i32,
        largura_origem: i32,
        altura_origem: i32,
        bits: *const c_void,
        info: *const BitmapInfoHeader,
        uso: u32,
        operacao: u32,
    ) -> i32;
    #[cfg(test)]
    fn GetPixel(dc: Hdc, x: i32, y: i32) -> u32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(nome: *const u16) -> Hinstance;
    fn GetLastError() -> u32;
}

// ─── Estado compartilhado com a janela ───────────────────────────────────────────

/// O que a `wnd_proc` pode comunicar ao apresentador.
///
/// `Cell`, não atômico: a `wnd_proc` só roda na thread que criou a janela, dentro de
/// `PeekMessageW`/`DispatchMessageW`, que o próprio apresentador chama.
#[derive(Default)]
struct Estado {
    fechar_pedido: Cell<bool>,
}

/// Janela Win32 que apresenta quadros.
///
/// `!Send` e `!Sync` por construção (guarda ponteiros brutos): uma janela pertence à
/// thread que a criou.
pub(crate) struct Janela {
    hwnd: Hwnd,
    classe: Vec<u16>,
    instancia: Hinstance,
    // `Box` para o endereço ser estável: ele fica guardado em GWLP_USERDATA.
    estado: Box<Estado>,
}

fn utf16(texto: &str) -> Vec<u16> {
    texto.encode_utf16().chain(std::iter::once(0)).collect()
}

fn erro_win32(contexto: &str) -> Error {
    // SAFETY: só lê o código de erro da thread atual.
    let codigo = unsafe { GetLastError() };
    Error::format(format!("{contexto} falhou (erro Win32 {codigo})"))
}

unsafe extern "system" fn wnd_proc(hwnd: Hwnd, msg: u32, w: Wparam, l: Lparam) -> Lresult {
    // SAFETY: o valor em GWLP_USERDATA é nulo até `nova` o configurar (mensagens de
    // criação, como WM_NCCREATE, chegam antes disso e veem nulo), e depois é o endereço de
    // um `Estado` dentro do `Box` do `Janela`, que só é solto depois de `DestroyWindow`.
    // O `Drop` zera o valor antes de destruir, então nem mensagens de destruição leem
    // um ponteiro solto.
    let estado = unsafe { (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Estado).as_ref() };
    match msg {
        WM_CLOSE => {
            // Não destrói: quem decide encerrar é o laço de emulação, via `Evento::Fechar`.
            if let Some(e) = estado {
                e.fechar_pedido.set(true);
                return 0;
            }
            // SAFETY: repassa os mesmos argumentos que recebemos.
            unsafe { DefWindowProcW(hwnd, msg, w, l) }
        }
        // Evita o flash de fundo a cada quadro: `apresentar` cobre a área cliente inteira.
        // Consequência: antes do primeiro quadro o fundo é indefinido, e `eventos` só
        // deve ser chamado depois de haver o que mostrar.
        WM_ERASEBKGND => 1,
        _ => {
            // SAFETY: repassa os mesmos argumentos que recebemos.
            unsafe { DefWindowProcW(hwnd, msg, w, l) }
        }
    }
}

impl Janela {
    /// Cria uma janela com área cliente de `largura × altura` pixels.
    pub(crate) fn nova(titulo: &str, largura: u32, altura: u32) -> Result<Self> {
        let largura_i = i32::try_from(largura)
            .map_err(|_| Error::format("largura da janela não cabe em i32"))?;
        let altura_i =
            i32::try_from(altura).map_err(|_| Error::format("altura da janela não cabe em i32"))?;
        if largura == 0 || altura == 0 {
            return Err(Error::format("janela de dimensão nula"));
        }

        // Nome único por instância: duas janelas no mesmo processo não podem disputar o
        // registro da mesma classe, e o `Drop` desregistra a sua.
        let n = CONTADOR.fetch_add(1, Ordering::Relaxed);
        let classe = utf16(&format!("ObsidianFrame{}_{n}", std::process::id()));
        let titulo = utf16(titulo);

        // SAFETY: nome nulo devolve o módulo do próprio executável.
        let instancia = unsafe { GetModuleHandleW(std::ptr::null()) };
        if instancia.is_null() {
            return Err(erro_win32("GetModuleHandleW"));
        }

        let estilo = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
        // A janela fixa não é redimensionável: o quadro tem proporção definida, e sem
        // borda de redimensionamento não há como o usuário distorcê-la.
        let mut borda = Rect {
            left: 0,
            top: 0,
            right: largura_i,
            bottom: altura_i,
        };
        // SAFETY: `borda` é um local vivo e válido para escrita.
        if unsafe { AdjustWindowRect(&raw mut borda, estilo, 0) } == 0 {
            return Err(erro_win32("AdjustWindowRect"));
        }

        let wc = WndClassExW {
            cb_size: u32::try_from(std::mem::size_of::<WndClassExW>()).unwrap_or(0),
            style: CS_OWNDC,
            lpfn_wnd_proc: wnd_proc,
            cb_cls_extra: 0,
            cb_wnd_extra: 0,
            h_instance: instancia,
            h_icon: null_mut(),
            // SAFETY: `IDC_ARROW` é um identificador de recurso do sistema, não ponteiro
            // real; é assim que a API pede o cursor padrão.
            h_cursor: unsafe { LoadCursorW(null_mut(), IDC_ARROW as *const u16) },
            hbr_background: null_mut(),
            lpsz_menu_name: std::ptr::null(),
            lpsz_class_name: classe.as_ptr(),
            h_icon_sm: null_mut(),
        };
        // SAFETY: `wc` e o nome da classe estão vivos durante a chamada.
        if unsafe { RegisterClassExW(&raw const wc) } == 0 {
            return Err(erro_win32("RegisterClassExW"));
        }

        let estado = Box::<Estado>::default();
        // SAFETY: classe e título são UTF-16 terminados em nulo e vivos na chamada.
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                classe.as_ptr(),
                titulo.as_ptr(),
                estilo,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                borda.right - borda.left,
                borda.bottom - borda.top,
                null_mut(),
                null_mut(),
                instancia,
                null_mut(),
            )
        };
        if hwnd.is_null() {
            let erro = erro_win32("CreateWindowExW");
            // SAFETY: desfaz o registro que acabamos de fazer.
            unsafe { UnregisterClassW(classe.as_ptr(), instancia) };
            return Err(erro);
        }

        // SAFETY: `hwnd` é válido; o valor é o endereço do `Estado` no `Box`, que o
        // `Janela` mantém vivo até depois de `DestroyWindow` (ver `Drop`).
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, &raw const *estado as isize);
            ShowWindow(hwnd, SW_SHOWNORMAL);
        }

        Ok(Self {
            hwnd,
            classe,
            instancia,
            estado,
        })
    }

    /// Lê a cor de um pixel da área cliente, como `0x00BBGGRR`, direto do DC da janela.
    ///
    /// Existe para **verificar** que o que foi apresentado chegou na superfície, e não só
    /// que a chamada de desenho retornou. Só existe em teste: a capacidade de ler a tela de
    /// volta não faz parte do contrato de apresentação nem do build de produção.
    /// Largura e altura da área cliente, como o sistema a calculou. Só em teste.
    #[cfg(test)]
    pub(crate) fn area_cliente(&self) -> Option<(i32, i32)> {
        let mut r = Rect {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        // SAFETY: `hwnd` é válido e `r` é um local vivo e gravável.
        (unsafe { GetClientRect(self.hwnd, &raw mut r) } != 0)
            .then_some((r.right - r.left, r.bottom - r.top))
    }

    #[cfg(test)]
    pub(crate) fn ler_pixel(&self, x: i32, y: i32) -> Option<u32> {
        // SAFETY: `hwnd` é válido; o DC obtido é liberado antes de retornar.
        unsafe {
            let dc = GetDC(self.hwnd);
            if dc.is_null() {
                return None;
            }
            let cor = GetPixel(dc, x, y);
            ReleaseDC(self.hwnd, dc);
            // CLR_INVALID: ponto fora da área recortada ou sem superfície.
            (cor != 0xFFFF_FFFF).then_some(cor)
        }
    }
}

impl Apresentador for Janela {
    fn apresentar(&mut self, quadro: &Quadro) -> Result<()> {
        let largura = i32::try_from(quadro.largura())
            .map_err(|_| Error::format("largura do quadro não cabe em i32"))?;
        let altura = i32::try_from(quadro.altura())
            .map_err(|_| Error::format("altura do quadro não cabe em i32"))?;

        let cabecalho = BitmapInfoHeader {
            bi_size: u32::try_from(std::mem::size_of::<BitmapInfoHeader>()).unwrap_or(0),
            bi_width: largura,
            // Altura negativa = DIB top-down: a primeira linha do buffer é a de cima, que
            // é a ordem do framebuffer do guest. Sem isso a imagem sairia espelhada.
            bi_height: -altura,
            bi_planes: 1,
            bi_bit_count: 32,
            bi_compression: BI_RGB,
            bi_size_image: 0,
            bi_x_pels_per_meter: 0,
            bi_y_pels_per_meter: 0,
            bi_clr_used: 0,
            bi_clr_important: 0,
        };

        let mut cliente = Rect {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        // SAFETY: `hwnd` é válido e `cliente` é um local vivo e gravável.
        if unsafe { GetClientRect(self.hwnd, &raw mut cliente) } == 0 {
            return Err(erro_win32("GetClientRect"));
        }
        let (dest_l, dest_a) = (cliente.right - cliente.left, cliente.bottom - cliente.top);

        // SAFETY: `hwnd` é válido. O DC é liberado em todos os caminhos abaixo.
        let dc = unsafe { GetDC(self.hwnd) };
        if dc.is_null() {
            return Err(erro_win32("GetDC"));
        }
        // SAFETY: `quadro.bytes()` tem exatamente `largura × altura × 4` bytes — o
        // construtor de `Quadro` garante isso — e o cabeçalho descreve exatamente essas
        // dimensões em 32 bpp, então o sistema nunca lê além do buffer. O DC é válido.
        let linhas = unsafe {
            StretchDIBits(
                dc,
                0,
                0,
                dest_l,
                dest_a,
                0,
                0,
                largura,
                altura,
                quadro.bytes().as_ptr().cast::<c_void>(),
                &raw const cabecalho,
                DIB_RGB_COLORS,
                SRCCOPY,
            )
        };
        // SAFETY: devolve o DC obtido acima.
        unsafe { ReleaseDC(self.hwnd, dc) };

        if linhas == 0 {
            return Err(erro_win32("StretchDIBits"));
        }
        Ok(())
    }

    fn eventos(&mut self) -> Result<Evento> {
        let mut msg = Msg {
            hwnd: null_mut(),
            message: 0,
            w_param: 0,
            l_param: 0,
            time: 0,
            pt: Point { x: 0, y: 0 },
        };
        // Esvazia a fila inteira: com mensagens acumuladas, a janela ficaria "não
        // respondendo". Só desta janela (`hwnd`), nunca de outras threads.
        // SAFETY: `msg` é um local vivo e gravável; `hwnd` é válido.
        while unsafe { PeekMessageW(&raw mut msg, self.hwnd, 0, 0, PM_REMOVE) } != 0 {
            // SAFETY: `msg` foi preenchida por `PeekMessageW` logo acima.
            unsafe {
                TranslateMessage(&raw const msg);
                DispatchMessageW(&raw const msg);
            }
        }
        Ok(if self.estado.fechar_pedido.get() {
            Evento::Fechar
        } else {
            Evento::Nenhum
        })
    }
}

impl Drop for Janela {
    fn drop(&mut self) {
        // A janela é destruída ANTES do `Box<Estado>` ser solto (campos caem depois do
        // corpo de `drop`), então a `wnd_proc` nunca vê um ponteiro solto — mesmo que
        // `DestroyWindow` entregue mensagens durante a destruição.
        // SAFETY: `hwnd` e `classe` foram criados por `nova` e este é o único dono.
        unsafe {
            SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
            DestroyWindow(self.hwnd);
            UnregisterClassW(self.classe.as_ptr(), self.instancia);
        }
    }
}

impl std::fmt::Debug for Janela {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Janela")
            .field("fechar_pedido", &self.estado.fechar_pedido.get())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::{Janela, WM_CLOSE};
    use crate::display::{Apresentador, Evento, Quadro};

    #[link(name = "user32")]
    unsafe extern "system" {
        fn PostMessageW(janela: super::Hwnd, msg: u32, w: usize, l: isize) -> i32;
    }

    /// Tamanho do quadro de teste. É o do framebuffer real do guest, e **não é arbitrário**:
    /// o Windows impõe uma largura mínima à janela por causa da barra de título, então uma
    /// janela de 64 px de cliente sai com 130 px, e o quadro seria esticado — foi assim que
    /// a primeira versão deste teste falhou, lendo a metade esquerda de uma imagem escalada
    /// 2x. Em 640x480 a escala é 1:1, e o teste afirma isso em vez de assumir.
    const L: u32 = 640;
    const A: u32 = 480;

    /// Cores distintas por quadrante, em ordem `B,G,R,A`, para detectar qualquer
    /// espelhamento ou troca de canal: se algo estiver invertido, o quadrante errado
    /// aparece na posição errada.
    fn quadro_de_teste() -> Quadro {
        let mut bytes = Vec::with_capacity((L * A * 4) as usize);
        for y in 0..A {
            for x in 0..L {
                // `b, g, r` são os canais de cor, na ordem de memória do guest.
                #[allow(clippy::many_single_char_names)]
                let (b, g, r) = match (x < L / 2, y < A / 2) {
                    (true, true) => (0x00, 0x00, 0xFF),   // alto-esquerda: vermelho
                    (false, true) => (0x00, 0xFF, 0x00),  // alto-direita: verde
                    (true, false) => (0xFF, 0x00, 0x00),  // baixo-esquerda: azul
                    (false, false) => (0xFF, 0xFF, 0xFF), // baixo-direita: branco
                };
                bytes.extend_from_slice(&[b, g, r, 0]);
            }
        }
        Quadro::novo(L, A, bytes).unwrap()
    }

    /// `GetPixel` devolve `0x00BBGGRR`.
    const fn colorref(r: u32, g: u32, b: u32) -> u32 {
        (b << 16) | (g << 8) | r
    }

    #[test]
    fn quadro_chega_na_superficie_com_orientacao_e_canais_corretos() {
        let mut janela = Janela::nova("obsidian-teste", L, A).expect("janela");
        // A premissa do teste, afirmada: cliente 1:1 com o quadro. Se o sistema devolver
        // outra coisa, a falha diz isso em vez de aparecer como pixel de cor errada.
        assert_eq!(
            janela.area_cliente(),
            Some((L.cast_signed(), A.cast_signed())),
            "a área cliente não é do tamanho pedido: o quadro seria escalado"
        );
        let quadro = quadro_de_teste();
        janela.apresentar(&quadro).expect("apresentar");
        janela.eventos().expect("eventos");

        // Lê de volta da superfície: prova que o desenho chegou, não que a chamada voltou.
        // Se este host não tem sessão gráfica, `ler_pixel` devolve None e o teste avisa em
        // vez de passar sem verificar nada.
        let Some(topo_esq) = janela.ler_pixel(100, 100) else {
            eprintln!("aviso: sem superfície legível neste host; verificação de pixel pulada");
            return;
        };
        assert_eq!(
            topo_esq,
            colorref(0xFF, 0x00, 0x00),
            "alto-esquerda deveria ser vermelho"
        );
        assert_eq!(
            janela.ler_pixel(540, 100),
            Some(colorref(0x00, 0xFF, 0x00)),
            "alto-direita deveria ser verde"
        );
        assert_eq!(
            janela.ler_pixel(100, 380),
            Some(colorref(0x00, 0x00, 0xFF)),
            "baixo-esquerda deveria ser azul: imagem espelhada na vertical?"
        );
        assert_eq!(
            janela.ler_pixel(540, 380),
            Some(colorref(0xFF, 0xFF, 0xFF)),
            "baixo-direita deveria ser branco"
        );
    }

    #[test]
    fn fechar_a_janela_vira_evento_e_nao_destroi_nada() {
        let mut janela = Janela::nova("obsidian-teste", L, A).expect("janela");
        janela.apresentar(&quadro_de_teste()).unwrap();
        assert_eq!(janela.eventos().unwrap(), Evento::Nenhum);

        // SAFETY: `hwnd` é válido; WM_CLOSE sem parâmetros é uma mensagem normal.
        assert_ne!(unsafe { PostMessageW(janela.hwnd, WM_CLOSE, 0, 0) }, 0);

        assert_eq!(janela.eventos().unwrap(), Evento::Fechar);
        // O pedido persiste até o laço agir, e a janela continua utilizável: quem encerra
        // é o emulador, não o sistema.
        assert_eq!(janela.eventos().unwrap(), Evento::Fechar);
        janela
            .apresentar(&quadro_de_teste())
            .expect("janela deve continuar viva");
    }

    #[test]
    fn criar_e_soltar_muitas_janelas_nao_vaza_classe_nem_trava() {
        for i in 0..40 {
            let mut janela = Janela::nova("obsidian-ciclo", 32, 24).unwrap_or_else(|e| {
                panic!("janela {i}: {e}");
            });
            let quadro = Quadro::novo(32, 24, vec![0x80; 32 * 24 * 4]).unwrap();
            janela.apresentar(&quadro).unwrap();
            janela.eventos().unwrap();
        }
    }

    #[test]
    fn dimensao_nula_e_recusada_antes_de_tocar_o_sistema() {
        assert!(Janela::nova("x", 0, 480).is_err());
        assert!(Janela::nova("x", 640, 0).is_err());
        assert!(Janela::nova("x", u32::MAX, 480).is_err(), "não cabe em i32");
    }

    #[test]
    fn quadro_de_tamanho_diferente_da_janela_e_escalado_sem_falhar() {
        // A janela é 64x48 e o quadro 32x24: StretchDIBits escala.
        let mut janela = Janela::nova("obsidian-escala", 64, 48).unwrap();
        let pequeno = Quadro::novo(32, 24, vec![0xFF; 32 * 24 * 4]).unwrap();
        janela.apresentar(&pequeno).unwrap();
        if let Some(cor) = janela.ler_pixel(60, 44) {
            assert_eq!(
                cor,
                colorref(0xFF, 0xFF, 0xFF),
                "o quadro deveria cobrir a janela toda"
            );
        }
    }
}
