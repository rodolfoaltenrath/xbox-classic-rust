//! Codificação e decodificação do protocolo X11 core usado pelo apresentador.
//!
//! Tudo aqui é puro: bytes entram, bytes ou valores saem. Nenhuma função abre socket,
//! o que permite testar cada requisição, a leitura do setup e a interpretação de
//! eventos sem servidor X. A conexão em si vive em `super`.
//!
//! Referência: *X Window System Protocol*, X Consortium Standard, versão 11 release
//! 6.9/7 (capítulos "Connection Setup", "Requests", "Events" e o apêndice
//! "Protocol Encoding"). Só o recorte que o apresentador usa está codificado; cada
//! opcode abaixo cita o nome da requisição na especificação.

use std::collections::VecDeque;

use xbox_core::{Error, Result};

use crate::display::Evento;

// ─── Opcodes de requisição (apêndice "Protocol Encoding") ───────────────────────

pub(crate) const CREATE_WINDOW: u8 = 1;
pub(crate) const DESTROY_WINDOW: u8 = 4;
pub(crate) const MAP_WINDOW: u8 = 8;
pub(crate) const QUERY_TREE: u8 = 15;
pub(crate) const INTERN_ATOM: u8 = 16;
pub(crate) const CHANGE_PROPERTY: u8 = 18;
pub(crate) const GET_PROPERTY: u8 = 20;
pub(crate) const SEND_EVENT: u8 = 25;
pub(crate) const GET_INPUT_FOCUS: u8 = 43;
pub(crate) const CREATE_GC: u8 = 55;
pub(crate) const FREE_GC: u8 = 60;
pub(crate) const PUT_IMAGE: u8 = 72;
pub(crate) const GET_IMAGE: u8 = 73;
pub(crate) const GET_KEYBOARD_MAPPING: u8 = 101;

// ─── Átomos predefinidos ────────────────────────────────────────────────────────

pub(crate) const ATOM_ATOM: u32 = 4;
pub(crate) const ATOM_STRING: u32 = 31;
pub(crate) const ATOM_WM_NAME: u32 = 39;
pub(crate) const ATOM_WM_NORMAL_HINTS: u32 = 40;
pub(crate) const ATOM_WM_SIZE_HINTS: u32 = 41;
pub(crate) const ATOM_WM_CLASS: u32 = 67;

// ─── Eventos ────────────────────────────────────────────────────────────────────

pub(crate) const KEY_PRESS: u8 = 2;
pub(crate) const KEY_RELEASE: u8 = 3;
pub(crate) const EXPOSE: u8 = 12;
pub(crate) const DESTROY_NOTIFY: u8 = 17;
pub(crate) const CLIENT_MESSAGE: u8 = 33;

pub(crate) const MASCARA_KEY_PRESS: u32 = 1 << 0;
pub(crate) const MASCARA_KEY_RELEASE: u32 = 1 << 1;
pub(crate) const MASCARA_EXPOSURE: u32 = 1 << 15;
pub(crate) const MASCARA_STRUCTURE_NOTIFY: u32 = 1 << 17;

/// Keysyms dos atalhos do host (`keysymdef.h`). Não são input do Xbox.
pub(crate) const XK_SPACE: u32 = 0x0020;
pub(crate) const XK_F5: u32 = 0xFFC2;
pub(crate) const XK_F9: u32 = 0xFFC6;

/// Classe de visual `TrueColor`.
const TRUE_COLOR: u8 = 4;

/// Valor de `WM_CLASS`: instância e classe, cada uma terminada em NUL. É por ele que a
/// ferramenta de verificação encontra a janela sem depender do título, que muda.
pub(crate) const WM_CLASS: &[u8] = b"obsidian\0Obsidian\0";

/// Limite da fila de atalhos, igual ao do apresentador Win32: segura o backlog sem
/// jamais perder o pedido de fechar, que tem sinal próprio e prioritário.
const FILA_MAXIMA: usize = 32;

fn pad4(n: usize) -> usize {
    (4 - n % 4) % 4
}

fn u16_le(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}

fn u32_le(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

fn curto(o_que: &str) -> Error {
    Error::format(format!("X11: {o_que} truncado"))
}

// ─── DISPLAY e Xauthority ───────────────────────────────────────────────────────

/// Número do display local lido de `DISPLAY` (`:0`, `:1.0`, `unix:0`).
///
/// Display remoto (`host:N`, típico de encaminhamento SSH) usaria TCP e não faz parte
/// deste recorte: recusa com o valor recebido em vez de tentar outra coisa.
pub(crate) fn numero_do_display(display: &str) -> Result<u32> {
    let (host, resto) = display
        .rsplit_once(':')
        .ok_or_else(|| Error::format(format!("DISPLAY sem ':': {display:?}")))?;
    if !host.is_empty() && host != "unix" {
        return Err(Error::format(format!(
            "DISPLAY {display:?} é remoto; só displays locais (:N) são suportados"
        )));
    }
    let numero = resto.split('.').next().unwrap_or_default();
    numero
        .parse()
        .map_err(|_| Error::format(format!("número de display inválido em {display:?}")))
}

/// Uma entrada de `.Xauthority`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EntradaAuth {
    pub(crate) familia: u16,
    pub(crate) endereco: Vec<u8>,
    pub(crate) numero: Vec<u8>,
    pub(crate) nome: Vec<u8>,
    pub(crate) dados: Vec<u8>,
}

/// Família `FamilyLocal` e `FamilyWild` do Xau.
const FAMILIA_LOCAL: u16 = 256;
const FAMILIA_WILD: u16 = 65535;
pub(crate) const MIT_MAGIC_COOKIE: &[u8] = b"MIT-MAGIC-COOKIE-1";

/// Lê todas as entradas de um arquivo Xauthority (formato da libXau: campos com
/// comprimento `u16` big-endian). Arquivo truncado é erro, não lista parcial.
pub(crate) fn ler_xauthority(bytes: &[u8]) -> Result<Vec<EntradaAuth>> {
    fn campo<'a>(b: &'a [u8], i: &mut usize) -> Result<&'a [u8]> {
        let n = b
            .get(*i..*i + 2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]) as usize)
            .ok_or_else(|| curto("Xauthority"))?;
        let v = b
            .get(*i + 2..*i + 2 + n)
            .ok_or_else(|| curto("Xauthority"))?;
        *i += 2 + n;
        Ok(v)
    }
    let mut entradas = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let familia = bytes
            .get(i..i + 2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .ok_or_else(|| curto("Xauthority"))?;
        i += 2;
        entradas.push(EntradaAuth {
            familia,
            endereco: campo(bytes, &mut i)?.to_vec(),
            numero: campo(bytes, &mut i)?.to_vec(),
            nome: campo(bytes, &mut i)?.to_vec(),
            dados: campo(bytes, &mut i)?.to_vec(),
        });
    }
    Ok(entradas)
}

/// Escolhe o cookie MIT-MAGIC-COOKIE-1 do display local `numero`.
///
/// Prefere entradas `Local`/`Wild` (as que Xorg e Xwayland gravam para socket Unix) e
/// depois qualquer família com o mesmo número; número vazio vale para todos.
pub(crate) fn escolher_cookie(entradas: &[EntradaAuth], numero: u32) -> Option<Vec<u8>> {
    let alvo = numero.to_string();
    let candidatas = entradas.iter().filter(|e| {
        e.nome == MIT_MAGIC_COOKIE && (e.numero.is_empty() || e.numero == alvo.as_bytes())
    });
    let (locais, outras): (Vec<_>, Vec<_>) =
        candidatas.partition(|e| matches!(e.familia, FAMILIA_LOCAL | FAMILIA_WILD));
    locais
        .first()
        .or_else(|| outras.first())
        .map(|e| e.dados.clone())
}

// ─── Setup da conexão ───────────────────────────────────────────────────────────

/// Pedido inicial: ordem de bytes little-endian (`'l'`), protocolo 11.0.
pub(crate) fn pedido_de_setup(nome_auth: &[u8], dados_auth: &[u8]) -> Vec<u8> {
    let mut b = vec![b'l', 0];
    b.extend_from_slice(&11u16.to_le_bytes());
    b.extend_from_slice(&0u16.to_le_bytes());
    b.extend_from_slice(&(nome_auth.len() as u16).to_le_bytes());
    b.extend_from_slice(&(dados_auth.len() as u16).to_le_bytes());
    b.extend_from_slice(&[0, 0]);
    for parte in [nome_auth, dados_auth] {
        b.extend_from_slice(parte);
        b.resize(b.len() + pad4(parte.len()), 0);
    }
    b
}

/// O que o apresentador precisa do setup aceito pelo servidor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Setup {
    pub(crate) id_base: u32,
    pub(crate) id_mascara: u32,
    /// Tamanho máximo de requisição, em unidades de 4 bytes.
    pub(crate) requisicao_maxima: u16,
    /// Ordem de bytes das imagens: `true` = `LSBFirst`.
    pub(crate) imagem_lsb: bool,
    pub(crate) keycode_min: u8,
    pub(crate) keycode_max: u8,
    pub(crate) raiz: u32,
    pub(crate) visual: u32,
    pub(crate) profundidade: u8,
}

/// Tamanho total da resposta de setup a partir dos 8 bytes iniciais.
pub(crate) fn tamanho_do_setup(cabecalho: [u8; 8]) -> usize {
    8 + 4 * u16_le(&cabecalho, 6) as usize
}

/// Interpreta a resposta de setup da tela `tela`, exigindo o único formato que a cópia
/// direta do quadro suporta: profundidade 24 em 32 bits por pixel, `TrueColor` com
/// máscaras R=0xFF0000, G=0xFF00, B=0xFF no visual raiz.
///
/// Outros formatos são recusados com o que o servidor ofereceu: converter cores para
/// 16 bits ou criar colormap próprio é trabalho que nenhum alvo pediu até agora.
pub(crate) fn ler_setup(b: &[u8], tela: usize) -> Result<Setup> {
    if b.len() < 8 {
        return Err(curto("setup"));
    }
    match b[0] {
        1 => {}
        0 => {
            let n = b[1] as usize;
            let motivo = b.get(8..8 + n).unwrap_or_default();
            return Err(Error::format(format!(
                "servidor X recusou a conexão: {}",
                String::from_utf8_lossy(motivo).trim()
            )));
        }
        2 => {
            let motivo = b.get(8..).unwrap_or_default();
            return Err(Error::format(format!(
                "servidor X exige autenticação adicional: {}",
                String::from_utf8_lossy(motivo)
                    .trim_end_matches('\0')
                    .trim()
            )));
        }
        s => {
            return Err(Error::format(format!(
                "status de setup X11 desconhecido: {s}"
            )));
        }
    }
    if b.len() < 40 {
        return Err(curto("setup"));
    }
    let vendor = u16_le(b, 24) as usize;
    let telas = b[28] as usize;
    let formatos = b[29] as usize;
    let mut i = 40 + vendor + pad4(vendor);
    let mut formato_24_32 = false;
    for _ in 0..formatos {
        let formato = b.get(i..i + 8).ok_or_else(|| curto("formatos do setup"))?;
        formato_24_32 |= formato[0] == 24 && formato[1] == 32;
        i += 8;
    }
    if tela >= telas {
        return Err(Error::format(format!(
            "tela {tela} pedida, servidor X tem {telas}"
        )));
    }
    for n in 0..telas {
        let bytes_tela = b.get(i..i + 40).ok_or_else(|| curto("tela do setup"))?;
        let raiz = u32_le(bytes_tela, 0);
        let visual_raiz = u32_le(bytes_tela, 32);
        let profundidade = bytes_tela[38];
        let profundidades = bytes_tela[39] as usize;
        i += 40;
        let mut visual_ok = false;
        for _ in 0..profundidades {
            let bytes_prof = b
                .get(i..i + 8)
                .ok_or_else(|| curto("profundidade do setup"))?;
            let visuais = u16_le(bytes_prof, 2) as usize;
            i += 8;
            for _ in 0..visuais {
                let bytes_visual = b.get(i..i + 24).ok_or_else(|| curto("visual do setup"))?;
                if u32_le(bytes_visual, 0) == visual_raiz {
                    visual_ok = bytes_visual[4] == TRUE_COLOR
                        && u32_le(bytes_visual, 8) == 0xFF_0000
                        && u32_le(bytes_visual, 12) == 0xFF00
                        && u32_le(bytes_visual, 16) == 0xFF;
                }
                i += 24;
            }
        }
        if n != tela {
            continue;
        }
        if profundidade != 24 || !formato_24_32 || !visual_ok {
            return Err(Error::format(format!(
                "visual raiz do servidor X não é TrueColor 24 bits em 32 bpp \
                 (profundidade {profundidade}, formato 24/32 {formato_24_32}, \
                 máscaras RGB888 {visual_ok}); só esse formato é suportado"
            )));
        }
        return Ok(Setup {
            id_base: u32_le(b, 12),
            id_mascara: u32_le(b, 16),
            requisicao_maxima: u16_le(b, 26),
            imagem_lsb: b[30] == 0,
            keycode_min: b[34],
            keycode_max: b[35],
            raiz,
            visual: visual_raiz,
            profundidade,
        });
    }
    unreachable!("a tela pedida foi validada contra o total")
}

/// N-ésimo identificador de recurso livre para o cliente.
pub(crate) fn id_de_recurso(setup: &Setup, n: u32) -> u32 {
    setup.id_base | ((n << setup.id_mascara.trailing_zeros()) & setup.id_mascara)
}

// ─── Requisições ────────────────────────────────────────────────────────────────

/// Monta uma requisição: opcode, byte de dados, comprimento em palavras e o corpo
/// completado até múltiplo de 4.
pub(crate) fn requisicao(opcode: u8, dado: u8, corpo: &[u8]) -> Vec<u8> {
    let total = 4 + corpo.len() + pad4(corpo.len());
    let mut b = Vec::with_capacity(total);
    b.push(opcode);
    b.push(dado);
    b.extend_from_slice(&((total / 4) as u16).to_le_bytes());
    b.extend_from_slice(corpo);
    b.resize(total, 0);
    b
}

fn corpo(campos: &[u32]) -> Vec<u8> {
    campos.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// `CreateWindow` `InputOutput` com fundo preto e a máscara de eventos dada.
pub(crate) fn create_window(
    janela: u32,
    pai: u32,
    largura: u16,
    altura: u16,
    profundidade: u8,
    visual: u32,
    eventos: u32,
) -> Vec<u8> {
    const BACK_PIXEL: u32 = 1 << 1;
    const EVENT_MASK: u32 = 1 << 11;
    let mut c = corpo(&[janela, pai]);
    for v in [0u16, 0, largura, altura, 0, 1] {
        c.extend_from_slice(&v.to_le_bytes());
    }
    c.extend(corpo(&[visual, BACK_PIXEL | EVENT_MASK, 0, eventos]));
    requisicao(CREATE_WINDOW, profundidade, &c)
}

/// `ChangeProperty` no modo Replace. `formato` é 8 ou 32 bits por item.
pub(crate) fn change_property(
    janela: u32,
    propriedade: u32,
    tipo: u32,
    formato: u8,
    dados: &[u8],
) -> Vec<u8> {
    let itens = dados.len() / (formato as usize / 8);
    let mut c = corpo(&[janela, propriedade, tipo]);
    c.extend_from_slice(&[formato, 0, 0, 0]);
    c.extend_from_slice(&(itens as u32).to_le_bytes());
    c.extend_from_slice(dados);
    requisicao(CHANGE_PROPERTY, 0, &c)
}

/// `WM_NORMAL_HINTS` com tamanho mínimo igual ao máximo: janela fixa, como a Win32,
/// para o gerenciador de janelas não esticar o quadro.
pub(crate) fn hints_de_tamanho_fixo(largura: u16, altura: u16) -> Vec<u8> {
    const P_MIN_SIZE: u32 = 1 << 4;
    const P_MAX_SIZE: u32 = 1 << 5;
    let mut campos = [0u32; 18];
    campos[0] = P_MIN_SIZE | P_MAX_SIZE;
    campos[5] = largura.into();
    campos[6] = altura.into();
    campos[7] = largura.into();
    campos[8] = altura.into();
    corpo(&campos)
}

pub(crate) fn um_recurso(opcode: u8, id: u32) -> Vec<u8> {
    requisicao(opcode, 0, &id.to_le_bytes())
}

pub(crate) fn intern_atom(nome: &str, so_se_existir: bool) -> Vec<u8> {
    let mut c = (nome.len() as u16).to_le_bytes().to_vec();
    c.extend_from_slice(&[0, 0]);
    c.extend_from_slice(nome.as_bytes());
    requisicao(INTERN_ATOM, u8::from(so_se_existir), &c)
}

/// `CreateGC` sem atributos: os padrões (função Copy, todos os planos) bastam para
/// `PutImage`.
pub(crate) fn create_gc(gc: u32, drawable: u32) -> Vec<u8> {
    requisicao(CREATE_GC, 0, &corpo(&[gc, drawable, 0]))
}

pub(crate) fn get_keyboard_mapping(primeiro: u8, quantidade: u8) -> Vec<u8> {
    requisicao(GET_KEYBOARD_MAPPING, 0, &[primeiro, quantidade, 0, 0])
}

pub(crate) fn get_input_focus() -> Vec<u8> {
    requisicao(GET_INPUT_FOCUS, 0, &[])
}

/// `PutImage` `ZPixmap` de `linhas` linhas a partir de `y`.
pub(crate) fn put_image(
    drawable: u32,
    gc: u32,
    largura: u16,
    y: u16,
    linhas: u16,
    profundidade: u8,
    pixels: &[u8],
) -> Vec<u8> {
    const Z_PIXMAP: u8 = 2;
    let mut c = corpo(&[drawable, gc]);
    for v in [largura, linhas, 0, y] {
        c.extend_from_slice(&v.to_le_bytes());
    }
    c.extend_from_slice(&[0, profundidade, 0, 0]);
    c.extend_from_slice(pixels);
    requisicao(PUT_IMAGE, Z_PIXMAP, &c)
}

/// Quantas linhas de `largura` pixels de 32 bits cabem numa `PutImage`.
///
/// O servidor fecha a conexão diante de requisição maior que o máximo anunciado
/// (`BadLength`). Um quadro de 640×480 tem 1,2 MB e o máximo sem BIG-REQUESTS é
/// 256 KiB, por isso a imagem vai em faixas.
pub(crate) fn linhas_por_faixa(requisicao_maxima: u16, largura: u32) -> Result<u32> {
    const CABECALHO_PUT_IMAGE: u32 = 24;
    let disponivel = (u32::from(requisicao_maxima) * 4).saturating_sub(CABECALHO_PUT_IMAGE);
    let linhas = disponivel / (largura * 4);
    if linhas == 0 {
        return Err(Error::format(format!(
            "linha de {largura} pixels não cabe numa requisição X de {} bytes",
            u32::from(requisicao_maxima) * 4
        )));
    }
    Ok(linhas)
}

/// Converte `B,G,R,A` do guest para o pixel de 32 bits do servidor.
///
/// Em `LSBFirst` o pixel `0x00RRGGBB` fica em memória como `B,G,R,X`, a mesma ordem do
/// guest: cópia direta. Em `MSBFirst` fica `X,R,G,B`.
pub(crate) fn converter_pixels(origem: &[u8], destino: &mut Vec<u8>, lsb: bool) {
    destino.clear();
    if lsb {
        destino.extend_from_slice(origem);
    } else {
        for p in origem.as_chunks::<4>().0 {
            destino.extend_from_slice(&[0, p[2], p[1], p[0]]);
        }
    }
}

// Requisições usadas só pela ferramenta de verificação (outra conexão, Fase 8.2).

pub(crate) fn query_tree(janela: u32) -> Vec<u8> {
    um_recurso(QUERY_TREE, janela)
}

pub(crate) fn get_property(janela: u32, propriedade: u32, tipo: u32, palavras: u32) -> Vec<u8> {
    requisicao(
        GET_PROPERTY,
        0,
        &corpo(&[janela, propriedade, tipo, 0, palavras]),
    )
}

/// `GetImage` `ZPixmap` de todos os planos, `altura` linhas a partir de `y`.
pub(crate) fn get_image(drawable: u32, y: u16, largura: u16, altura: u16) -> Vec<u8> {
    let mut c = drawable.to_le_bytes().to_vec();
    for v in [0u16, y, largura, altura] {
        c.extend_from_slice(&v.to_le_bytes());
    }
    c.extend_from_slice(&u32::MAX.to_le_bytes());
    requisicao(GET_IMAGE, 2, &c)
}

pub(crate) fn send_event(destino: u32, mascara: u32, evento: &[u8; 32]) -> Vec<u8> {
    let mut c = corpo(&[destino, mascara]);
    c.extend_from_slice(evento);
    requisicao(SEND_EVENT, 0, &c)
}

/// `ClientMessage` `WM_PROTOCOLS/WM_DELETE_WINDOW`: o que o gerenciador de janelas manda
/// quando o usuário clica em fechar.
pub(crate) fn evento_fechar(janela: u32, wm_protocols: u32, wm_delete: u32) -> [u8; 32] {
    let mut e = [0u8; 32];
    e[0] = CLIENT_MESSAGE;
    e[1] = 32;
    e[4..8].copy_from_slice(&janela.to_le_bytes());
    e[8..12].copy_from_slice(&wm_protocols.to_le_bytes());
    e[12..16].copy_from_slice(&wm_delete.to_le_bytes());
    e
}

/// `KeyPress` ou `KeyRelease` sintético com `tempo` informado.
pub(crate) fn evento_tecla(tipo: u8, keycode: u8, tempo: u32, raiz: u32, janela: u32) -> [u8; 32] {
    let mut e = [0u8; 32];
    e[0] = tipo;
    e[1] = keycode;
    e[4..8].copy_from_slice(&tempo.to_le_bytes());
    e[8..12].copy_from_slice(&raiz.to_le_bytes());
    e[12..16].copy_from_slice(&janela.to_le_bytes());
    e[30] = 1; // same-screen
    e
}

// ─── Pacotes recebidos ──────────────────────────────────────────────────────────

/// Um pacote completo vindo do servidor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Pacote {
    Resposta {
        sequencia: u16,
        dados: Vec<u8>,
    },
    Erro {
        codigo: u8,
        sequencia: u16,
        valor: u32,
        opcode: u8,
    },
    Evento([u8; 32]),
}

impl Pacote {
    /// Texto do erro com o nome da requisição, para diagnóstico.
    pub(crate) fn descrever_erro(codigo: u8, valor: u32, opcode: u8) -> String {
        let nome = match codigo {
            1 => "BadRequest",
            2 => "BadValue",
            3 => "BadWindow",
            8 => "BadMatch",
            9 => "BadDrawable",
            11 => "BadAlloc",
            13 => "BadGC",
            14 => "BadIDChoice",
            16 => "BadLength",
            _ => "erro",
        };
        format!(
            "servidor X devolveu {nome} (código {codigo}) na requisição {opcode}, valor 0x{valor:X}"
        )
    }
}

/// Retira do início de `entrada` o primeiro pacote completo, se houver.
pub(crate) fn extrair_pacote(entrada: &mut Vec<u8>) -> Option<Pacote> {
    if entrada.len() < 32 {
        return None;
    }
    let tamanho = if entrada[0] == 1 {
        32 + 4 * u32_le(entrada, 4) as usize
    } else {
        32
    };
    if entrada.len() < tamanho {
        return None;
    }
    let bytes: Vec<u8> = entrada.drain(..tamanho).collect();
    let sequencia = u16_le(&bytes, 2);
    Some(match bytes[0] {
        0 => Pacote::Erro {
            codigo: bytes[1],
            sequencia,
            valor: u32_le(&bytes, 4),
            opcode: bytes[10],
        },
        1 => Pacote::Resposta {
            sequencia,
            dados: bytes,
        },
        _ => {
            let mut e = [0u8; 32];
            e.copy_from_slice(&bytes);
            Pacote::Evento(e)
        }
    })
}

/// Átomo da resposta de `InternAtom`.
pub(crate) fn atomo_da_resposta(dados: &[u8]) -> u32 {
    u32_le(dados, 8)
}

/// Primeiro keysym de cada keycode, a partir da resposta de `GetKeyboardMapping`.
pub(crate) fn keysyms_da_resposta(dados: &[u8], quantidade: usize) -> Result<Vec<u32>> {
    let por_keycode = dados[1] as usize;
    if por_keycode == 0 {
        return Ok(vec![0; quantidade]);
    }
    (0..quantidade)
        .map(|k| {
            let i = 32 + 4 * k * por_keycode;
            dados
                .get(i..i + 4)
                .map(|_| u32_le(dados, i))
                .ok_or_else(|| curto("mapa de teclado"))
        })
        .collect()
}

// ─── Interpretação de eventos ───────────────────────────────────────────────────

/// Traduz eventos X da janela do apresentador para [`Evento`], com a mesma política do
/// Win32: fechar é prioritário e permanente, atalhos seguem em ordem numa fila
/// limitada e autorrepetição é ignorada.
#[derive(Debug)]
pub(crate) struct Interpretador {
    janela: u32,
    wm_protocols: u32,
    wm_delete: u32,
    keycode_min: u8,
    keysyms: Vec<u32>,
    pressionadas: [bool; 256],
    ultima_soltura: Option<(u8, u32)>,
    fechar: bool,
    repintar: bool,
    comandos: VecDeque<Evento>,
}

impl Interpretador {
    pub(crate) fn novo(
        janela: u32,
        wm_protocols: u32,
        wm_delete: u32,
        keycode_min: u8,
        keysyms: Vec<u32>,
    ) -> Self {
        Self {
            janela,
            wm_protocols,
            wm_delete,
            keycode_min,
            keysyms,
            pressionadas: [false; 256],
            ultima_soltura: None,
            fechar: false,
            repintar: false,
            comandos: VecDeque::new(),
        }
    }

    fn keysym(&self, keycode: u8) -> u32 {
        keycode
            .checked_sub(self.keycode_min)
            .and_then(|i| self.keysyms.get(i as usize))
            .copied()
            .unwrap_or(0)
    }

    /// Consome um evento. O bit 0x80 (evento enviado por `SendEvent`) é ignorado: o
    /// gerenciador de janelas entrega `WM_DELETE_WINDOW` exatamente assim.
    pub(crate) fn consumir(&mut self, e: &[u8; 32]) {
        let alvo = |i: usize| u32_le(e, i) == self.janela;
        match e[0] & 0x7F {
            KEY_PRESS if alvo(12) => {
                let (keycode, tempo) = (e[1], u32_le(e, 4));
                let repeticao = self.pressionadas[keycode as usize]
                    || self.ultima_soltura == Some((keycode, tempo));
                self.pressionadas[keycode as usize] = true;
                let comando = match self.keysym(keycode) {
                    XK_SPACE => Some(Evento::PausarRetomar),
                    XK_F5 => Some(Evento::SalvarEstado),
                    XK_F9 => Some(Evento::RestaurarEstado),
                    _ => None,
                };
                if let Some(comando) = comando
                    && !repeticao
                    && self.comandos.len() < FILA_MAXIMA
                {
                    self.comandos.push_back(comando);
                }
            }
            KEY_RELEASE if alvo(12) => {
                self.pressionadas[e[1] as usize] = false;
                self.ultima_soltura = Some((e[1], u32_le(e, 4)));
            }
            // count == 0: último Expose da sequência; um repinte cobre todos.
            EXPOSE if alvo(4) && u16_le(e, 16) == 0 => self.repintar = true,
            DESTROY_NOTIFY if alvo(8) => self.fechar = true,
            CLIENT_MESSAGE
                if alvo(4)
                    && e[1] == 32
                    && u32_le(e, 8) == self.wm_protocols
                    && u32_le(e, 12) == self.wm_delete =>
            {
                self.fechar = true;
            }
            _ => {}
        }
    }

    /// `true` uma vez depois de um Expose: o conteúdo precisa ser reenviado.
    pub(crate) fn tomar_repintar(&mut self) -> bool {
        std::mem::take(&mut self.repintar)
    }

    /// Próximo evento para o laço de emulação.
    pub(crate) fn proximo(&mut self) -> Evento {
        if self.fechar {
            Evento::Fechar
        } else {
            self.comandos.pop_front().unwrap_or(Evento::Nenhum)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entrada_auth(familia: u16, numero: &str, nome: &[u8], dados: &[u8]) -> Vec<u8> {
        let mut b = familia.to_be_bytes().to_vec();
        for campo in [b"host".as_slice(), numero.as_bytes(), nome, dados] {
            b.extend_from_slice(&(campo.len() as u16).to_be_bytes());
            b.extend_from_slice(campo);
        }
        b
    }

    #[test]
    fn display_local_aceita_formas_usuais_e_recusa_remoto() {
        assert_eq!(numero_do_display(":0").unwrap(), 0);
        assert_eq!(numero_do_display(":1.0").unwrap(), 1);
        assert_eq!(numero_do_display("unix:12").unwrap(), 12);
        assert!(
            numero_do_display("localhost:10.0")
                .unwrap_err()
                .to_string()
                .contains("remoto")
        );
        assert!(numero_do_display("0").is_err());
        assert!(numero_do_display(":x").is_err());
    }

    #[test]
    fn xauthority_le_entradas_e_escolhe_cookie_local_do_display() {
        let mut arquivo = entrada_auth(0, "0", MIT_MAGIC_COOKIE, &[9; 16]);
        arquivo.extend(entrada_auth(FAMILIA_LOCAL, "1", MIT_MAGIC_COOKIE, &[1; 16]));
        arquivo.extend(entrada_auth(
            FAMILIA_WILD,
            "0",
            b"XDM-AUTHORIZATION-1",
            &[2; 8],
        ));
        arquivo.extend(entrada_auth(FAMILIA_WILD, "0", MIT_MAGIC_COOKIE, &[3; 16]));
        let entradas = ler_xauthority(&arquivo).unwrap();
        assert_eq!(entradas.len(), 4);
        assert_eq!(entradas[1].endereco, b"host");
        // Wild/0 vence a entrada de família Internet com o mesmo número.
        assert_eq!(escolher_cookie(&entradas, 0), Some(vec![3; 16]));
        assert_eq!(escolher_cookie(&entradas, 1), Some(vec![1; 16]));
        assert_eq!(escolher_cookie(&entradas, 7), None);
        // Sem Local/Wild, aceita outra família com o mesmo número.
        assert_eq!(escolher_cookie(&entradas[..1], 0), Some(vec![9; 16]));
        // Número vazio vale para qualquer display.
        let wild =
            ler_xauthority(&entrada_auth(FAMILIA_WILD, "", MIT_MAGIC_COOKIE, &[4; 4])).unwrap();
        assert_eq!(escolher_cookie(&wild, 5), Some(vec![4; 4]));
    }

    #[test]
    fn xauthority_truncado_e_erro_nao_lista_parcial() {
        let arquivo = entrada_auth(FAMILIA_LOCAL, "0", MIT_MAGIC_COOKIE, &[1; 16]);
        for corte in [1, 3, 10, arquivo.len() - 1] {
            assert!(
                ler_xauthority(&arquivo[..corte]).is_err(),
                "aceitou corte {corte}"
            );
        }
        assert_eq!(ler_xauthority(&[]).unwrap(), []);
    }

    #[test]
    fn pedido_de_setup_alinha_nome_e_dados() {
        let b = pedido_de_setup(MIT_MAGIC_COOKIE, &[0xAB; 16]);
        assert_eq!(&b[..12], &[b'l', 0, 11, 0, 0, 0, 18, 0, 16, 0, 0, 0]);
        assert_eq!(&b[12..30], MIT_MAGIC_COOKIE);
        assert_eq!(&b[30..32], &[0, 0]);
        assert_eq!(&b[32..48], &[0xAB; 16]);
        assert_eq!(b.len(), 48);
        assert_eq!(pedido_de_setup(&[], &[]).len(), 12);
    }

    /// Setup mínimo de uma tela com o visual pedido.
    fn setup_sintetico(profundidade: u8, mascaras: [u32; 3], bpp: u8, lsb: bool) -> Vec<u8> {
        let vendor = b"Obsidian";
        let mut b = vec![1, 0, 11, 0, 0, 0, 0, 0];
        let mut d = Vec::new();
        for v in [0u32, 0x0040_0000, 0x001F_FFFF, 0] {
            d.extend_from_slice(&v.to_le_bytes());
        }
        d.extend_from_slice(&(vendor.len() as u16).to_le_bytes());
        d.extend_from_slice(&0xFFFFu16.to_le_bytes());
        d.extend_from_slice(&[1, 1, u8::from(!lsb), 0, 32, 32, 8, 255, 0, 0, 0, 0]);
        d.extend_from_slice(vendor);
        d.extend_from_slice(&[24, bpp, 32, 0, 0, 0, 0, 0]);
        let mut tela = Vec::new();
        for v in [0x1EBu32, 0x20, 0xFF_FFFF, 0, 0] {
            tela.extend_from_slice(&v.to_le_bytes());
        }
        for v in [1920u16, 1080, 500, 300, 1, 1] {
            tela.extend_from_slice(&v.to_le_bytes());
        }
        tela.extend_from_slice(&0x21u32.to_le_bytes());
        tela.extend_from_slice(&[0, 0, profundidade, 1]);
        tela.extend_from_slice(&[profundidade, 0, 1, 0, 0, 0, 0, 0]);
        tela.extend_from_slice(&0x21u32.to_le_bytes());
        tela.extend_from_slice(&[TRUE_COLOR, 8, 0, 1]);
        for m in mascaras {
            tela.extend_from_slice(&m.to_le_bytes());
        }
        tela.extend_from_slice(&[0; 4]);
        d.extend(tela);
        b[6..8].copy_from_slice(&((d.len() / 4) as u16).to_le_bytes());
        b.extend(d);
        b
    }

    #[test]
    fn setup_aceito_extrai_ids_tela_e_ordem_de_imagem() {
        let b = setup_sintetico(24, [0xFF_0000, 0xFF00, 0xFF], 32, true);
        let mut cab = [0u8; 8];
        cab.copy_from_slice(&b[..8]);
        assert_eq!(tamanho_do_setup(cab), b.len());
        let s = ler_setup(&b, 0).unwrap();
        assert_eq!(
            s,
            Setup {
                id_base: 0x0040_0000,
                id_mascara: 0x001F_FFFF,
                requisicao_maxima: 0xFFFF,
                imagem_lsb: true,
                keycode_min: 8,
                keycode_max: 255,
                raiz: 0x1EB,
                visual: 0x21,
                profundidade: 24,
            }
        );
        assert_eq!(id_de_recurso(&s, 1), 0x0040_0001);
        assert!(
            !ler_setup(
                &setup_sintetico(24, [0xFF_0000, 0xFF00, 0xFF], 32, false),
                0
            )
            .unwrap()
            .imagem_lsb
        );
        assert!(ler_setup(&b, 1).unwrap_err().to_string().contains("tela 1"));
    }

    #[test]
    fn setup_recusa_visual_que_a_copia_direta_nao_suporta() {
        for b in [
            setup_sintetico(16, [0xF800, 0x7E0, 0x1F], 16, true),
            setup_sintetico(24, [0xFF, 0xFF00, 0xFF_0000], 32, true),
            setup_sintetico(24, [0xFF_0000, 0xFF00, 0xFF], 24, true),
        ] {
            let erro = ler_setup(&b, 0).unwrap_err().to_string();
            assert!(erro.contains("TrueColor 24"), "{erro}");
        }
    }

    #[test]
    fn setup_recusado_ou_truncado_traz_o_motivo() {
        let mut recusa = vec![0, 13, 11, 0, 0, 0, 4, 0];
        recusa.extend_from_slice(b"No protocol\0\0\0\0\0");
        let erro = ler_setup(&recusa, 0).unwrap_err().to_string();
        assert!(
            erro.contains("recusou") && erro.contains("No protocol"),
            "{erro}"
        );
        let mut auth = vec![2, 0, 0, 0, 0, 0, 2, 0];
        auth.extend_from_slice(b"Kerb\0\0\0\0");
        assert!(
            ler_setup(&auth, 0)
                .unwrap_err()
                .to_string()
                .contains("Kerb")
        );
        let b = setup_sintetico(24, [0xFF_0000, 0xFF00, 0xFF], 32, true);
        for corte in [4, 39, 60, b.len() - 1] {
            assert!(ler_setup(&b[..corte], 0).is_err(), "aceitou corte {corte}");
        }
    }

    #[test]
    fn requisicoes_tem_comprimento_em_palavras_e_campos_no_lugar() {
        let janela = create_window(0x40_0001, 0x1EB, 640, 480, 24, 0x21, 0x2_8003);
        assert_eq!((janela[0], janela[1]), (CREATE_WINDOW, 24));
        assert_eq!(u16_le(&janela, 2) as usize * 4, janela.len());
        assert_eq!(janela.len(), 40);
        assert_eq!(u32_le(&janela, 4), 0x40_0001);
        assert_eq!((u16_le(&janela, 16), u16_le(&janela, 18)), (640, 480));
        assert_eq!(u16_le(&janela, 22), 1, "classe InputOutput");
        assert_eq!(
            u32_le(&janela, 28),
            0x802,
            "máscara: back-pixel envio event-mask"
        );
        assert_eq!(u32_le(&janela, 36), 0x2_8003);

        let propriedade = change_property(7, ATOM_WM_NAME, ATOM_STRING, 8, b"abc");
        assert_eq!(propriedade.len(), 28);
        assert_eq!(u16_le(&propriedade, 2), 7);
        assert_eq!((propriedade[16], u32_le(&propriedade, 20)), (8, 3));
        assert_eq!(&propriedade[24..28], b"abc\0");
        let hints = change_property(
            7,
            ATOM_WM_NORMAL_HINTS,
            ATOM_WM_SIZE_HINTS,
            32,
            &hints_de_tamanho_fixo(640, 480),
        );
        assert_eq!(u32_le(&hints, 20), 18, "WM_SIZE_HINTS tem 18 palavras");
        assert_eq!(u32_le(&hints, 24), 0x30);
        assert_eq!(
            (
                u32_le(&hints, 44),
                u32_le(&hints, 48),
                u32_le(&hints, 52),
                u32_le(&hints, 56)
            ),
            (640, 480, 640, 480)
        );

        let atomo = intern_atom("WM_DELETE_WINDOW", false);
        assert_eq!(
            (atomo[0], atomo[1], u16_le(&atomo, 4)),
            (INTERN_ATOM, 0, 16)
        );
        assert_eq!(atomo.len(), 24);
        assert_eq!(get_input_focus(), vec![GET_INPUT_FOCUS, 0, 1, 0]);
        assert_eq!(
            get_keyboard_mapping(8, 248),
            vec![GET_KEYBOARD_MAPPING, 0, 2, 0, 8, 248, 0, 0]
        );
        assert_eq!(
            um_recurso(MAP_WINDOW, 9),
            vec![MAP_WINDOW, 0, 2, 0, 9, 0, 0, 0]
        );
        assert_eq!(create_gc(2, 1).len(), 16);

        let imagem = get_image(9, 102, 640, 50);
        assert_eq!(
            (imagem[0], imagem[1], u16_le(&imagem, 2)),
            (GET_IMAGE, 2, 5)
        );
        assert_eq!(
            (
                u16_le(&imagem, 10),
                u16_le(&imagem, 12),
                u16_le(&imagem, 14)
            ),
            (102, 640, 50)
        );
        assert_eq!(u32_le(&imagem, 16), u32::MAX);
        let leitura = get_property(9, ATOM_WM_CLASS, ATOM_STRING, 64);
        assert_eq!(
            (
                leitura[0],
                u16_le(&leitura, 2),
                u32_le(&leitura, 8),
                u32_le(&leitura, 20)
            ),
            (GET_PROPERTY, 6, ATOM_WM_CLASS, 64)
        );
        assert_eq!(query_tree(9), vec![QUERY_TREE, 0, 2, 0, 9, 0, 0, 0]);
        let envio = send_event(9, 0, &evento_fechar(9, 100, 101));
        assert_eq!(
            (envio[0], u16_le(&envio, 2), envio.len()),
            (SEND_EVENT, 11, 44)
        );
        assert_eq!(&envio[12..16], &[CLIENT_MESSAGE, 32, 0, 0]);
        assert_eq!(u32_le(&envio, 24), 101);
    }

    #[test]
    fn put_image_em_faixas_respeita_o_maximo_do_servidor() {
        // 0xFFFF palavras = 262140 bytes; linha de 640 px = 2560 bytes.
        assert_eq!(linhas_por_faixa(0xFFFF, 640).unwrap(), 102);
        assert!(linhas_por_faixa(100, 640).is_err());
        let linhas = linhas_por_faixa(0xFFFF, 640).unwrap();
        let r = put_image(
            5,
            6,
            640,
            102,
            linhas as u16,
            24,
            &vec![0; (640 * linhas * 4) as usize],
        );
        assert!(r.len() <= 0xFFFF * 4);
        assert_eq!(u16_le(&r, 2) as usize * 4, r.len());
        assert_eq!((r[0], r[1]), (PUT_IMAGE, 2));
        assert_eq!(
            (u16_le(&r, 12), u16_le(&r, 14), u16_le(&r, 18)),
            (640, 102, 102)
        );
        assert_eq!(r[21], 24);
    }

    #[test]
    fn conversao_de_pixels_preserva_bgra_em_lsb_e_reordena_em_msb() {
        let origem = [1, 2, 3, 4, 5, 6, 7, 8];
        let mut destino = Vec::new();
        converter_pixels(&origem, &mut destino, true);
        assert_eq!(destino, origem);
        converter_pixels(&origem, &mut destino, false);
        assert_eq!(destino, [0, 3, 2, 1, 0, 7, 6, 5]);
    }

    #[test]
    fn pacotes_sao_extraidos_inteiros_na_ordem() {
        let mut entrada = Vec::new();
        let mut resposta = vec![1, 0, 5, 0, 2, 0, 0, 0];
        resposta.resize(40, 0xAA);
        entrada.extend_from_slice(&resposta);
        let mut erro = [0u8; 32];
        erro[..11].copy_from_slice(&[0, 16, 6, 0, 0x34, 0x12, 0, 0, 0, 0, PUT_IMAGE]);
        entrada.extend_from_slice(&erro);
        let evento = evento_fechar(1, 2, 3);
        entrada.extend_from_slice(&evento[..20]);

        assert_eq!(
            extrair_pacote(&mut entrada),
            Some(Pacote::Resposta {
                sequencia: 5,
                dados: resposta
            })
        );
        assert_eq!(
            extrair_pacote(&mut entrada),
            Some(Pacote::Erro {
                codigo: 16,
                sequencia: 6,
                valor: 0x1234,
                opcode: PUT_IMAGE
            })
        );
        assert_eq!(
            extrair_pacote(&mut entrada),
            None,
            "evento incompleto fica no buffer"
        );
        entrada.extend_from_slice(&evento[20..]);
        assert_eq!(extrair_pacote(&mut entrada), Some(Pacote::Evento(evento)));
        assert_eq!(entrada, []);
        assert!(Pacote::descrever_erro(16, 0x1234, PUT_IMAGE).contains("BadLength"));
    }

    #[test]
    fn mapa_de_teclado_pega_o_primeiro_keysym_de_cada_keycode() {
        let mut r = vec![1, 2, 0, 0, 6, 0, 0, 0];
        r.resize(32, 0);
        for k in [XK_SPACE, 0x53, XK_F5, 0, XK_F9, 0xFFC7] {
            r.extend_from_slice(&k.to_le_bytes());
        }
        assert_eq!(
            keysyms_da_resposta(&r, 3).unwrap(),
            vec![XK_SPACE, XK_F5, XK_F9]
        );
        assert!(keysyms_da_resposta(&r, 4).is_err());
    }

    const JANELA: u32 = 0x40_0001;

    fn interpretador() -> Interpretador {
        // keycode 8 → espaço, 9 → F5, 10 → F9, 11 → 'a'.
        Interpretador::novo(JANELA, 100, 101, 8, vec![XK_SPACE, XK_F5, XK_F9, 0x61])
    }

    fn tecla(tipo: u8, keycode: u8, tempo: u32) -> [u8; 32] {
        evento_tecla(tipo, keycode, tempo, 1, JANELA)
    }

    #[test]
    fn atalhos_preservam_ordem_ignoram_autorepeticao_e_fechar_tem_prioridade() {
        let mut i = interpretador();
        for e in [
            tecla(KEY_PRESS, 8, 10),
            // Autorrepetição clássica: soltura e pressão com o mesmo instante.
            tecla(KEY_RELEASE, 8, 20),
            tecla(KEY_PRESS, 8, 20),
            // Autorrepetição "detectável": pressões sem soltura.
            tecla(KEY_PRESS, 8, 30),
            tecla(KEY_RELEASE, 8, 40),
            tecla(KEY_PRESS, 11, 50),
            tecla(KEY_PRESS, 9, 60),
            tecla(KEY_PRESS, 10, 70),
        ] {
            i.consumir(&e);
        }
        for esperado in [
            Evento::PausarRetomar,
            Evento::SalvarEstado,
            Evento::RestaurarEstado,
            Evento::Nenhum,
        ] {
            assert_eq!(i.proximo(), esperado);
        }
        i.consumir(&tecla(KEY_RELEASE, 9, 61));
        i.consumir(&tecla(KEY_PRESS, 9, 80));
        // Enviado por SendEvent (bit 0x80), como o gerenciador de janelas faz.
        let mut fechar = evento_fechar(JANELA, 100, 101);
        fechar[0] |= 0x80;
        i.consumir(&fechar);
        assert_eq!(i.proximo(), Evento::Fechar);
        assert_eq!(i.proximo(), Evento::Fechar);
    }

    #[test]
    fn eventos_de_outra_janela_ou_outro_protocolo_sao_ignorados() {
        let mut i = interpretador();
        i.consumir(&evento_tecla(KEY_PRESS, 8, 1, 1, JANELA + 1));
        i.consumir(&evento_fechar(JANELA + 1, 100, 101));
        i.consumir(&evento_fechar(JANELA, 100, 999));
        i.consumir(&evento_fechar(JANELA, 999, 101));
        assert_eq!(i.proximo(), Evento::Nenhum);
        let mut destruida = [0u8; 32];
        destruida[0] = DESTROY_NOTIFY;
        destruida[8..12].copy_from_slice(&JANELA.to_le_bytes());
        i.consumir(&destruida);
        assert_eq!(i.proximo(), Evento::Fechar);
    }

    #[test]
    fn expose_pede_um_repinte_so_no_ultimo_da_sequencia() {
        let mut i = interpretador();
        let mut e = [0u8; 32];
        e[0] = EXPOSE;
        e[4..8].copy_from_slice(&JANELA.to_le_bytes());
        e[16] = 2;
        i.consumir(&e);
        assert!(!i.tomar_repintar());
        e[16] = 0;
        i.consumir(&e);
        assert!(i.tomar_repintar());
        assert!(!i.tomar_repintar(), "repinte é consumido uma vez");
    }

    #[test]
    fn fila_de_atalhos_e_limitada() {
        let mut i = interpretador();
        for t in 0..100u32 {
            i.consumir(&tecla(KEY_PRESS, 9, t * 2));
            i.consumir(&tecla(KEY_RELEASE, 9, t * 2 + 1));
        }
        let mut n = 0;
        while i.proximo() == Evento::SalvarEstado {
            n += 1;
        }
        assert_eq!(n, FILA_MAXIMA);
    }
}
