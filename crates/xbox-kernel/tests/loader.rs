use xbox_formats::{
    Xbe,
    fixtures::{xbe_com_codigo, xbe_minimo},
};
use xbox_kernel::loader::{Image, Mode};
use xbox_mem::{Acesso, GuestVirt};

fn image_with_imports() -> Vec<u8> {
    let mut image = xbe_minimo();
    for (index, value) in [0x80000008u32, 0x800000A5, 0].into_iter().enumerate() {
        image[0x2000 + index * 4..0x2004 + index * 4].copy_from_slice(&value.to_le_bytes());
    }
    image
}

#[test]
fn t14_fixture_verifica_bytes_imports_bss_alias_e_pagina_ausente() {
    let mut bytes = image_with_imports();
    bytes[0x338 + 8..0x338 + 12].copy_from_slice(&0x400u32.to_le_bytes());
    let image = Image::load(&bytes, Mode::Inspect).unwrap();
    let report = image.verify(&bytes).unwrap();
    assert_eq!(report.sections, 2);
    assert_eq!(report.pages, 3);
    assert_eq!(report.imports, [8, 165]);
    assert_eq!(report.entry, 0x11000);
    assert_eq!(report.unmapped_probe, 0);
    assert!(
        image
            .bus
            .traduzir(GuestVirt(0x11000), Acesso::Escrita)
            .is_err()
    );
    let physical = image
        .bus
        .traduzir(GuestVirt(0x11000), Acesso::Leitura)
        .unwrap();
    assert_ne!(physical.0, 0x11000);
}

#[test]
fn inspecao_aceita_rwx_mas_aot_recusa_inclusive_pagina_compartilhada() {
    let mut bytes = xbe_com_codigo(&[0x90, 0x90, 0x90, 0xC3]).unwrap();
    bytes[0x300..0x304].copy_from_slice(&7u32.to_le_bytes());
    assert!(
        Image::load(&bytes, Mode::Inspect)
            .unwrap()
            .verify(&bytes)
            .is_ok()
    );
    assert!(Image::load(&bytes, Mode::Aot).is_err());
    let mut bytes = xbe_com_codigo(&[0x90, 0xC3]).unwrap();
    bytes[0x33C..0x340].copy_from_slice(&0x11020u32.to_le_bytes());
    assert!(Image::load(&bytes, Mode::Inspect).is_ok());
    assert!(Image::load(&bytes, Mode::Aot).is_err());
    let bytes = xbe_com_codigo(&[0x90, 0xC3]).unwrap();
    let image = Image::load(&bytes, Mode::Aot).unwrap();
    assert_eq!(image.bus.ler_u8(xbox_mem::BOOT_DMA_ADDRESS).unwrap(), 0);
    let code = image
        .bus
        .traduzir(GuestVirt(0x11000), Acesso::Leitura)
        .unwrap();
    assert!(image.bus.ler_u8(GuestVirt(0x80000000 + code.0)).is_err());
}

#[test]
fn verificador_detecta_corrupcao_de_bytes_imports_e_bss() {
    let bytes = image_with_imports();
    let mut image = Image::load(&bytes, Mode::Inspect).unwrap();
    let physical = image
        .bus
        .traduzir(GuestVirt(0x12000), Acesso::Leitura)
        .unwrap();
    image
        .bus
        .escrever_u32(GuestVirt(0x80000000 + physical.0), 0x80000009)
        .unwrap();
    assert!(image.verify(&bytes).is_err());
    assert_eq!(image.imports_from_memory().unwrap(), [9, 165]);
    let mut bytes = bytes;
    bytes[0x340..0x344].copy_from_slice(&0x80u32.to_le_bytes());
    let mut image = Image::load(&bytes, Mode::Inspect).unwrap();
    let physical = image
        .bus
        .traduzir(GuestVirt(0x12040), Acesso::Leitura)
        .unwrap();
    image
        .bus
        .escrever_u8(GuestVirt(0x80000000 + physical.0), 1)
        .unwrap();
    assert!(image.verify(&bytes).is_err());
}

#[test]
fn layout_invalido_e_recursos_excessivos_sao_erros_antes_de_mapear() {
    for (field, value) in [
        (0x304, 0x10000u32),
        (0x308, u32::MAX),
        (0x30C, 0xFFFFFFF0),
        (0x33C, 0x11000),
    ] {
        let mut bytes = image_with_imports();
        bytes[field..field + 4].copy_from_slice(&value.to_le_bytes());
        assert!(Image::load(&bytes, Mode::Inspect).is_err());
    }
    let mut bytes = image_with_imports();
    bytes[0x10C..0x110].copy_from_slice(&0x20000000u32.to_le_bytes());
    bytes[0x308..0x30C].copy_from_slice(&0x10000000u32.to_le_bytes());
    assert!(Image::load(&bytes, Mode::Inspect).is_err());
}

#[test]
#[ignore = "mídia local: definir OBSIDIAN_MIDIA_REAL e executar --ignored"]
fn t14_midia_real_confere_todas_as_secoes_e_ordinais() {
    use xbox_formats::{Media, Xdvdfs};
    let path = std::env::var_os("OBSIDIAN_MIDIA_REAL").expect("defina OBSIDIAN_MIDIA_REAL");
    let media = Media::open(path).unwrap();
    let bytes = if *media.read_at(0, 4).unwrap() == xbox_formats::xbe::MAGIC {
        assert!(media.len() < 128 * 1024 * 1024);
        media.read_at(0, media.len() as usize).unwrap().into_owned()
    } else {
        let disc = Xdvdfs::open(media).unwrap();
        let entry = disc.default_xbe().unwrap();
        assert!(entry.size < 128 * 1024 * 1024);
        disc.read(&entry).unwrap().into_owned()
    };
    let xbe = Xbe::parse(&bytes).unwrap();
    let report = Image::load(&bytes, Mode::Inspect)
        .unwrap()
        .verify(&bytes)
        .unwrap();
    assert_eq!(report.sections, xbe.sections.len());
    assert_eq!(report.imports, xbe.kernel_imports().unwrap());
    println!(
        "T14 real: {} seções, {} ordinais, {} bytes",
        report.sections,
        report.imports.len(),
        report.checked_bytes
    );
}
