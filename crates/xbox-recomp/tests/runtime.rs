use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_formats::fixtures::{ENTRY_POINT, xbe_com_codigo};
use xbox_mem::{Acesso, GuestVirt};
use xbox_recomp::runtime::{ESP, RETURN_SENTINEL, Session, State, arithmetic_flags, logic_flags};

#[test]
fn flags_cobrem_carry_overflow_sinal_paridade_e_auxiliary_carry() {
    // Resultados esperados especificados independentemente do emissor.
    for (a, b, result, sub, expected) in [
        (u32::MAX, 1, 0, false, 0x257),
        (0x7FFF_FFFF, 1, 0x8000_0000, false, 0xA96),
        (0, 1, u32::MAX, true, 0x297),
        (0x8000_0000, 1, 0x7FFF_FFFF, true, 0xA16),
        (5, 5, 0, true, 0x246),
    ] {
        assert_eq!(arithmetic_flags(0x202, a, b, result, sub), expected);
    }
    assert_eq!(logic_flags(0xAD7, 0), 0x246);
    assert_eq!(logic_flags(0x202, 1), 0x202);
}

#[test]
fn loader_mapeia_codigo_rx_dados_rw_e_pilha_com_retorno() {
    let session = Session::load(&xbe_com_codigo(&[0xC3]).unwrap()).unwrap();
    assert!(
        session
            .bus
            .traduzir(GuestVirt(ENTRY_POINT), Acesso::Execucao)
            .is_ok()
    );
    assert!(
        session
            .bus
            .traduzir(GuestVirt(ENTRY_POINT), Acesso::Escrita)
            .is_err()
    );
    assert!(
        session
            .bus
            .traduzir(GuestVirt(0x12020), Acesso::Escrita)
            .is_ok()
    );
    assert!(
        session
            .bus
            .traduzir(GuestVirt(0x12020), Acesso::Execucao)
            .is_err()
    );
    assert_eq!(
        session
            .bus
            .ler_u32(GuestVirt(session.state.regs[ESP]))
            .unwrap(),
        RETURN_SENTINEL
    );
    assert_eq!(session.bus.ler_u32(GuestVirt(0x12020)).unwrap(), 0);
}

#[test]
fn snapshot_de_cpu_e_deterministico_e_rejeita_truncamento() {
    let mut state = State::new(ENTRY_POINT);
    state.regs[0] = 0x1234_5678;
    state.retired = 9;
    let mut writer = SnapshotWriter::new();
    state.save(&mut writer);
    let bytes = writer.finish();
    let mut restored = State::new(0);
    restored
        .load(&mut SnapshotReader::new(&bytes).unwrap())
        .unwrap();
    assert_eq!(state, restored);
    let mut writer = SnapshotWriter::new();
    restored.save(&mut writer);
    assert_eq!(bytes, writer.finish());
    for end in 0..bytes.len() {
        if let Ok(mut reader) = SnapshotReader::new(&bytes[..end]) {
            assert!(State::new(0).load(&mut reader).is_err());
        }
    }
}

#[test]
fn tls_imports_e_codigo_gravavel_sao_recusados() {
    assert!(Session::load(&xbox_formats::fixtures::xbe_minimo()).is_err());
    let mut bytes = xbe_com_codigo(&[0xC3]).unwrap();
    bytes[0x300..0x304].copy_from_slice(&7u32.to_le_bytes());
    assert!(Session::load(&bytes).is_err());
}
