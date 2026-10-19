//! Construtores de arquivos de teste.
//!
//! O ROADMAP §14 proíbe material proprietário no repositório, então as fixtures não
//! são dumps: são **geradas por código**. Isso tem outra vantagem prática — um teste
//! pode produzir exatamente o caso patológico que quer (seção apontando para fora do
//! arquivo, ciclo na árvore de diretórios, imagem truncada) sem precisar achar um
//! arquivo real que tenha aquele defeito.
//!
//! ## Limite honesto destas fixtures
//!
//! Construtor e parser escritos pela mesma pessoa concordando entre si **não provam
//! conformidade com o formato real** — provam consistência interna. A conformidade
//! precisa de verificação contra um arquivo de verdade (um XBE gerado pelo nxdk, ou um
//! disco que o usuário possua). Isso está registrado como item pendente em
//! `docs/compat/formatos.md`.
//!
//! Por isso as fixtures são deliberadamente "mínimas mas plausíveis": layout de
//! headers, ordem de campos e alinhamentos seguem a documentação pública, para que a
//! divergência, se houver, apareça como campo errado e não como estrutura inteira
//! errada.
//!
//! Este módulo é código de apoio a teste: aqui `panic` em índice inválido é
//! aceitável, porque os dados são nossos, não do usuário.

use crate::xbe::{
    self, Certificate, ENTRY_XOR_CHIHIRO, ENTRY_XOR_DEBUG, ENTRY_XOR_RETAIL, MediaFlags,
    RegionFlags, SectionFlags, THUNK_XOR_CHIHIRO, THUNK_XOR_DEBUG, THUNK_XOR_RETAIL, XbeKind,
};
use crate::xdvdfs::{ATTR_DIRECTORY, ATTR_NORMAL, MAGIC, SECTOR_SIZE, VOLUME_SECTOR};

// ─── Valores que os testes verificam ──────────────────────────────────────────────

/// Nome do título nas fixtures.
pub const TITULO: &str = "Obsidian Fixture";
/// Title ID das fixtures (formata como `OB-0001`).
pub const TITLE_ID: u32 = 0x4F42_0001;
/// Entry point das fixtures (início de `.text`).
pub const ENTRY_POINT: u32 = 0x0001_1000;
/// Endereço da thunk table nas fixtures (dentro de `.rdata`).
pub const KERNEL_THUNK: u32 = 0x0001_2000;
/// Conteúdo da seção `.text`.
pub const TEXT_DATA: &[u8] = b"\xCC\xC3obsidian\x90\x90\x90\x90\x90\x90";
/// Tamanho da seção `.rdata`.
pub const RDATA_LEN: usize = 0x40;
/// Conteúdo de `media/readme.txt` nas imagens.
pub const README: &[u8] = b"fixture gerada por codigo; nada proprietario aqui\n";

// ─── Layout do XBE de fixture ─────────────────────────────────────────────────────

const BASE: u32 = xbe::DEFAULT_BASE_ADDRESS;
const OFF_CERT: usize = 0x0180;
const OFF_SECTION_HEADERS: usize = 0x0300;
const OFF_NOME_TEXT: usize = 0x0380;
const OFF_NOME_RDATA: usize = 0x0390;
const OFF_LIBS: usize = 0x03A0;
const OFF_TLS: usize = 0x03C0;
const OFF_LOGO: usize = 0x0400;
const OFF_DEBUG_PATH: usize = 0x0900;
const SIZE_OF_HEADERS: u32 = 0x1000;
const OFF_TEXT_DATA: usize = 0x1000;
const OFF_RDATA_DATA: usize = 0x2000;
const TAMANHO_ARQUIVO: usize = OFF_RDATA_DATA + RDATA_LEN;
const CERT_SIZE: u32 = 0xD0;

/// XBE retail mínimo, válido e completo o suficiente para o loader da Fase 4.
pub fn xbe_minimo() -> Vec<u8> {
    xbe_com_kind(XbeKind::Retail)
}

/// XBE de teste executável, com até uma página de código fornecido pelo teste.
/// Sem TLS ou importações do kernel; `.rdata` é gravável e começa zerada.
/// Não é um programa nxdk nem prova de compatibilidade com jogos comerciais.
pub fn xbe_com_codigo(code: &[u8]) -> xbox_core::Result<Vec<u8>> {
    if code.is_empty() || code.len() > OFF_RDATA_DATA - OFF_TEXT_DATA {
        return Err(xbox_core::Error::format(
            "código da fixture deve ter de 1 a 4096 bytes",
        ));
    }
    let mut bytes = xbe_minimo();
    bytes[OFF_TEXT_DATA..].fill(0);
    bytes[OFF_TEXT_DATA..OFF_TEXT_DATA + code.len()].copy_from_slice(code);
    put_u32(&mut bytes, OFF_SECTION_HEADERS + 0x08, code.len() as u32);
    put_u32(&mut bytes, OFF_SECTION_HEADERS + 0x10, code.len() as u32);
    put_u32(
        &mut bytes,
        OFF_SECTION_HEADERS + 0x38,
        SectionFlags::PRELOAD | SectionFlags::WRITABLE,
    );
    put_u32(&mut bytes, 0x12C, 0); // sem TLS
    for offset in [0x160, 0x164, 0x168, 0x16C] {
        put_u32(&mut bytes, offset, 0); // sem bibliotecas declaradas
    }
    Ok(bytes)
}

/// Como [`xbe_minimo`], mas ofuscado com as chaves do tipo de build pedido.
pub fn xbe_com_kind(kind: XbeKind) -> Vec<u8> {
    let (chave_entry, chave_thunk) = match kind {
        XbeKind::Retail => (ENTRY_XOR_RETAIL, THUNK_XOR_RETAIL),
        XbeKind::Debug => (ENTRY_XOR_DEBUG, THUNK_XOR_DEBUG),
        XbeKind::Chihiro => (ENTRY_XOR_CHIHIRO, THUNK_XOR_CHIHIRO),
    };

    let mut f = vec![0u8; TAMANHO_ARQUIVO];

    // ── header ──
    f[0..4].copy_from_slice(b"XBEH");
    // 0x004..0x104: assinatura digital; fica zerada (não a validamos).
    put_u32(&mut f, 0x104, BASE);
    put_u32(&mut f, 0x108, SIZE_OF_HEADERS);
    put_u32(&mut f, 0x10C, 0x3000); // size of image
    put_u32(&mut f, 0x110, 0x0178); // size of image header
    put_u32(&mut f, 0x114, 0x6700_0000); // timestamp
    put_u32(&mut f, 0x118, BASE + OFF_CERT as u32);
    put_u32(&mut f, 0x11C, 2); // section count
    put_u32(&mut f, 0x120, BASE + OFF_SECTION_HEADERS as u32);
    put_u32(&mut f, 0x124, 0x0000_0005); // init flags: mount-utility-drive | limit-64mb
    put_u32(&mut f, 0x128, ENTRY_POINT ^ chave_entry);
    put_u32(&mut f, 0x12C, BASE + OFF_TLS as u32);
    put_u32(&mut f, 0x130, 0x0001_0000); // stack commit
    put_u32(&mut f, 0x134, 0x0010_0000); // heap reserve
    put_u32(&mut f, 0x138, 0x0001_0000); // heap commit
    put_u32(&mut f, 0x13C, 0x0001_0000); // PE base
    put_u32(&mut f, 0x140, 0x3000); // PE size of image
    put_u32(&mut f, 0x144, 0); // PE checksum
    put_u32(&mut f, 0x148, 0x6700_0000); // PE timestamp
    put_u32(&mut f, 0x14C, BASE + OFF_DEBUG_PATH as u32);
    put_u32(&mut f, 0x150, BASE + OFF_DEBUG_PATH as u32);
    put_u32(&mut f, 0x154, 0);
    put_u32(&mut f, 0x158, KERNEL_THUNK ^ chave_thunk);
    put_u32(&mut f, 0x15C, 0); // imports não-kernel
    put_u32(&mut f, 0x160, 2); // library version count
    put_u32(&mut f, 0x164, BASE + OFF_LIBS as u32);
    put_u32(&mut f, 0x168, BASE + OFF_LIBS as u32); // kernel lib version
    put_u32(&mut f, 0x16C, BASE + OFF_LIBS as u32); // xapi lib version
    put_u32(&mut f, 0x170, BASE + OFF_LOGO as u32);

    // ── certificado ──
    put_u32(&mut f, OFF_CERT, CERT_SIZE);
    put_u32(&mut f, OFF_CERT + 0x04, 0x6700_0000);
    put_u32(&mut f, OFF_CERT + 0x08, TITLE_ID);
    put_utf16(&mut f, OFF_CERT + 0x0C, TITULO, 40);
    put_u32(
        &mut f,
        OFF_CERT + 0x9C,
        MediaFlags::HARDDISK | MediaFlags::DVD_X2,
    );
    put_u32(&mut f, OFF_CERT + 0xA0, RegionFlags::NORTH_AMERICA);
    put_u32(&mut f, OFF_CERT + 0xA4, 0xFFFF_FFFF); // ratings: sem classificação
    put_u32(&mut f, OFF_CERT + 0xA8, 0); // disco 1 de 1
    put_u32(&mut f, OFF_CERT + 0xAC, 1); // versão
    for i in 0..16 {
        f[OFF_CERT + 0xB0 + i] = 0xA0 + i as u8; // lan key
        f[OFF_CERT + 0xC0 + i] = 0xB0 + i as u8; // signature key
    }

    // ── headers de seção ──
    escrever_secao(
        &mut f,
        OFF_SECTION_HEADERS,
        SectionFlags::PRELOAD | SectionFlags::EXECUTABLE,
        BASE + OFF_TEXT_DATA as u32,
        TEXT_DATA.len() as u32,
        OFF_TEXT_DATA as u32,
        TEXT_DATA.len() as u32,
        BASE + OFF_NOME_TEXT as u32,
    );
    escrever_secao(
        &mut f,
        OFF_SECTION_HEADERS + 0x38,
        SectionFlags::PRELOAD,
        BASE + OFF_RDATA_DATA as u32,
        RDATA_LEN as u32,
        OFF_RDATA_DATA as u32,
        RDATA_LEN as u32,
        BASE + OFF_NOME_RDATA as u32,
    );
    put_bytes(&mut f, OFF_NOME_TEXT, b".text\0");
    put_bytes(&mut f, OFF_NOME_RDATA, b".rdata\0");

    // ── versões de biblioteca ──
    escrever_lib(&mut f, OFF_LIBS, b"XAPILIB", 1, 0, 5849);
    escrever_lib(&mut f, OFF_LIBS + 0x10, b"D3D8", 1, 0, 5849);

    // ── TLS ──
    put_u32(&mut f, OFF_TLS, BASE + OFF_RDATA_DATA as u32);
    put_u32(&mut f, OFF_TLS + 0x04, BASE + OFF_RDATA_DATA as u32 + 0x10);
    put_u32(&mut f, OFF_TLS + 0x08, BASE + OFF_RDATA_DATA as u32 + 0x20);
    put_u32(&mut f, OFF_TLS + 0x0C, 0);
    put_u32(&mut f, OFF_TLS + 0x10, 8);
    put_u32(&mut f, OFF_TLS + 0x14, 0);

    // ── logo ──
    let rle = xbe::codificar_logo(&logo_de_teste());
    assert!(
        OFF_LOGO + rle.len() < OFF_DEBUG_PATH,
        "logo de teste grande demais para o layout da fixture: {} bytes",
        rle.len()
    );
    put_bytes(&mut f, OFF_LOGO, &rle);
    put_u32(&mut f, 0x174, rle.len() as u32);

    // ── strings de debug e dados das seções ──
    put_bytes(&mut f, OFF_DEBUG_PATH, b"D:\\obsidian\\fixture.xbe\0");
    put_bytes(&mut f, OFF_TEXT_DATA, TEXT_DATA);
    for i in 0..RDATA_LEN {
        f[OFF_RDATA_DATA + i] = (i as u8).wrapping_mul(3);
    }

    f
}

/// XBE cuja primeira seção aponta para dados fora do arquivo.
pub fn xbe_com_secao_corrompida() -> Vec<u8> {
    let mut f = xbe_minimo();
    put_u32(&mut f, OFF_SECTION_HEADERS + 0x0C, 0x1000_0000); // raw address
    f
}

/// Padrão de logo usado nas fixtures: faixas diagonais, que comprimem bem em RLE.
fn logo_de_teste() -> Vec<u8> {
    let (w, h) = (xbe::Logo::LARGURA, xbe::Logo::ALTURA);
    let mut pixels = vec![0u8; w * h];
    for y in 0..h {
        for x in 0..w {
            pixels[y * w + x] = ((x / 10 + y / 3) % 16) as u8;
        }
    }
    pixels
}

#[allow(clippy::too_many_arguments)]
fn escrever_secao(
    f: &mut [u8],
    off: usize,
    flags: u32,
    va: u32,
    vsize: u32,
    raw: u32,
    rsize: u32,
    nome_va: u32,
) {
    put_u32(f, off, flags);
    put_u32(f, off + 0x04, va);
    put_u32(f, off + 0x08, vsize);
    put_u32(f, off + 0x0C, raw);
    put_u32(f, off + 0x10, rsize);
    put_u32(f, off + 0x14, nome_va);
    put_u32(f, off + 0x18, 0); // name ref count
    put_u32(f, off + 0x1C, 0); // head shared page ref count addr
    put_u32(f, off + 0x20, 0); // tail shared page ref count addr
    for i in 0..20 {
        f[off + 0x24 + i] = i as u8; // digest de mentira
    }
}

fn escrever_lib(f: &mut [u8], off: usize, nome: &[u8], major: u16, minor: u16, build: u16) {
    put_bytes(f, off, &nome[..nome.len().min(8)]);
    put_u16(f, off + 0x08, major);
    put_u16(f, off + 0x0A, minor);
    put_u16(f, off + 0x0C, build);
    put_u16(f, off + 0x0E, 0);
}

// ─── Imagens XDVDFS ───────────────────────────────────────────────────────────────

/// Setor onde fica o diretório raiz nas fixtures.
const SETOR_RAIZ: u32 = 33;
/// Setor do diretório `media`.
const SETOR_MEDIA: u32 = 34;
/// Primeiro setor de `default.xbe`.
const SETOR_XBE: u32 = 35;
/// Setor de `media/readme.txt`.
const SETOR_README: u32 = 40;
/// Setores totais da imagem.
const SETORES_TOTAIS: u64 = 41;

/// Imagem XISO cortada (partição na base 0) com `default.xbe` e `media/readme.txt`.
pub fn xiso_minimo() -> Vec<u8> {
    xiso_com_base(0)
}

/// Como [`xiso_minimo`], mas com a partição começando em `base`.
///
/// Use bases pequenas em teste: a imagem é alocada inteira, então `base` grande
/// significa alocação grande.
pub fn xiso_com_base(base: u64) -> Vec<u8> {
    construir_xiso(base, false)
}

/// Imagem cuja árvore de diretórios tem uma entrada que aponta para si mesma.
pub fn xiso_com_ciclo() -> Vec<u8> {
    construir_xiso(0, true)
}

fn construir_xiso(base: u64, com_ciclo: bool) -> Vec<u8> {
    let xbe = xbe_minimo();
    let total = usize::try_from(base + SETORES_TOTAIS * SECTOR_SIZE).expect("imagem cabe em usize");
    let mut img = vec![0u8; total];
    let setor = |n: u32| usize::try_from(base + u64::from(n) * SECTOR_SIZE).expect("setor");

    // ── diretório raiz: "default.xbe" (dword 0) e "media" (dword 7) ──
    let raiz = setor(SETOR_RAIZ);
    let dir_media_size = 14 + 10; // cabeçalho + "readme.txt"
    escrever_entrada(
        &mut img,
        raiz,
        0xFFFF,
        7,
        SETOR_XBE,
        xbe.len() as u32,
        ATTR_NORMAL,
        b"default.xbe",
    );
    escrever_entrada(
        &mut img,
        raiz + 28,
        0xFFFF,
        if com_ciclo { 7 } else { 0xFFFF },
        SETOR_MEDIA,
        dir_media_size,
        ATTR_DIRECTORY,
        b"media",
    );
    let raiz_size: u32 = 48; // 28 + 20, já alinhado a 4

    // ── diretório "media" ──
    escrever_entrada(
        &mut img,
        setor(SETOR_MEDIA),
        0xFFFF,
        0xFFFF,
        SETOR_README,
        README.len() as u32,
        ATTR_NORMAL,
        b"readme.txt",
    );

    // ── descritor de volume (setor 32) ──
    let vol = usize::try_from(base + VOLUME_SECTOR * SECTOR_SIZE).expect("volume");
    put_bytes(&mut img, vol, MAGIC);
    put_u32(&mut img, vol + 0x14, SETOR_RAIZ);
    put_u32(&mut img, vol + 0x18, raiz_size);
    put_bytes(
        &mut img,
        vol + 0x1C,
        &0x01C0_0000_0000_0000u64.to_le_bytes(),
    );
    let fim_magic = vol + SECTOR_SIZE as usize - MAGIC.len();
    put_bytes(&mut img, fim_magic, MAGIC);

    // ── conteúdo dos arquivos ──
    put_bytes(&mut img, setor(SETOR_XBE), &xbe);
    put_bytes(&mut img, setor(SETOR_README), README);

    img
}

#[allow(clippy::too_many_arguments)]
fn escrever_entrada(
    img: &mut [u8],
    off: usize,
    esquerda: u16,
    direita: u16,
    setor: u32,
    tamanho: u32,
    atributos: u8,
    nome: &[u8],
) {
    put_u16(img, off, esquerda);
    put_u16(img, off + 0x02, direita);
    put_u32(img, off + 0x04, setor);
    put_u32(img, off + 0x08, tamanho);
    img[off + 0x0C] = atributos;
    img[off + 0x0D] = nome.len() as u8;
    put_bytes(img, off + 0x0E, nome);
}

// ─── Escrita de campos ────────────────────────────────────────────────────────────

fn put_u16(buf: &mut [u8], off: usize, v: u16) {
    buf[off..off + 2].copy_from_slice(&v.to_le_bytes());
}

fn put_u32(buf: &mut [u8], off: usize, v: u32) {
    buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

fn put_bytes(buf: &mut [u8], off: usize, b: &[u8]) {
    buf[off..off + b.len()].copy_from_slice(b);
}

fn put_utf16(buf: &mut [u8], off: usize, s: &str, chars: usize) {
    for (i, u) in s.encode_utf16().take(chars).enumerate() {
        put_u16(buf, off + i * 2, u);
    }
}

/// Certificado de fixture já montado, para testes que precisam só dos metadados.
///
/// Evita que cada teste reconstrua um XBE inteiro quando só quer um [`Certificate`].
pub fn certificado_de_teste() -> Certificate {
    let bytes = xbe_minimo();
    crate::xbe::Xbe::parse(&bytes)
        .expect("fixture válida")
        .certificate
}

// ─── Partições FATX ───────────────────────────────────────────────────────────────

/// Setores por cluster nas fixtures FATX (16 KiB por cluster).
pub const FATX_CLUSTER_SETORES: u32 = 32;
/// Tamanho da partição de fixture.
const FATX_TAMANHO: usize = 2 * 1024 * 1024;
/// Conteúdo de `savedata.bin`.
pub const FATX_SAVE: &[u8] = b"save de fixture: nada proprietario aqui\n";
/// Tamanho de `TDATA/grande.bin` — maior que um cluster, de propósito.
pub const FATX_GRANDE_LEN: usize = 40_000;

/// Cluster do diretório raiz (o FATX fixa em 1).
const FATX_CLUSTER_RAIZ: u32 = 1;
/// Cluster do diretório `TDATA`.
const FATX_CLUSTER_TDATA: u32 = 2;
/// Cluster de `savedata.bin`.
const FATX_CLUSTER_SAVE: u32 = 3;
/// Primeiro cluster de `TDATA/grande.bin`.
const FATX_CLUSTER_GRANDE: u32 = 4;

/// Partição FATX mínima: `savedata.bin` na raiz e `TDATA/grande.bin` com mais de um
/// cluster, que é o caso que exercita o caminhamento da FAT.
pub fn fatx_minima() -> Vec<u8> {
    construir_fatx(false)
}

/// Partição em que a cadeia de clusters de `savedata.bin` aponta para si mesma.
pub fn fatx_com_ciclo() -> Vec<u8> {
    construir_fatx(true)
}

/// Partição FATX colocada no deslocamento de uma partição real do HD.
///
/// Devolve `(imagem, deslocamento)`.
pub fn fatx_em_deslocamento() -> (Vec<u8>, u64) {
    let inicio = 0x0008_0000u64; // mesmo lugar da partição X: do HD do Xbox
    let particao = fatx_minima();
    let total = usize::try_from(inicio).unwrap() + particao.len();
    let mut img = vec![0u8; total];
    put_bytes(&mut img, usize::try_from(inicio).unwrap(), &particao);
    (img, inicio)
}

fn construir_fatx(com_ciclo: bool) -> Vec<u8> {
    // 0xFF é o preenchimento que o FATX usa de verdade: em entrada de diretório marca
    // "fim das entradas", e na FAT marca "fim de cadeia". Começar tudo com 0xFF deixa a
    // fixture coerente sem precisar escrever cada byte.
    let mut img = vec![0xFFu8; FATX_TAMANHO];

    // ── superbloco ──
    put_bytes(&mut img, 0, &crate::fatx::MAGIC);
    put_u32(&mut img, 0x04, 0x0B5D_1A41); // volume id
    put_u32(&mut img, 0x08, FATX_CLUSTER_SETORES);
    put_u16(&mut img, 0x0C, 1); // uma cópia de FAT
    put_u16(&mut img, 0x0E, 0);

    // ── FAT de 16 bits ──
    let fat = usize::try_from(crate::fatx::FAT_OFFSET).unwrap();
    let mut entrada_fat = |cluster: u32, valor: u16| {
        put_u16(&mut img, fat + cluster as usize * 2, valor);
    };
    entrada_fat(0, 0xFFF8); // reservado
    entrada_fat(FATX_CLUSTER_RAIZ, 0xFFFF);
    entrada_fat(FATX_CLUSTER_TDATA, 0xFFFF);
    entrada_fat(
        FATX_CLUSTER_SAVE,
        if com_ciclo {
            FATX_CLUSTER_SAVE as u16
        } else {
            0xFFFF
        },
    );
    // grande.bin ocupa três clusters encadeados.
    entrada_fat(FATX_CLUSTER_GRANDE, FATX_CLUSTER_GRANDE as u16 + 1);
    entrada_fat(FATX_CLUSTER_GRANDE + 1, FATX_CLUSTER_GRANDE as u16 + 2);
    entrada_fat(FATX_CLUSTER_GRANDE + 2, 0xFFFF);

    // ── diretórios ──
    let raiz = offset_cluster_fatx(FATX_CLUSTER_RAIZ);
    escrever_entrada_fatx(
        &mut img,
        raiz,
        b"TDATA",
        crate::fatx::ATTR_DIRECTORY,
        FATX_CLUSTER_TDATA,
        0,
    );
    escrever_entrada_fatx(
        &mut img,
        raiz + crate::fatx::DIR_ENTRY_SIZE,
        b"savedata.bin",
        crate::fatx::ATTR_ARCHIVE,
        FATX_CLUSTER_SAVE,
        FATX_SAVE.len() as u32,
    );

    escrever_entrada_fatx(
        &mut img,
        offset_cluster_fatx(FATX_CLUSTER_TDATA),
        b"grande.bin",
        crate::fatx::ATTR_ARCHIVE,
        FATX_CLUSTER_GRANDE,
        FATX_GRANDE_LEN as u32,
    );

    // ── conteúdo dos arquivos ──
    put_bytes(&mut img, offset_cluster_fatx(FATX_CLUSTER_SAVE), FATX_SAVE);

    let cluster_bytes = FATX_CLUSTER_SETORES as usize * 512;
    for i in 0..FATX_GRANDE_LEN {
        let cluster = FATX_CLUSTER_GRANDE + (i / cluster_bytes) as u32;
        let dentro = i % cluster_bytes;
        img[offset_cluster_fatx(cluster) + dentro] = (i % 251) as u8;
    }

    img
}

/// Deslocamento de um cluster na fixture.
///
/// Repete a conta do leitor de propósito: se a fórmula de uma das duas mudar, os testes
/// acusam em vez de concordarem por construção.
fn offset_cluster_fatx(cluster: u32) -> usize {
    let cluster_bytes = FATX_CLUSTER_SETORES as usize * 512;
    let estimativa = FATX_TAMANHO / cluster_bytes + 1;
    let fat_bytes = (estimativa * 2).div_ceil(0x1000) * 0x1000;
    let dados = 0x1000 + fat_bytes;
    dados + (cluster as usize - 1) * cluster_bytes
}

fn escrever_entrada_fatx(
    img: &mut [u8],
    off: usize,
    nome: &[u8],
    atributos: u8,
    cluster: u32,
    tamanho: u32,
) {
    img[off] = nome.len() as u8;
    img[off + 0x01] = atributos;
    // O nome é preenchido com 0xFF até os 42 bytes, como o FATX faz.
    for i in 0..crate::fatx::MAX_NAME_LEN {
        img[off + 0x02 + i] = nome.get(i).copied().unwrap_or(0xFF);
    }
    put_u32(img, off + 0x2C, cluster);
    put_u32(img, off + 0x30, tamanho);
    put_u32(img, off + 0x34, 0x2C21_8000); // criação
    put_u32(img, off + 0x38, 0x2C21_8000); // última escrita
    put_u32(img, off + 0x3C, 0x2C21_8000); // último acesso
}
