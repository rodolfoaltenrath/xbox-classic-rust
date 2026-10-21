//! Contrato público Xbox, efeitos guest e limites do perfil sem scheduler.
use xbox_formats::fixtures::xbe_com_imports;
use xbox_kernel::dispatch::{CRITICAL_SECTION_SIZE, Dispatcher, Erro, INITIAL_THREAD};
use xbox_kernel::imports;
use xbox_kernel::loader::{Image, STACK_BASE, STACK_SIZE};
use xbox_mem::{Acesso, Bus, GuestPhys, GuestVirt, Perm};

const SECTION: u32 = 0x12100;
const ORDINALS: [u32; 4] = [277, 291, 294, 306];

fn setup() -> (Bus, Dispatcher) {
    let image = xbe_com_imports(&[0xC3], &ORDINALS).unwrap();
    (
        Image::prepare_aot(&image).unwrap().image.bus,
        Dispatcher::new(),
    )
}

fn call(bus: &mut Bus, d: &mut Dispatcher, ordinal: u32, section: u32) -> Result<u32, Erro> {
    let esp = STACK_BASE + STACK_SIZE - 0x100;
    bus.escrever_u32(GuestVirt(esp), 0x11000).unwrap();
    bus.escrever_u32(GuestVirt(esp + 4), section).unwrap();
    let mut regs = [0; 8];
    regs[4] = esp;
    d.atender(bus, imports::trampoline(ordinal), &regs)
        .map(|r| r.eax)
}

fn bytes(bus: &Bus, at: u32, size: usize) -> Vec<u8> {
    let mut bytes = vec![0; size];
    bus.ler_bytes(GuestVirt(at), &mut bytes).unwrap();
    bytes
}

fn state(bus: &Bus) -> [u32; 3] {
    [16, 20, 24].map(|offset| bus.ler_u32(GuestVirt(SECTION + offset)).unwrap())
}

fn set_state(bus: &mut Bus, state: [u32; 3]) {
    for (offset, word) in [16, 20, 24].into_iter().zip(state) {
        bus.escrever_u32(GuestVirt(SECTION + offset), word).unwrap();
    }
}

#[test]
fn initialize_preenche_layout_xbox_de_28_bytes_e_preserva_guardas() {
    let (mut bus, mut d) = setup();
    assert_eq!(CRITICAL_SECTION_SIZE, 28);
    bus.escrever_bytes(GuestVirt(SECTION - 1), &[0xAA; 30])
        .unwrap();
    assert_eq!(call(&mut bus, &mut d, 291, SECTION).unwrap(), 0);
    let expected: Vec<u8> = [0x0004_0001u32, 0, SECTION + 8, SECTION + 8, u32::MAX, 0, 0]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    assert_eq!(bytes(&bus, SECTION, 28), expected);
    assert_eq!(bus.ler_u8(GuestVirt(SECTION - 1)).unwrap(), 0xAA);
    assert_eq!(bus.ler_u8(GuestVirt(SECTION + 28)).unwrap(), 0xAA);
}

#[test]
fn entrada_try_recursao_e_liberacao_atualizam_contagens_e_proprietario() {
    let (mut bus, mut d) = setup();
    call(&mut bus, &mut d, 291, SECTION).unwrap();
    let event = bytes(&bus, SECTION, 16);
    for (ordinal, eax, expected) in [
        (306, 1, [0, 1, INITIAL_THREAD]),
        (277, 0, [1, 2, INITIAL_THREAD]),
        (306, 1, [2, 3, INITIAL_THREAD]),
        (294, 0, [1, 2, INITIAL_THREAD]),
        (294, 0, [0, 1, INITIAL_THREAD]),
        (294, 0, [u32::MAX, 0, 0]),
        (277, 0, [0, 1, INITIAL_THREAD]),
        (294, 0, [u32::MAX, 0, 0]),
    ] {
        assert_eq!(call(&mut bus, &mut d, ordinal, SECTION).unwrap(), eax);
        assert_eq!(state(&bus), expected);
        assert_eq!(bytes(&bus, SECTION, 16), event);
    }
    assert_eq!(d.atendidas(), 9);
}

#[test]
fn try_ocupado_devolve_false_e_enter_recusa_sem_esperar_ou_mudar_memoria() {
    let (mut bus, mut d) = setup();
    call(&mut bus, &mut d, 291, SECTION).unwrap();
    set_state(&mut bus, [0, 1, 2]);
    let before = bytes(&bus, SECTION, 28);
    assert_eq!(call(&mut bus, &mut d, 306, SECTION).unwrap(), 0);
    let error = call(&mut bus, &mut d, 277, SECTION).unwrap_err();
    assert!(matches!(error, Erro::VarianteNaoImplementada { .. }));
    let message = error.to_string();
    for fragment in [
        "RtlEnterCriticalSection",
        "277",
        "0x00011000",
        "0x00012100",
        "scheduler",
    ] {
        assert!(message.contains(fragment), "{message}");
    }
    assert!(matches!(
        call(&mut bus, &mut d, 294, SECTION),
        Err(Erro::ArgumentoInvalido { .. })
    ));
    assert_eq!(bytes(&bus, SECTION, 28), before);
    assert_eq!((d.atendidas(), d.recusadas()), (2, 1));
}

#[test]
fn espera_evento_e_estouro_sao_variantes_sem_efeitos_parciais() {
    let (mut bus, mut d) = setup();
    for (offset, value) in [(4, 1), (8, SECTION + 0x100), (12, SECTION + 0x100)] {
        call(&mut bus, &mut d, 291, SECTION).unwrap();
        bus.escrever_u32(GuestVirt(SECTION + offset), value)
            .unwrap();
        let before = bytes(&bus, SECTION, 28);
        for ordinal in [277, 294, 306] {
            assert!(matches!(
                call(&mut bus, &mut d, ordinal, SECTION),
                Err(Erro::VarianteNaoImplementada { .. })
            ));
            assert_eq!(bytes(&bus, SECTION, 28), before);
        }
    }
    call(&mut bus, &mut d, 291, SECTION).unwrap();
    for counts in [
        [1, 1, INITIAL_THREAD],
        [i32::MAX as u32 - 1, i32::MAX as u32, INITIAL_THREAD],
    ] {
        set_state(&mut bus, counts);
        let before = bytes(&bus, SECTION, 28);
        for ordinal in [277, 306] {
            assert!(matches!(
                call(&mut bus, &mut d, ordinal, SECTION),
                Err(Erro::VarianteNaoImplementada { .. })
            ));
            assert_eq!(bytes(&bus, SECTION, 28), before);
        }
    }
    // Uma liberação que teria de acordar um esperador também deve recusar.
    set_state(&mut bus, [1, 1, INITIAL_THREAD]);
    assert!(matches!(
        call(&mut bus, &mut d, 294, SECTION),
        Err(Erro::VarianteNaoImplementada { .. })
    ));
    assert_eq!(state(&bus), [1, 1, INITIAL_THREAD]);
}

#[test]
fn estrutura_nao_inicializada_ou_incoerente_e_leave_livre_sao_erros() {
    let (mut bus, mut d) = setup();
    for ordinal in [277, 294, 306] {
        assert!(matches!(
            call(&mut bus, &mut d, ordinal, SECTION),
            Err(Erro::ArgumentoInvalido { .. })
        ));
    }
    call(&mut bus, &mut d, 291, SECTION).unwrap();
    for counts in [
        [u32::MAX, 0, 1],
        [0, 0, 0],
        [0, 2, 1],
        [0, u32::MAX, 1],
        [u32::MAX - 1, 0, 0],
    ] {
        set_state(&mut bus, counts);
        let before = bytes(&bus, SECTION, 28);
        for ordinal in [277, 294, 306] {
            assert!(matches!(
                call(&mut bus, &mut d, ordinal, SECTION),
                Err(Erro::ArgumentoInvalido { .. })
            ));
            assert_eq!(bytes(&bus, SECTION, 28), before);
        }
    }
    call(&mut bus, &mut d, 291, SECTION).unwrap();
    assert!(matches!(
        call(&mut bus, &mut d, 294, SECTION),
        Err(Erro::ArgumentoInvalido { .. })
    ));
    assert_eq!(state(&bus), [u32::MAX, 0, 0]);
}

#[test]
fn ponteiros_invalidos_e_pagina_parcial_nao_causam_store_parcial() {
    let (mut bus, mut d) = setup();
    bus.escrever_bytes(GuestVirt(0x12FF0), &[0xAA; 16]).unwrap();
    for pointer in [0, u32::MAX - 15, 0x5000_0000, 0x11000, 0x12FF0] {
        for ordinal in ORDINALS {
            assert!(matches!(
                call(&mut bus, &mut d, ordinal, pointer),
                Err(Erro::ArgumentoInvalido { .. })
            ));
            assert_eq!(bytes(&bus, 0x12FF0, 16), [0xAA; 16]);
        }
    }
    assert_eq!(d.atendidas(), 0);
}

#[test]
fn mmio_e_ram_somente_leitura_sao_recusados() {
    let (mut bus, mut d) = setup();
    bus.mapear(GuestVirt(0x5000_0000), GuestPhys(0xFD00_0000), 1, Perm::RW)
        .unwrap();
    call(&mut bus, &mut d, 291, SECTION).unwrap();
    let before = bytes(&bus, SECTION, 28);
    let page = bus.traduzir(GuestVirt(0x12000), Acesso::Leitura).unwrap();
    bus.mapear(GuestVirt(0x12000), page, 1, Perm::RO).unwrap();
    for ordinal in ORDINALS {
        for pointer in [SECTION, 0x5000_0000] {
            assert!(matches!(
                call(&mut bus, &mut d, ordinal, pointer),
                Err(Erro::ArgumentoInvalido { .. })
            ));
        }
    }
    assert_eq!(bytes(&bus, SECTION, 28), before);
}

#[test]
fn estrutura_atravessa_duas_paginas_e_novo_dispatcher_le_o_estado_guest() {
    let (mut bus, mut d) = setup();
    bus.mapear(GuestVirt(0x13000), GuestPhys(0x13000), 1, Perm::RW)
        .unwrap();
    let pointer = 0x12FF0;
    call(&mut bus, &mut d, 291, pointer).unwrap();
    call(&mut bus, &mut d, 277, pointer).unwrap();
    let mut fresh = Dispatcher::new();
    assert_eq!(call(&mut bus, &mut fresh, 306, pointer).unwrap(), 1);
    assert_eq!(bus.ler_u32(GuestVirt(pointer + 20)).unwrap(), 2);
    call(&mut bus, &mut fresh, 294, pointer).unwrap();
    call(&mut bus, &mut fresh, 294, pointer).unwrap();
    assert_eq!(bus.ler_u32(GuestVirt(pointer + 16)).unwrap(), u32::MAX);
}

#[test]
fn secoes_distintas_nao_compartilham_contagem_e_reinicializar_reconstroi_o_objeto() {
    let (mut bus, mut d) = setup();
    let other = SECTION + 32;
    for pointer in [SECTION, other] {
        call(&mut bus, &mut d, 291, pointer).unwrap();
    }
    call(&mut bus, &mut d, 277, SECTION).unwrap();
    assert_eq!(bus.ler_u32(GuestVirt(other + 24)).unwrap(), 0);
    call(&mut bus, &mut d, 291, SECTION).unwrap();
    assert_eq!(state(&bus), [u32::MAX, 0, 0]);
    assert_eq!(call(&mut bus, &mut d, 306, other).unwrap(), 1);
    assert_eq!(state(&bus), [u32::MAX, 0, 0]);
}
