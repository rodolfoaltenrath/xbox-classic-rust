use xbox_core::error::SnapshotError;
use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_formats::eeprom::{Eeprom, Idioma, PadraoDeVideo};
use xbox_formats::fixtures::xbe_com_imports;
use xbox_kernel::dispatch::{
    Dispatcher, EepromSource, EepromState, Erro, REG_BINARY, STATUS_ACCESS_VIOLATION,
    STATUS_BUFFER_TOO_SMALL, STATUS_SUCCESS, XC_MAX_ALL,
};
use xbox_kernel::imports;
use xbox_kernel::loader::{Image, STACK_BASE, STACK_SIZE};
use xbox_mem::{Acesso, Bus, GuestPhys, GuestVirt, Perm};

const VALUE: u32 = STACK_BASE + 0x1000;
const TYPE: u32 = STACK_BASE + 0x2000;
const LENGTH: u32 = TYPE + 4;
const ARGS: [u32; 5] = [XC_MAX_ALL, TYPE, VALUE, 256, LENGTH];

fn setup() -> (Bus, Dispatcher) {
    let image = xbe_com_imports(&[0xC3], &[24]).unwrap();
    (
        Image::prepare_aot(&image).unwrap().image.bus,
        Dispatcher::new(),
    )
}

fn call(bus: &mut Bus, d: &mut Dispatcher, args: [u32; 5]) -> Result<u32, Erro> {
    let esp = STACK_BASE + STACK_SIZE - 0x100;
    bus.escrever_u32(GuestVirt(esp), 0x11000).unwrap();
    for (i, arg) in args.into_iter().enumerate() {
        bus.escrever_u32(GuestVirt(esp + 4 + i as u32 * 4), arg)
            .unwrap();
    }
    let mut regs = [0; 8];
    regs[4] = esp;
    d.atender(bus, imports::trampoline(24), &regs)
        .map(|r| r.eax)
}

fn read(bus: &Bus, address: u32, len: usize) -> Vec<u8> {
    let mut bytes = vec![0; len];
    bus.ler_bytes(GuestVirt(address), &mut bytes).unwrap();
    bytes
}

fn guards(bus: &mut Bus) {
    bus.escrever_bytes(GuestVirt(VALUE - 1), &[0xAA; 258])
        .unwrap();
    bus.escrever_bytes(GuestVirt(TYPE), &[0xBB; 8]).unwrap();
}

fn unchanged(bus: &Bus) {
    assert_eq!(read(bus, VALUE - 1, 258), [0xAA; 258]);
    assert_eq!(read(bus, TYPE, 8), [0xBB; 8]);
}

fn saved(d: &Dispatcher) -> Vec<u8> {
    let mut out = SnapshotWriter::new();
    d.save(&mut out);
    out.finish()
}

#[test]
fn imagem_padrao_reutiliza_gerador_com_identidade_e_configuracao_explicitas() {
    let (_, d) = setup();
    assert_eq!(d.eeprom().source(), EepromSource::SyntheticV1);
    let expected = Eeprom::sintetica(PadraoDeVideo::NtscM, Idioma::Portugues).bytes();
    assert_eq!(*d.eeprom().bytes(), expected);
    let parsed = Eeprom::parse(&expected).unwrap();
    assert_eq!(parsed.serial, "OBSIDIAN0001");
    assert_eq!(parsed.mac, [2, 0, 0, 0x0B, 0x51, 0xD1]);
    assert!(parsed.checksums_conferem());
    assert_eq!(parsed.idioma, Idioma::Portugues);
    assert_eq!(parsed.padrao_de_video, PadraoDeVideo::NtscM);
}

#[test]
fn consulta_integral_copia_imagem_e_preenche_tipo_tamanho_sem_exceder_saidas() {
    let (mut bus, mut d) = setup();
    guards(&mut bus);
    assert_eq!(call(&mut bus, &mut d, ARGS).unwrap(), STATUS_SUCCESS);
    assert_eq!(read(&bus, VALUE, 256), *d.eeprom().bytes());
    assert_eq!(bus.ler_u32(GuestVirt(TYPE)).unwrap(), REG_BINARY);
    assert_eq!(bus.ler_u32(GuestVirt(LENGTH)).unwrap(), 256);
    assert_eq!(bus.ler_u8(GuestVirt(VALUE - 1)).unwrap(), 0xAA);
    assert_eq!(bus.ler_u8(GuestVirt(VALUE + 256)).unwrap(), 0xAA);
    // O buffer guest é uma cópia: alterá-lo não reescreve a EEPROM.
    let state = d.eeprom().clone();
    bus.escrever_bytes(GuestVirt(VALUE), &[0; 256]).unwrap();
    assert_eq!(d.eeprom(), &state);
    call(&mut bus, &mut d, ARGS).unwrap();
    assert_eq!(read(&bus, VALUE, 256), *state.bytes());
}

#[test]
fn bytes_fornecidos_sao_preservados_sem_normalizar_checksum_ou_bloco_cifrado() {
    let (mut bus, _) = setup();
    let bytes = std::array::from_fn(|i| i as u8 ^ 0xA5);
    let mut d = Dispatcher::with_eeprom(EepromState::supplied(bytes));
    call(&mut bus, &mut d, ARGS).unwrap();
    assert_eq!(d.eeprom().source(), EepromSource::Supplied);
    assert_eq!(read(&bus, VALUE, 256), bytes);
}

#[test]
fn buffer_curto_publica_apenas_tamanho_e_permite_sondagem_com_type_value_nulos() {
    let (mut bus, mut d) = setup();
    for capacity in [0, 1, 255] {
        guards(&mut bus);
        let mut args = ARGS;
        args[3] = capacity;
        assert_eq!(
            call(&mut bus, &mut d, args).unwrap(),
            STATUS_BUFFER_TOO_SMALL
        );
        assert_eq!(read(&bus, VALUE - 1, 258), [0xAA; 258]);
        assert_eq!(bus.ler_u32(GuestVirt(TYPE)).unwrap(), 0xBBBBBBBB);
        assert_eq!(bus.ler_u32(GuestVirt(LENGTH)).unwrap(), 256);
    }
    assert_eq!(
        call(&mut bus, &mut d, [XC_MAX_ALL, 0, 0, 0, LENGTH]).unwrap(),
        STATUS_BUFFER_TOO_SMALL
    );
}

#[test]
fn capacidade_maior_que_imagem_nao_amplia_faixa_escrita() {
    let (mut bus, mut d) = setup();
    let mut args = ARGS;
    args[2] = STACK_BASE + STACK_SIZE - 256;
    args[3] = u32::MAX;
    assert_eq!(call(&mut bus, &mut d, args).unwrap(), STATUS_SUCCESS);
    assert_eq!(read(&bus, args[2], 256), *d.eeprom().bytes());
}

#[test]
fn indices_sem_suporte_recusam_com_contexto_e_nao_tocam_nenhuma_saida() {
    let (mut bus, mut d) = setup();
    for index in [0, 7, 0x100, 0xFFFE, u32::MAX] {
        guards(&mut bus);
        let mut args = ARGS;
        args[0] = index;
        let error = call(&mut bus, &mut d, args).unwrap_err();
        assert!(matches!(
            error,
            Erro::VarianteNaoImplementada {
                ordinal: 24,
                chamador: 0x11000,
                ..
            }
        ));
        assert!(error.to_string().contains("ExQueryNonVolatileSetting"));
        unchanged(&bus);
    }
    assert_eq!(d.recusadas(), 5);
    assert_eq!(d.atendidas(), 0);
}

#[test]
fn qualquer_saida_invalida_impede_todas_as_escritas_inclusive_tamanho() {
    let (mut bus, mut d) = setup();
    for index in [1, 2, 4] {
        for pointer in [
            0,
            u32::MAX - 1,
            0x50000000,
            0x11000,
            STACK_BASE + STACK_SIZE - 2,
        ] {
            guards(&mut bus);
            let mut args = ARGS;
            args[index] = pointer;
            assert_eq!(
                call(&mut bus, &mut d, args).unwrap(),
                STATUS_ACCESS_VIOLATION
            );
            unchanged(&bus);
        }
    }
    assert_eq!(
        call(&mut bus, &mut d, [XC_MAX_ALL, 0, 0, 0, 0]).unwrap(),
        STATUS_ACCESS_VIOLATION
    );
}

#[test]
fn mmio_em_qualquer_saida_e_recusado_sem_descarte_silencioso() {
    let (mut bus, mut d) = setup();
    bus.mapear(GuestVirt(0x50000000), GuestPhys(0xFD000000), 1, Perm::RW)
        .unwrap();
    for index in [1, 2, 4] {
        guards(&mut bus);
        let mut args = ARGS;
        args[index] = 0x50000000;
        assert_eq!(
            call(&mut bus, &mut d, args).unwrap(),
            STATUS_ACCESS_VIOLATION
        );
        unchanged(&bus);
    }
}

#[test]
fn segunda_pagina_ro_da_saida_nao_deixa_primeira_pagina_parcial() {
    let (mut bus, mut d) = setup();
    guards(&mut bus);
    let output = VALUE + 3 * 4096 - 100;
    bus.escrever_bytes(GuestVirt(output), &[0xCC; 256]).unwrap();
    let page = VALUE + 3 * 4096;
    let frame = bus.traduzir(GuestVirt(page), Acesso::Leitura).unwrap();
    bus.mapear(GuestVirt(page), frame, 1, Perm::RO).unwrap();
    let mut args = ARGS;
    args[2] = output;
    assert_eq!(
        call(&mut bus, &mut d, args).unwrap(),
        STATUS_ACCESS_VIOLATION
    );
    assert_eq!(read(&bus, output, 256), [0xCC; 256]);
    unchanged(&bus);
}

#[test]
fn saidas_sobrepostas_seguem_ordem_de_store_declarada_do_perfil() {
    let (mut bus, mut d) = setup();
    call(&mut bus, &mut d, [XC_MAX_ALL, VALUE, VALUE, 256, VALUE]).unwrap();
    assert_eq!(bus.ler_u32(GuestVirt(VALUE)).unwrap(), 256);
    assert_eq!(read(&bus, VALUE + 4, 252), d.eeprom().bytes()[4..]);
}

#[test]
fn snapshot_preserva_bytes_origem_e_ignora_perfil_inicial_da_sessao_de_destino() {
    for state in [
        EepromState::supplied(std::array::from_fn(|i| i as u8)),
        EepromState::synthetic(PadraoDeVideo::PalI, Idioma::Ingles),
    ] {
        let d = Dispatcher::with_eeprom(state.clone());
        let snapshot = saved(&d);
        assert_eq!(snapshot, saved(&d));
        let mut restored = Dispatcher::new();
        let mut input = SnapshotReader::new(&snapshot).unwrap();
        restored.load(&mut input).unwrap();
        assert_eq!(input.remaining(), 0);
        assert_eq!(restored.eeprom(), &state);
        assert_eq!(saved(&restored), snapshot);
    }
}

#[test]
fn snapshot_kernel_v1_migra_para_eeprom_padrao_preservando_contadores() {
    let mut out = SnapshotWriter::new();
    out.section("KRNL", 1);
    out.u64(123);
    out.u64(4);
    out.section("KMUT", 1);
    out.u32(8);
    out.u32(1);
    out.u32(4);
    out.u32(1);
    out.u32(1);
    let bytes = out.finish();
    let mut d = Dispatcher::with_eeprom(EepromState::supplied([0xAA; 256]));
    let mut input = SnapshotReader::new(&bytes).unwrap();
    d.load(&mut input).unwrap();
    assert_eq!(input.remaining(), 0);
    assert_eq!(d.atendidas(), 123);
    assert_eq!(d.recusadas(), 4);
    assert_eq!(d.eeprom(), &EepromState::default());
    assert_eq!(d.mutant(4).unwrap().owner, Some(1));
}

#[test]
fn snapshot_eeprom_recusa_versao_origem_invalida_e_truncamento() {
    for (version, source) in [(0, 1), (2, 1), (1, 0), (1, 3)] {
        let mut out = SnapshotWriter::new();
        out.section("EEPR", version);
        out.u8(source);
        out.raw(&[0; 256]);
        let bytes = out.finish();
        let mut state = EepromState::default();
        let before = state.clone();
        assert!(matches!(
            state.load(&mut SnapshotReader::new(&bytes).unwrap()),
            Err(SnapshotError::BadValue { .. } | SnapshotError::SectionTooNew { .. })
        ));
        assert_eq!(state, before);
    }
    let mut out = SnapshotWriter::new();
    EepromState::default().save(&mut out);
    let bytes = out.finish();
    assert!(
        EepromState::default()
            .load(&mut SnapshotReader::new(&bytes[..bytes.len() - 1]).unwrap())
            .is_err()
    );
}
