//! Parser de executáveis Xbox (`.xbe`).
//!
//! O XBE é um PE modificado, carregado em `0x00010000`, que importa o kernel **por
//! ordinal** através de uma *thunk table*. Para o emulador ele é o ponto de entrada de
//! tudo: é dele que saem o endereço inicial de execução, as seções a mapear na memória
//! do guest, a tabela de thunks a resolver (ADR-0001) e os metadados que a biblioteca
//! da interface mostra (nome, title ID, região, logo).
//!
//! ## Ofuscação do entry point e da thunk table
//!
//! Dois campos do header são gravados XOR-ados com uma chave que depende do tipo de
//! build: retail, debug ou Chihiro (arcade). As chaves são constantes publicadas em
//! documentação pública ([`ENTRY_XOR_RETAIL`] e vizinhas), mas o parser **não confia
//! nelas cegamente**: ele testa as três e aceita a que produz endereços dentro da
//! imagem. Se nenhuma produzir endereço plausível, o erro lista os três candidatos em
//! vez de seguir com lixo — assim uma constante errada aparece como diagnóstico, não
//! como corrupção silenciosa.
//!
//! ## Referência
//!
//! Layout conforme documentação pública do formato (`XboxDevWiki` e descrição de Caustik).
//! Nenhum código ou header proprietário foi usado (ROADMAP §14).

use xbox_core::{Error, Result};

use crate::bytes::{latin1_at, slice_at, u16_at, u32_at, utf16_at};

/// Endereço de carga padrão de um XBE.
pub const DEFAULT_BASE_ADDRESS: u32 = 0x0001_0000;

/// Assinatura no início do arquivo: `XBEH`.
pub const MAGIC: [u8; 4] = *b"XBEH";

/// Tamanho de um header de seção.
const SECTION_HEADER_SIZE: usize = 0x38;

/// Tamanho de um registro de versão de biblioteca.
const LIBRARY_VERSION_SIZE: usize = 0x10;

/// Limite defensivo de seções. Títulos reais têm poucas dezenas; um arquivo que
/// declara mais do que isto está corrompido ou é malicioso.
const MAX_SECTIONS: u32 = 512;

/// Limite defensivo de versões de biblioteca declaradas.
const MAX_LIBRARIES: u32 = 64;

/// Chave XOR do entry point em builds retail.
pub const ENTRY_XOR_RETAIL: u32 = 0xA8FC_57AB;
/// Chave XOR do entry point em builds de debug.
pub const ENTRY_XOR_DEBUG: u32 = 0x9485_9D4B;
/// Chave XOR do entry point em builds Chihiro (arcade).
pub const ENTRY_XOR_CHIHIRO: u32 = 0x40B5_C16E;
/// Chave XOR do endereço da thunk table em builds retail.
pub const THUNK_XOR_RETAIL: u32 = 0x5B6D_40B6;
/// Chave XOR do endereço da thunk table em builds de debug.
pub const THUNK_XOR_DEBUG: u32 = 0xEFB1_F152;
/// Chave XOR do endereço da thunk table em builds Chihiro.
pub const THUNK_XOR_CHIHIRO: u32 = 0x2290_059D;

/// Tipo de build do executável, deduzido de qual chave XOR funciona.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XbeKind {
    /// Console de varejo.
    Retail,
    /// Kit de desenvolvimento / build de debug.
    Debug,
    /// Placa arcade Chihiro.
    Chihiro,
}

impl XbeKind {
    /// As três variantes, na ordem em que a detecção as testa.
    const TODAS: [Self; 3] = [Self::Retail, Self::Debug, Self::Chihiro];

    /// Chaves XOR (entry point, thunk table) deste tipo de build.
    const fn chaves(self) -> (u32, u32) {
        match self {
            Self::Retail => (ENTRY_XOR_RETAIL, THUNK_XOR_RETAIL),
            Self::Debug => (ENTRY_XOR_DEBUG, THUNK_XOR_DEBUG),
            Self::Chihiro => (ENTRY_XOR_CHIHIRO, THUNK_XOR_CHIHIRO),
        }
    }

    /// Nome legível.
    pub const fn nome(self) -> &'static str {
        match self {
            Self::Retail => "retail",
            Self::Debug => "debug",
            Self::Chihiro => "chihiro",
        }
    }
}

/// Flags de inicialização pedidas pelo título.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InitFlags(pub u32);

impl InitFlags {
    /// Monta o drive de utilidade (`Z:`).
    pub const MOUNT_UTILITY_DRIVE: u32 = 0x0000_0001;
    /// Formata o drive de utilidade no boot.
    pub const FORMAT_UTILITY_DRIVE: u32 = 0x0000_0002;
    /// O título exige que a RAM seja limitada a 64 MB.
    pub const LIMIT_64MB: u32 = 0x0000_0004;
    /// Não inicializa o HD.
    pub const DONT_SETUP_HARDDISK: u32 = 0x0000_0008;

    /// Testa um bit.
    pub const fn tem(self, bit: u32) -> bool {
        self.0 & bit != 0
    }

    /// Nomes dos bits ligados, para exibição.
    pub fn nomes(self) -> Vec<&'static str> {
        let mut v = Vec::new();
        for (bit, nome) in [
            (Self::MOUNT_UTILITY_DRIVE, "mount-utility-drive"),
            (Self::FORMAT_UTILITY_DRIVE, "format-utility-drive"),
            (Self::LIMIT_64MB, "limit-64mb"),
            (Self::DONT_SETUP_HARDDISK, "dont-setup-harddisk"),
        ] {
            if self.tem(bit) {
                v.push(nome);
            }
        }
        v
    }
}

/// Mídias em que o título aceita rodar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediaFlags(pub u32);

impl MediaFlags {
    /// Bits relevantes de mídia (o resto é modo de segurança).
    pub const MASK: u32 = 0x00FF_FFFF;
    /// Roda do HD.
    pub const HARDDISK: u32 = 0x0000_0001;
    /// DVD de camada dupla do Xbox.
    pub const DVD_X2: u32 = 0x0000_0002;
    /// DVD/CD.
    pub const DVD_CD: u32 = 0x0000_0004;
    /// CD.
    pub const CD: u32 = 0x0000_0008;
    /// DVD-5 somente leitura.
    pub const DVD_5_RO: u32 = 0x0000_0010;
    /// DVD-9 somente leitura.
    pub const DVD_9_RO: u32 = 0x0000_0020;
    /// DVD-5 gravável.
    pub const DVD_5_RW: u32 = 0x0000_0040;
    /// DVD-9 gravável.
    pub const DVD_9_RW: u32 = 0x0000_0080;
    /// Dongle.
    pub const DONGLE: u32 = 0x0000_0100;
    /// Placa de mídia (arcade).
    pub const MEDIA_BOARD: u32 = 0x0000_0200;
    /// Permite HD não seguro.
    pub const NONSECURE_HARD_DISK: u32 = 0x4000_0000;
    /// Modo não seguro.
    pub const NONSECURE_MODE: u32 = 0x8000_0000;

    /// Testa um bit.
    pub const fn tem(self, bit: u32) -> bool {
        self.0 & bit != 0
    }

    /// Nomes dos bits ligados, para exibição.
    pub fn nomes(self) -> Vec<&'static str> {
        let mut v = Vec::new();
        for (bit, nome) in [
            (Self::HARDDISK, "harddisk"),
            (Self::DVD_X2, "dvd-x2"),
            (Self::DVD_CD, "dvd-cd"),
            (Self::CD, "cd"),
            (Self::DVD_5_RO, "dvd5-ro"),
            (Self::DVD_9_RO, "dvd9-ro"),
            (Self::DVD_5_RW, "dvd5-rw"),
            (Self::DVD_9_RW, "dvd9-rw"),
            (Self::DONGLE, "dongle"),
            (Self::MEDIA_BOARD, "media-board"),
            (Self::NONSECURE_HARD_DISK, "nonsecure-harddisk"),
            (Self::NONSECURE_MODE, "nonsecure-mode"),
        ] {
            if self.tem(bit) {
                v.push(nome);
            }
        }
        v
    }
}

/// Regiões em que o título aceita rodar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegionFlags(pub u32);

impl RegionFlags {
    /// América do Norte.
    pub const NORTH_AMERICA: u32 = 0x0000_0001;
    /// Japão.
    pub const JAPAN: u32 = 0x0000_0002;
    /// Resto do mundo.
    pub const REST_OF_WORLD: u32 = 0x0000_0004;
    /// Reservado para manufatura.
    pub const MANUFACTURING: u32 = 0x8000_0000;

    /// Testa um bit.
    pub const fn tem(self, bit: u32) -> bool {
        self.0 & bit != 0
    }

    /// Nomes dos bits ligados, para exibição.
    pub fn nomes(self) -> Vec<&'static str> {
        let mut v = Vec::new();
        for (bit, nome) in [
            (Self::NORTH_AMERICA, "america-do-norte"),
            (Self::JAPAN, "japao"),
            (Self::REST_OF_WORLD, "resto-do-mundo"),
            (Self::MANUFACTURING, "manufatura"),
        ] {
            if self.tem(bit) {
                v.push(nome);
            }
        }
        v
    }
}

/// Flags de uma seção.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SectionFlags(pub u32);

impl SectionFlags {
    /// Gravável.
    pub const WRITABLE: u32 = 0x0000_0001;
    /// Carregada no boot (não sob demanda).
    pub const PRELOAD: u32 = 0x0000_0002;
    /// Executável.
    pub const EXECUTABLE: u32 = 0x0000_0004;
    /// Arquivo inserido.
    pub const INSERTED_FILE: u32 = 0x0000_0008;
    /// Primeira página somente leitura.
    pub const HEAD_PAGE_READ_ONLY: u32 = 0x0000_0010;
    /// Última página somente leitura.
    pub const TAIL_PAGE_READ_ONLY: u32 = 0x0000_0020;

    /// Testa um bit.
    pub const fn tem(self, bit: u32) -> bool {
        self.0 & bit != 0
    }

    /// Resumo em três caracteres no estilo `rwx`.
    pub fn rwx(self) -> String {
        format!(
            "r{}{}",
            if self.tem(Self::WRITABLE) { 'w' } else { '-' },
            if self.tem(Self::EXECUTABLE) { 'x' } else { '-' }
        )
    }
}

/// Header do XBE, com os campos já lidos (endereços ainda virtuais).
#[derive(Debug, Clone)]
pub struct Header {
    /// Endereço de carga da imagem.
    pub base_address: u32,
    /// Tamanho da região de headers.
    pub size_of_headers: u32,
    /// Tamanho da imagem em memória.
    pub size_of_image: u32,
    /// Tamanho do header de imagem.
    pub size_of_image_header: u32,
    /// Data/hora de build (epoch Unix).
    pub timestamp: u32,
    /// Endereço virtual do certificado.
    pub certificate_address: u32,
    /// Quantidade de seções.
    pub section_count: u32,
    /// Endereço virtual da tabela de headers de seção.
    pub section_headers_address: u32,
    /// Flags de inicialização.
    pub init_flags: InitFlags,
    /// Entry point **ainda ofuscado**.
    pub entry_point_raw: u32,
    /// Endereço virtual do diretório de TLS (0 = sem TLS).
    pub tls_address: u32,
    /// `SizeOfStackCommit` herdado do PE.
    pub pe_stack_commit: u32,
    /// `SizeOfHeapReserve` herdado do PE.
    pub pe_heap_reserve: u32,
    /// `SizeOfHeapCommit` herdado do PE.
    pub pe_heap_commit: u32,
    /// `ImageBase` original do PE.
    pub pe_base_address: u32,
    /// `SizeOfImage` original do PE.
    pub pe_size_of_image: u32,
    /// Checksum do PE.
    pub pe_checksum: u32,
    /// Timestamp do PE.
    pub pe_timestamp: u32,
    /// Endereço virtual do caminho de debug.
    pub debug_pathname_address: u32,
    /// Endereço virtual do nome de arquivo de debug.
    pub debug_filename_address: u32,
    /// Endereço virtual do nome de arquivo de debug em Unicode.
    pub debug_unicode_filename_address: u32,
    /// Endereço da thunk table do kernel, **ainda ofuscado**.
    pub kernel_thunk_raw: u32,
    /// Endereço do diretório de imports não-kernel.
    pub non_kernel_import_dir_address: u32,
    /// Quantidade de versões de biblioteca declaradas.
    pub library_version_count: u32,
    /// Endereço virtual da tabela de versões de biblioteca.
    pub library_versions_address: u32,
    /// Endereço virtual da versão da biblioteca do kernel.
    pub kernel_library_version_address: u32,
    /// Endereço virtual da versão do XAPI.
    pub xapi_library_version_address: u32,
    /// Endereço virtual do bitmap do logo.
    pub logo_bitmap_address: u32,
    /// Tamanho do bitmap do logo, em bytes.
    pub logo_bitmap_size: u32,
    /// Campos crus além de `0x178`, quando `size_of_image_header` passa disso.
    ///
    /// Headers de SDK novo são maiores: um XBE retail de 2004 declara
    /// `size_of_image_header = 0x184`, com três palavras extras cujo significado não
    /// está documentado publicamente. Guardamos cruas em vez de ignorar, para que o
    /// loader da Fase 4 possa examiná-las se precisar.
    pub extended: Vec<u32>,
}

impl Header {
    fn parse(data: &[u8]) -> Result<Self> {
        let magic = slice_at(data, 0, 4)?;
        if magic != MAGIC {
            return Err(Error::format(format!(
                "não é um XBE: esperava assinatura {:?}, encontrou {magic:02X?}",
                std::str::from_utf8(&MAGIC).unwrap_or("XBEH")
            )));
        }
        // 0x004..0x104 é a assinatura digital de 256 bytes; não a validamos (precisaria
        // da chave pública da Microsoft) e ela não afeta a execução sob kernel HLE.
        let mut header = Self {
            base_address: u32_at(data, 0x104)?,
            size_of_headers: u32_at(data, 0x108)?,
            size_of_image: u32_at(data, 0x10C)?,
            size_of_image_header: u32_at(data, 0x110)?,
            timestamp: u32_at(data, 0x114)?,
            certificate_address: u32_at(data, 0x118)?,
            section_count: u32_at(data, 0x11C)?,
            section_headers_address: u32_at(data, 0x120)?,
            init_flags: InitFlags(u32_at(data, 0x124)?),
            entry_point_raw: u32_at(data, 0x128)?,
            tls_address: u32_at(data, 0x12C)?,
            pe_stack_commit: u32_at(data, 0x130)?,
            pe_heap_reserve: u32_at(data, 0x134)?,
            pe_heap_commit: u32_at(data, 0x138)?,
            pe_base_address: u32_at(data, 0x13C)?,
            pe_size_of_image: u32_at(data, 0x140)?,
            pe_checksum: u32_at(data, 0x144)?,
            pe_timestamp: u32_at(data, 0x148)?,
            debug_pathname_address: u32_at(data, 0x14C)?,
            debug_filename_address: u32_at(data, 0x150)?,
            debug_unicode_filename_address: u32_at(data, 0x154)?,
            kernel_thunk_raw: u32_at(data, 0x158)?,
            non_kernel_import_dir_address: u32_at(data, 0x15C)?,
            library_version_count: u32_at(data, 0x160)?,
            library_versions_address: u32_at(data, 0x164)?,
            kernel_library_version_address: u32_at(data, 0x168)?,
            xapi_library_version_address: u32_at(data, 0x16C)?,
            logo_bitmap_address: u32_at(data, 0x170)?,
            logo_bitmap_size: u32_at(data, 0x174)?,
            extended: Vec::new(),
        };

        // `min` limita um `size_of_image_header` corrompido; nenhum header conhecido
        // passa de 0x200.
        let fim = (header.size_of_image_header as usize).min(0x200);
        let mut off = 0x178;
        while off + 4 <= fim {
            header.extended.push(u32_at(data, off)?);
            off += 4;
        }
        Ok(header)
    }
}

/// Certificado do título: é daqui que saem os metadados da biblioteca.
#[derive(Debug, Clone)]
pub struct Certificate {
    /// Tamanho declarado do certificado.
    pub size: u32,
    /// Data/hora.
    pub timestamp: u32,
    /// Title ID (os dois primeiros bytes identificam a publicadora).
    pub title_id: u32,
    /// Nome do título (UTF-16 no arquivo).
    pub title_name: String,
    /// Title IDs alternativos.
    pub alternate_title_ids: Vec<u32>,
    /// Mídias permitidas.
    pub allowed_media: MediaFlags,
    /// Regiões permitidas.
    pub game_region: RegionFlags,
    /// Classificação indicativa.
    pub game_ratings: u32,
    /// Número do disco.
    pub disk_number: u32,
    /// Versão do título.
    pub version: u32,
    /// Chave de LAN (System Link).
    pub lan_key: [u8; 16],
    /// Chave de assinatura.
    pub signature_key: [u8; 16],
}

impl Certificate {
    fn parse(data: &[u8], off: usize) -> Result<Self> {
        let size = u32_at(data, off)?;
        // O certificado cresceu entre versões do SDK; só lemos campos que caibam no
        // tamanho declarado, para não inventar dados em XBEs antigos.
        let cabe = |fim: u32| size >= fim;

        let mut alternate_title_ids = Vec::new();
        if cabe(0x9C) {
            for i in 0..16 {
                alternate_title_ids.push(u32_at(data, off + 0x5C + i * 4)?);
            }
        }
        let mut lan_key = [0u8; 16];
        if cabe(0xC0) {
            lan_key.copy_from_slice(slice_at(data, off + 0xB0, 16)?);
        }
        let mut signature_key = [0u8; 16];
        if cabe(0xD0) {
            signature_key.copy_from_slice(slice_at(data, off + 0xC0, 16)?);
        }

        Ok(Self {
            size,
            timestamp: u32_at(data, off + 0x04)?,
            title_id: u32_at(data, off + 0x08)?,
            title_name: utf16_at(data, off + 0x0C, 40)?,
            alternate_title_ids,
            allowed_media: MediaFlags(if cabe(0xA0) {
                u32_at(data, off + 0x9C)?
            } else {
                0
            }),
            game_region: RegionFlags(if cabe(0xA4) {
                u32_at(data, off + 0xA0)?
            } else {
                0
            }),
            game_ratings: if cabe(0xA8) {
                u32_at(data, off + 0xA4)?
            } else {
                0
            },
            disk_number: if cabe(0xAC) {
                u32_at(data, off + 0xA8)?
            } else {
                0
            },
            version: if cabe(0xB0) {
                u32_at(data, off + 0xAC)?
            } else {
                0
            },
            lan_key,
            signature_key,
        })
    }

    /// Title ID no formato convencional, ex.: `MS-004D`.
    pub fn title_id_formatado(&self) -> String {
        let bytes = self.title_id.to_be_bytes();
        let publicadora: String = bytes[0..2]
            .iter()
            .map(|&b| {
                if b.is_ascii_graphic() {
                    char::from(b)
                } else {
                    '?'
                }
            })
            .collect();
        format!(
            "{publicadora}-{:04X}",
            u16::from_be_bytes([bytes[2], bytes[3]])
        )
    }
}

/// Uma seção da imagem: o que o loader mapeia na memória do guest.
#[derive(Debug, Clone)]
pub struct Section {
    /// Nome (`.text`, `.rdata`, ...).
    pub name: String,
    /// Flags.
    pub flags: SectionFlags,
    /// Endereço virtual onde a seção é carregada.
    pub virtual_address: u32,
    /// Tamanho em memória.
    pub virtual_size: u32,
    /// Deslocamento no arquivo.
    pub raw_address: u32,
    /// Tamanho no arquivo.
    pub raw_size: u32,
    /// Digest SHA-1 declarado.
    pub digest: [u8; 20],
}

/// Versão de uma biblioteca estaticamente linkada no título.
///
/// É isto que diz quais versões de `D3D8`, `XAPILIB`, `DSOUND` etc. estão **dentro** do
/// jogo. Não usamos isso para patch (ADR-0002), mas é informação de diagnóstico
/// valiosa quando um título se comporta de forma estranha.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryVersion {
    /// Nome da biblioteca.
    pub name: String,
    /// Versão maior.
    pub major: u16,
    /// Versão menor.
    pub minor: u16,
    /// Número de build.
    pub build: u16,
    /// Flags (QFE, aprovação, build de debug).
    pub flags: u16,
}

impl LibraryVersion {
    /// Versão no formato `major.minor.build`.
    pub fn versao(&self) -> String {
        format!("{}.{}.{}", self.major, self.minor, self.build)
    }
}

/// Diretório de TLS (Thread Local Storage).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tls {
    /// Início dos dados de TLS.
    pub data_start_address: u32,
    /// Fim dos dados de TLS.
    pub data_end_address: u32,
    /// Endereço do índice de TLS.
    pub index_address: u32,
    /// Endereço do array de callbacks.
    pub callback_address: u32,
    /// Bytes zerados após os dados.
    pub size_of_zero_fill: u32,
    /// Características.
    pub characteristics: u32,
}

/// Logo decodificado do XBE (nibbles de escala de cinza).
#[derive(Debug, Clone)]
pub struct Logo {
    /// Largura em pixels.
    pub width: usize,
    /// Altura em pixels.
    pub height: usize,
    /// Um pixel por byte, valores 0–15.
    pub pixels: Vec<u8>,
}

impl Logo {
    /// Dimensões convencionais do logo de um XBE.
    pub const LARGURA: usize = 100;
    /// Altura convencional do logo de um XBE.
    pub const ALTURA: usize = 17;

    /// Valor do pixel, ou 0 fora dos limites.
    pub fn pixel(&self, x: usize, y: usize) -> u8 {
        if x >= self.width || y >= self.height {
            return 0;
        }
        self.pixels.get(y * self.width + x).copied().unwrap_or(0)
    }
}

/// Um XBE já parseado, emprestando os bytes do arquivo.
#[derive(Debug, Clone)]
pub struct Xbe<'a> {
    /// Bytes do arquivo inteiro.
    data: &'a [u8],
    /// Header.
    pub header: Header,
    /// Certificado.
    pub certificate: Certificate,
    /// Seções, na ordem do arquivo.
    pub sections: Vec<Section>,
    /// Versões de biblioteca declaradas.
    pub libraries: Vec<LibraryVersion>,
    /// Diretório de TLS, se houver.
    pub tls: Option<Tls>,
    /// Tipo de build detectado.
    pub kind: XbeKind,
    /// Entry point já desofuscado.
    pub entry_point: u32,
    /// Endereço da thunk table já desofuscado.
    pub kernel_thunk: u32,
}

impl<'a> Xbe<'a> {
    /// Parseia um XBE a partir dos bytes do arquivo.
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        let header = Header::parse(data)?;
        let (kind, entry_point, kernel_thunk) = detectar_kind(&header)?;

        let cert_off = va_para_offset(&header, header.certificate_address, "certificado")?;
        let certificate = Certificate::parse(data, cert_off)?;

        let sections = parse_sections(data, &header)?;
        let libraries = parse_libraries(data, &header)?;
        let tls = parse_tls(data, &header, &sections)?;

        Ok(Self {
            data,
            header,
            certificate,
            sections,
            libraries,
            tls,
            kind,
            entry_point,
            kernel_thunk,
        })
    }

    /// Nome do título, ou um texto de reserva quando o certificado não tem nome.
    pub fn titulo(&self) -> &str {
        if self.certificate.title_name.is_empty() {
            "(sem nome)"
        } else {
            &self.certificate.title_name
        }
    }

    /// Bytes de uma seção, como estão no arquivo.
    pub fn section_data(&self, index: usize) -> Result<&'a [u8]> {
        let s = self
            .sections
            .get(index)
            .ok_or_else(|| Error::format(format!("seção {index} não existe")))?;
        slice_at(self.data, s.raw_address as usize, s.raw_size as usize)
    }

    /// Procura uma seção pelo nome.
    pub fn section_por_nome(&self, nome: &str) -> Option<(usize, &Section)> {
        self.sections
            .iter()
            .enumerate()
            .find(|(_, s)| s.name == nome)
    }

    /// Traduz um endereço virtual do XBE em deslocamento no arquivo.
    ///
    /// Procura primeiro na região de headers e depois nas seções — é a tradução que o
    /// loader da Fase 4 faz, e sem ela qualquer ponteiro que o jogo guarda dentro de
    /// uma seção (TLS, thunk table, tabelas de dados) fica inalcançável.
    pub fn va_to_offset(&self, va: u32) -> Option<usize> {
        resolver_va(&self.header, &self.sections, va)
    }

    /// Ordinais do kernel que o título importa, lidos da thunk table.
    ///
    /// Esta é a lista de trabalho da Fase 4: cada ordinal aqui é uma função do kernel
    /// que precisa existir para o jogo rodar. Entradas são `u32` com o bit 31 ligado,
    /// terminadas por zero.
    ///
    /// Só metadados de importação são lidos — nenhum código da biblioteca é examinado,
    /// o que preserva o caráter de reimplementação limpa do ADR-0001.
    pub fn kernel_imports(&self) -> Result<Vec<u32>> {
        /// Bit que marca uma importação por ordinal.
        const BIT_ORDINAL: u32 = 0x8000_0000;
        /// O kernel do Xbox exporta menos de 400 ordinais; mais que isto é lixo.
        const MAX: usize = 512;

        if self.kernel_thunk == 0 {
            return Ok(Vec::new());
        }
        let mut off = self.va_to_offset(self.kernel_thunk).ok_or_else(|| {
            Error::format(format!(
                "thunk table em 0x{:08X} não cai em nenhuma seção",
                self.kernel_thunk
            ))
        })?;

        let mut ordinais = Vec::new();
        loop {
            let bruto = u32_at(self.data, off)?;
            if bruto == 0 {
                break;
            }
            if bruto & BIT_ORDINAL == 0 {
                return Err(Error::format(format!(
                    "entrada de thunk 0x{bruto:08X} sem o bit de ordinal: a tabela \
                     provavelmente não está em 0x{:08X}",
                    self.kernel_thunk
                )));
            }
            ordinais.push(bruto & 0x0000_FFFF);
            if ordinais.len() >= MAX {
                return Err(Error::format(format!(
                    "thunk table com mais de {MAX} entradas sem terminador"
                )));
            }
            off += 4;
        }
        Ok(ordinais)
    }

    /// Caminho de debug gravado no arquivo, se houver.
    pub fn debug_pathname(&self) -> Option<String> {
        let off = va_para_offset(&self.header, self.header.debug_pathname_address, "debug").ok()?;
        let resto = self.data.get(off..)?;
        let fim = resto
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(resto.len())
            .min(260);
        latin1_at(self.data, off, fim).ok()
    }

    /// Decodifica o logo (RLE de nibbles) em pixels de escala de cinza.
    ///
    /// Devolve `None` quando o XBE não tem logo.
    pub fn logo(&self) -> Option<Logo> {
        if self.header.logo_bitmap_size == 0 {
            return None;
        }
        let off = va_para_offset(&self.header, self.header.logo_bitmap_address, "logo").ok()?;
        let bytes = slice_at(self.data, off, self.header.logo_bitmap_size as usize).ok()?;
        Some(decodificar_logo(bytes, Logo::LARGURA, Logo::ALTURA))
    }
}

/// Testa as três chaves XOR e devolve a que produz endereços dentro da imagem.
fn detectar_kind(h: &Header) -> Result<(XbeKind, u32, u32)> {
    let base = h.base_address;
    let fim = base.saturating_add(h.size_of_image.max(h.size_of_headers));
    let plausivel = |va: u32| va >= base && va < fim;

    let mut candidatos = Vec::new();
    for kind in XbeKind::TODAS {
        let (ke, kt) = kind.chaves();
        let ep = h.entry_point_raw ^ ke;
        let thunk = h.kernel_thunk_raw ^ kt;
        // thunk == 0 é aceitável: um XBE que não importa o kernel (raro, mas válido).
        if plausivel(ep) && (thunk == 0 || plausivel(thunk)) {
            return Ok((kind, ep, thunk));
        }
        candidatos.push(format!(
            "{}: entry 0x{ep:08X} thunk 0x{thunk:08X}",
            kind.nome()
        ));
    }

    Err(Error::format(format!(
        "nenhuma chave XOR conhecida produz endereços dentro da imagem \
         (base 0x{base:08X}, fim 0x{fim:08X}); candidatos: {}",
        candidatos.join(" | ")
    )))
}

/// Converte endereço virtual em deslocamento no arquivo, para dados na região de
/// headers (certificado, tabelas, strings, logo).
fn va_para_offset(h: &Header, va: u32, oque: &str) -> Result<usize> {
    if va == 0 {
        return Err(Error::format(format!("{oque}: endereço nulo")));
    }
    let off = va
        .checked_sub(h.base_address)
        .ok_or_else(|| Error::format(format!("{oque}: VA 0x{va:08X} abaixo da base")))?;
    if off >= h.size_of_headers {
        return Err(Error::format(format!(
            "{oque}: VA 0x{va:08X} cai fora da região de headers (0x{:X} bytes)",
            h.size_of_headers
        )));
    }
    Ok(off as usize)
}

/// Converte VA em deslocamento no arquivo olhando headers **e** seções.
///
/// Devolve `None` quando o endereço não está em nenhuma região com dados no arquivo —
/// o que inclui o caso legítimo de um VA que só existe em memória (BSS: `virtual_size`
/// maior que `raw_size`).
fn resolver_va(h: &Header, sections: &[Section], va: u32) -> Option<usize> {
    if va == 0 {
        return None;
    }
    if let Ok(off) = va_para_offset(h, va, "endereço virtual") {
        return Some(off);
    }
    for s in sections {
        if s.raw_size == 0 {
            continue;
        }
        let fim = s.virtual_address.checked_add(s.raw_size)?;
        if va >= s.virtual_address && va < fim {
            let delta = (va - s.virtual_address) as usize;
            return (s.raw_address as usize).checked_add(delta);
        }
    }
    None
}

fn parse_sections(data: &[u8], h: &Header) -> Result<Vec<Section>> {
    if h.section_count > MAX_SECTIONS {
        return Err(Error::format(format!(
            "{} seções declaradas (limite {MAX_SECTIONS})",
            h.section_count
        )));
    }
    if h.section_count == 0 {
        return Ok(Vec::new());
    }
    let tabela = va_para_offset(h, h.section_headers_address, "tabela de seções")?;
    let mut saida = Vec::with_capacity(h.section_count as usize);

    for i in 0..h.section_count as usize {
        let off = tabela + i * SECTION_HEADER_SIZE;
        let flags = SectionFlags(u32_at(data, off)?);
        let virtual_address = u32_at(data, off + 0x04)?;
        let virtual_size = u32_at(data, off + 0x08)?;
        let raw_address = u32_at(data, off + 0x0C)?;
        let raw_size = u32_at(data, off + 0x10)?;
        let name_address = u32_at(data, off + 0x14)?;
        let mut digest = [0u8; 20];
        digest.copy_from_slice(slice_at(data, off + 0x24, 20)?);

        // Validar aqui evita que cada consumidor tenha que validar depois.
        let fim_raw = (raw_address as u64) + (raw_size as u64);
        if fim_raw > data.len() as u64 {
            return Err(Error::format(format!(
                "seção {i}: dados em 0x{raw_address:X}..0x{fim_raw:X} passam do fim do arquivo (0x{:X})",
                data.len()
            )));
        }

        let name = va_para_offset(h, name_address, "nome de seção")
            .and_then(|o| {
                let resto = slice_at(data, o, 1).map(|_| &data[o..])?;
                let fim = resto
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(resto.len())
                    .min(64);
                latin1_at(data, o, fim)
            })
            .unwrap_or_else(|_| format!("(sem nome {i})"));

        saida.push(Section {
            name,
            flags,
            virtual_address,
            virtual_size,
            raw_address,
            raw_size,
            digest,
        });
    }
    Ok(saida)
}

fn parse_libraries(data: &[u8], h: &Header) -> Result<Vec<LibraryVersion>> {
    if h.library_version_count == 0 || h.library_versions_address == 0 {
        return Ok(Vec::new());
    }
    if h.library_version_count > MAX_LIBRARIES {
        return Err(Error::format(format!(
            "{} versões de biblioteca declaradas (limite {MAX_LIBRARIES})",
            h.library_version_count
        )));
    }
    let tabela = va_para_offset(h, h.library_versions_address, "tabela de bibliotecas")?;
    let mut saida = Vec::with_capacity(h.library_version_count as usize);
    for i in 0..h.library_version_count as usize {
        let off = tabela + i * LIBRARY_VERSION_SIZE;
        saida.push(LibraryVersion {
            name: latin1_at(data, off, 8)?,
            major: u16_at(data, off + 0x08)?,
            minor: u16_at(data, off + 0x0A)?,
            build: u16_at(data, off + 0x0C)?,
            flags: u16_at(data, off + 0x0E)?,
        });
    }
    Ok(saida)
}

fn parse_tls(data: &[u8], h: &Header, sections: &[Section]) -> Result<Option<Tls>> {
    if h.tls_address == 0 {
        return Ok(None);
    }
    // O diretório de TLS **não** fica na região de headers em jogos reais: num XBE
    // retail de 2004 ele está dentro de `.rdata`. Resolver só pela região de headers
    // fazia o parser devolver `None` e perder o endereço — e sem TLS o loader da
    // Fase 4 não consegue inicializar as threads do título.
    let Some(off) = resolver_va(h, sections, h.tls_address) else {
        return Ok(None);
    };
    Ok(Some(Tls {
        data_start_address: u32_at(data, off)?,
        data_end_address: u32_at(data, off + 0x04)?,
        index_address: u32_at(data, off + 0x08)?,
        callback_address: u32_at(data, off + 0x0C)?,
        size_of_zero_fill: u32_at(data, off + 0x10)?,
        characteristics: u32_at(data, off + 0x14)?,
    }))
}

/// Decodifica o RLE do logo.
///
/// Dois formatos de registro, segundo a documentação pública:
///
/// - 1 byte, quando o bit 0 está ligado: comprimento nos bits 1–3, valor nos bits 4–7;
/// - 2 bytes, quando o bit 0 está desligado e o bit 1 ligado: comprimento nos bits
///   2–11, valor nos bits 12–15.
///
/// **Conformidade pendente de verificação:** o teste de ida e volta usa o nosso próprio
/// codificador, o que prova consistência interna, não conformidade com XBEs reais. Está
/// na lista de verificação da Fase 1 (`docs/compat/formatos.md`).
fn decodificar_logo(bytes: &[u8], largura: usize, altura: usize) -> Logo {
    let total = largura * altura;
    let mut pixels = Vec::with_capacity(total);
    let mut i = 0usize;

    while i < bytes.len() && pixels.len() < total {
        let b0 = bytes[i];
        let (comprimento, valor, consumido) = if b0 & 1 != 0 {
            (usize::from((b0 >> 1) & 0x07), (b0 >> 4) & 0x0F, 1)
        } else if b0 & 2 != 0 {
            let Some(&b1) = bytes.get(i + 1) else { break };
            let w = u16::from(b0) | (u16::from(b1) << 8);
            (usize::from((w >> 2) & 0x03FF), ((w >> 12) & 0x0F) as u8, 2)
        } else {
            // Registro inválido: paramos em vez de inventar pixels.
            break;
        };
        i += consumido;
        let quantos = comprimento.min(total - pixels.len());
        pixels.extend(std::iter::repeat_n(valor, quantos));
    }

    pixels.resize(total, 0);
    Logo {
        width: largura,
        height: altura,
        pixels,
    }
}

/// Codifica pixels em RLE no formato do logo do XBE.
///
/// Existe para construir fixtures de teste (ver [`crate::fixtures`]) e para o
/// `xbetool` poder gerar um XBE de exemplo.
pub fn codificar_logo(pixels: &[u8]) -> Vec<u8> {
    let mut saida = Vec::new();
    let mut i = 0usize;
    while i < pixels.len() {
        let valor = pixels[i] & 0x0F;
        let mut n = 1usize;
        while i + n < pixels.len() && pixels[i + n] & 0x0F == valor && n < 0x3FF {
            n += 1;
        }
        let mut restante = n;
        while restante > 0 {
            if restante <= 7 {
                saida.push(1 | ((restante as u8) << 1) | (valor << 4));
                restante = 0;
            } else {
                let pedaco = restante.min(0x3FF);
                let w = 0b10_u16 | ((pedaco as u16) << 2) | (u16::from(valor) << 12);
                saida.extend_from_slice(&w.to_le_bytes());
                restante -= pedaco;
            }
        }
        i += n;
    }
    saida
}

#[cfg(test)]
mod tests {
    use super::{
        Logo, MediaFlags, RegionFlags, SectionFlags, Xbe, XbeKind, codificar_logo, decodificar_logo,
    };
    use crate::fixtures;

    #[test]
    fn parseia_a_fixture_minima() {
        let bytes = fixtures::xbe_minimo();
        let xbe = Xbe::parse(&bytes).expect("fixture deve parsear");

        assert_eq!(xbe.kind, XbeKind::Retail);
        assert_eq!(xbe.header.base_address, super::DEFAULT_BASE_ADDRESS);
        assert_eq!(xbe.titulo(), fixtures::TITULO);
        assert_eq!(xbe.certificate.title_id, fixtures::TITLE_ID);
        assert_eq!(xbe.entry_point, fixtures::ENTRY_POINT);
        assert_eq!(xbe.kernel_thunk, fixtures::KERNEL_THUNK);
    }

    #[test]
    fn le_secoes_e_dados() {
        let bytes = fixtures::xbe_minimo();
        let xbe = Xbe::parse(&bytes).unwrap();

        assert_eq!(xbe.sections.len(), 2);
        let (idx, text) = xbe.section_por_nome(".text").expect("tem .text");
        assert!(text.flags.tem(SectionFlags::EXECUTABLE));
        assert_eq!(text.flags.rwx(), "r-x");
        assert_eq!(xbe.section_data(idx).unwrap(), fixtures::TEXT_DATA);

        let (idx, rdata) = xbe.section_por_nome(".rdata").expect("tem .rdata");
        assert!(!rdata.flags.tem(SectionFlags::EXECUTABLE));
        assert_eq!(xbe.section_data(idx).unwrap().len(), fixtures::RDATA_LEN);
    }

    #[test]
    fn le_bibliotecas_e_tls() {
        let bytes = fixtures::xbe_minimo();
        let xbe = Xbe::parse(&bytes).unwrap();

        let nomes: Vec<&str> = xbe.libraries.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(nomes, vec!["XAPILIB", "D3D8"]);
        assert_eq!(xbe.libraries[1].versao(), "1.0.5849");
        assert!(xbe.tls.is_some());
    }

    #[test]
    fn certificado_formata_title_id_e_flags() {
        let bytes = fixtures::xbe_minimo();
        let xbe = Xbe::parse(&bytes).unwrap();

        assert_eq!(xbe.certificate.title_id_formatado(), "OB-0001");
        assert!(xbe.certificate.allowed_media.tem(MediaFlags::DVD_X2));
        assert!(xbe.certificate.game_region.tem(RegionFlags::NORTH_AMERICA));
        assert!(
            xbe.certificate
                .game_region
                .nomes()
                .contains(&"america-do-norte")
        );
    }

    #[test]
    fn detecta_build_de_debug() {
        let bytes = fixtures::xbe_com_kind(XbeKind::Debug);
        let xbe = Xbe::parse(&bytes).unwrap();
        assert_eq!(xbe.kind, XbeKind::Debug);
        assert_eq!(xbe.entry_point, fixtures::ENTRY_POINT);
    }

    #[test]
    fn assinatura_errada_e_recusada() {
        let mut bytes = fixtures::xbe_minimo();
        bytes[0] = b'Z';
        let err = Xbe::parse(&bytes).unwrap_err();
        assert!(err.to_string().contains("não é um XBE"), "veio: {err}");
    }

    #[test]
    fn entry_point_implausivel_e_recusado() {
        let mut bytes = fixtures::xbe_minimo();
        // Entry point que nenhuma chave conhecida leva para dentro da imagem.
        bytes[0x128..0x12C].copy_from_slice(&0u32.to_le_bytes());
        let err = Xbe::parse(&bytes).unwrap_err();
        assert!(err.to_string().contains("nenhuma chave XOR"), "veio: {err}");
    }

    #[test]
    fn secao_apontando_fora_do_arquivo_e_recusada() {
        let bytes = fixtures::xbe_com_secao_corrompida();
        let err = Xbe::parse(&bytes).unwrap_err();
        assert!(
            err.to_string().contains("passam do fim do arquivo"),
            "veio: {err}"
        );
    }

    #[test]
    fn arquivo_vazio_ou_curto_nao_causa_panico() {
        for n in 0..0x200 {
            let bytes = fixtures::xbe_minimo();
            let _ = Xbe::parse(&bytes[..n.min(bytes.len())]);
        }
    }

    #[test]
    fn logo_ida_e_volta() {
        // Prova consistência do par codificador/decodificador, não conformidade com
        // XBEs reais — ver nota em `decodificar_logo`.
        let mut pixels = vec![0u8; Logo::LARGURA * Logo::ALTURA];
        for (i, p) in pixels.iter_mut().enumerate() {
            *p = ((i / 37) % 16) as u8;
        }
        let rle = codificar_logo(&pixels);
        let logo = decodificar_logo(&rle, Logo::LARGURA, Logo::ALTURA);
        assert_eq!(logo.pixels, pixels);
        assert!(
            rle.len() < pixels.len(),
            "RLE deveria comprimir: {} bytes",
            rle.len()
        );
    }

    #[test]
    fn logo_truncado_nao_causa_panico() {
        let pixels = vec![7u8; Logo::LARGURA * Logo::ALTURA];
        let rle = codificar_logo(&pixels);
        for n in 0..rle.len() {
            let logo = decodificar_logo(&rle[..n], Logo::LARGURA, Logo::ALTURA);
            assert_eq!(logo.pixels.len(), Logo::LARGURA * Logo::ALTURA);
        }
    }

    #[test]
    fn logo_da_fixture_e_legivel() {
        let bytes = fixtures::xbe_minimo();
        let xbe = Xbe::parse(&bytes).unwrap();
        let logo = xbe.logo().expect("fixture tem logo");
        assert_eq!(logo.width, Logo::LARGURA);
        assert_eq!(logo.height, Logo::ALTURA);
        assert!(logo.pixels.iter().any(|&p| p != 0), "logo saiu todo preto");
    }
}
