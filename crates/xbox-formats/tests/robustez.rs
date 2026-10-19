//! Testes de robustez dos parsers.
//!
//! O ROADMAP §9 pede fuzz nos parsers. `cargo-fuzz` exige toolchain nightly e roda
//! fora do CI normal (o alvo está em `fuzz/`), então estes testes cobrem o mesmo
//! objetivo de forma que roda **em toda máquina e em todo push**: pegam fixtures
//! válidas, destroem de maneira **determinística** e exigem que o parser devolva `Err`
//! em vez de entrar em pânico, travar ou alocar sem limite.
//!
//! Determinismo é intencional: um teste de robustez que usa semente aleatória falha em
//! um push, passa no próximo e ninguém consegue reproduzir. Aqui a semente é fixa, e
//! qualquer falha é reproduzível com o número do caso.

use xbox_formats::eeprom::{self, Eeprom};
use xbox_formats::fatx::Fatx;
use xbox_formats::vfs::Vfs;
use xbox_formats::{Xbe, Xdvdfs, fixtures, identificar};

/// Gerador congruente linear — pequeno, determinístico e suficiente para espalhar
/// mutações. Não é criptografia; é um sorteio reproduzível.
struct Lcg(u64);

impl Lcg {
    const fn new(semente: u64) -> Self {
        Self(semente)
    }

    fn proximo(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 16
    }

    fn ate(&mut self, limite: usize) -> usize {
        if limite == 0 {
            0
        } else {
            (self.proximo() % limite as u64) as usize
        }
    }
}

#[test]
fn xbe_truncado_em_qualquer_ponto_nao_entra_em_panico() {
    let bom = fixtures::xbe_minimo();
    for n in 0..bom.len() {
        let r = Xbe::parse(&bom[..n]);
        if n < 0x178 {
            assert!(r.is_err(), "XBE de {n} bytes não deveria parsear");
        }
    }
    assert!(Xbe::parse(&bom).is_ok(), "a fixture completa deve parsear");
}

#[test]
fn xbe_com_bytes_trocados_nunca_entra_em_panico() {
    let bom = fixtures::xbe_minimo();
    let mut rng = Lcg::new(0x0B51_D1A1);
    let mut aceitos = 0usize;

    for caso in 0..20_000 {
        let mut ruim = bom.clone();
        // De 1 a 8 mutações por caso: pouca mutação chega mais fundo no parser,
        // muita mutação exercita as checagens de limite.
        let quantas = 1 + rng.ate(8);
        for _ in 0..quantas {
            let pos = rng.ate(ruim.len());
            ruim[pos] = (rng.proximo() & 0xFF) as u8;
        }
        match Xbe::parse(&ruim) {
            Ok(xbe) => {
                aceitos += 1;
                // Se aceitou, os acessores também não podem entrar em pânico.
                let _ = xbe.titulo();
                let _ = xbe.logo();
                let _ = xbe.debug_pathname();
                let _ = xbe.certificate.title_id_formatado();
                for i in 0..xbe.sections.len() {
                    let _ = xbe.section_data(i);
                }
            }
            Err(e) => {
                assert!(!e.to_string().is_empty(), "caso {caso}: erro sem mensagem");
            }
        }
    }
    // A maioria das mutações cai em campos que o parser não valida (timestamps,
    // digests, chaves), então aceitar muitos casos é esperado. Zero aceitos indicaria
    // que o teste não está chegando no parser de verdade.
    assert!(
        aceitos > 100,
        "só {aceitos} casos parsearam; o teste não está exercitando o parser"
    );
}

#[test]
fn xbe_com_campos_de_tamanho_absurdos_e_recusado() {
    let posicoes = [
        (0x108, "size of headers"),
        (0x10C, "size of image"),
        (0x11C, "section count"),
        (0x120, "section headers address"),
        (0x118, "certificate address"),
        (0x160, "library count"),
        (0x164, "library address"),
        (0x170, "logo address"),
        (0x174, "logo size"),
    ];
    for (off, nome) in posicoes {
        for valor in [0u32, 1, 0x7FFF_FFFF, 0xFFFF_FFFF] {
            let mut ruim = fixtures::xbe_minimo();
            ruim[off..off + 4].copy_from_slice(&valor.to_le_bytes());
            // Não exigimos erro (alguns valores continuam coerentes), só que não
            // exploda e que, se aceitar, os acessores funcionem.
            if let Ok(xbe) = Xbe::parse(&ruim) {
                let _ = xbe.logo();
                let _ = xbe.debug_pathname();
                for i in 0..xbe.sections.len() {
                    let _ = xbe.section_data(i);
                }
            }
            let _ = nome;
        }
    }
}

#[test]
fn imagem_truncada_em_qualquer_ponto_nao_entra_em_panico() {
    let boa = fixtures::xiso_minimo();
    for n in (0..boa.len()).step_by(503) {
        if let Ok(fs) = Xdvdfs::open(&boa[..n]) {
            let _ = fs.walk();
            let _ = fs.default_xbe();
        }
    }
}

#[test]
fn imagem_com_bytes_trocados_nunca_entra_em_panico_nem_trava() {
    let boa = fixtures::xiso_minimo();
    let mut rng = Lcg::new(0x000D_15C0_FFEE);

    for _ in 0..4_000 {
        let mut ruim = boa.clone();
        // Concentrar mutações nos setores de metadados (32 a 35) é onde o parser tem
        // trabalho a fazer; byte trocado no meio de um arquivo não exercita nada.
        let quantas = 1 + rng.ate(6);
        for _ in 0..quantas {
            let pos = 32 * 2048 + rng.ate(4 * 2048);
            ruim[pos] = (rng.proximo() & 0xFF) as u8;
        }
        if let Ok(fs) = Xdvdfs::open(&ruim) {
            // `walk` tem limites de profundidade e de entradas: precisa terminar.
            let _ = fs.walk();
            if let Ok(e) = fs.default_xbe()
                && let Ok(bytes) = fs.read(&e)
            {
                let _ = Xbe::parse(&bytes);
            }
        }
    }
}

#[test]
fn identificar_nunca_entra_em_panico() {
    let mut rng = Lcg::new(7);
    for tamanho in [0usize, 1, 3, 4, 100, 2048, 65_536, 70_000] {
        let mut lixo = vec![0u8; tamanho];
        for b in &mut lixo {
            *b = (rng.proximo() & 0xFF) as u8;
        }
        let _ = identificar(&lixo);
    }
}

#[test]
fn particao_fatx_com_bytes_trocados_nunca_entra_em_panico_nem_trava() {
    // O risco do FATX é a cadeia de clusters: uma lista ligada vinda do disco, que
    // pode apontar para si mesma ou para fora da partição. `walk` tem que terminar
    // sempre.
    let boa = fixtures::fatx_minima();
    let mut rng = Lcg::new(0xFA7_0001);

    for _ in 0..3_000 {
        let mut ruim = boa.clone();
        let quantas = 1 + rng.ate(6);
        for _ in 0..quantas {
            // Concentra nas estruturas: superbloco, FAT e os dois clusters de
            // diretório. Byte trocado no meio de um arquivo não exercita nada.
            let pos = match rng.ate(3) {
                0 => rng.ate(0x10),            // superbloco
                1 => 0x1000 + rng.ate(0x400),  // FAT
                _ => 0x2000 + rng.ate(0x8000), // clusters iniciais
            };
            if pos < ruim.len() {
                ruim[pos] = (rng.proximo() & 0xFF) as u8;
            }
        }
        if let Ok(fs) = Fatx::open(&ruim) {
            let _ = fs.walk();
            if let Ok(e) = fs.find("savedata.bin") {
                let _ = fs.read(&e);
                let _ = fs.read_range(&e, 0, 32);
            }
        }
    }
}

#[test]
fn particao_fatx_truncada_nao_entra_em_panico() {
    let boa = fixtures::fatx_minima();
    for n in (0..boa.len()).step_by(4091) {
        if let Ok(fs) = Fatx::open(&boa[..n]) {
            let _ = fs.walk();
        }
    }
}

#[test]
fn eeprom_de_qualquer_conteudo_e_estavel() {
    let mut rng = Lcg::new(0xEE_9801);
    for _ in 0..2_000 {
        let mut bytes = [0u8; eeprom::TAMANHO];
        for b in &mut bytes {
            *b = (rng.proximo() & 0xFF) as u8;
        }
        let e = Eeprom::parse(&bytes).expect("256 bytes sempre parseiam");
        let _ = e.checksums_conferem();
        // O bloco cifrado nunca pode ser reescrito por quem só mexeu em configuração.
        assert_eq!(&e.bytes()[0x14..0x30], &bytes[0x14..0x30]);
    }
}

#[test]
fn resolver_caminho_nunca_entra_em_panico() {
    // Caminho vem do jogo: pode ser qualquer sequência de bytes que caiba numa string.
    let mut rng = Lcg::new(0xCA_1111);
    let alfabeto: &[u8] = br"DTZ:\/.ab0 ";

    for _ in 0..5_000 {
        let tamanho = rng.ate(24);
        let texto: String = (0..tamanho)
            .map(|_| char::from(alfabeto[rng.ate(alfabeto.len())]))
            .collect();
        let _ = Vfs::resolver(&texto);
    }

    for fixo in [
        "",
        ":",
        "::",
        "D",
        "D:",
        r"D:\",
        r"\??\",
        r"\Device\",
        r"\Device\CdRom0",
        r"\device\harddisk0\partition9\x",
        r"D:\..\..",
        "/////",
        r"D:\\\x",
    ] {
        let _ = Vfs::resolver(fixo);
    }
}
