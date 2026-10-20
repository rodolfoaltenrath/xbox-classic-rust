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
    state.fs_base = 0x7FFD0000;
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
fn snapshot_v2_preserva_rep_e_migra_v1_sem_continuacao() {
    use xbox_recomp::runtime::RepeatState;
    let mut state = State::new(ENTRY_POINT);
    state.repeat = Some(RepeatState {
        pc: ENTRY_POINT,
        eflags: 0xAD7,
    });
    let mut writer = SnapshotWriter::new();
    state.save(&mut writer);
    let bytes = writer.finish();
    let mut restored = State::new(0);
    restored
        .load(&mut SnapshotReader::new(&bytes).unwrap())
        .unwrap();
    assert_eq!(restored, state);
    for end in 0..bytes.len() {
        if let Ok(mut reader) = SnapshotReader::new(&bytes[..end]) {
            assert!(State::new(0).load(&mut reader).is_err());
        }
    }
    // Construção do payload v1 independente da serialização atual.
    let mut writer = SnapshotWriter::new();
    writer.section("AOTC", 1);
    for r in state.regs {
        writer.u32(r);
    }
    writer.u32(state.eip);
    writer.u32(state.eflags);
    writer.u64(state.retired);
    writer.bool(state.returned);
    let old = writer.finish();
    restored
        .load(&mut SnapshotReader::new(&old).unwrap())
        .unwrap();
    state.repeat = None;
    assert_eq!(restored, state);
}

#[test]
fn snapshot_v3_migra_v2_com_fs_zero_e_preserva_rep() {
    use xbox_recomp::runtime::RepeatState;
    let mut writer = SnapshotWriter::new();
    writer.section("AOTC", 2);
    for r in 0..8 {
        writer.u32(r);
    }
    writer.u32(ENTRY_POINT);
    writer.u32(0x202);
    writer.u64(37);
    writer.bool(false);
    writer.bool(true);
    writer.u32(ENTRY_POINT);
    writer.u32(0xAD7);
    let bytes = writer.finish();
    let mut restored = State::new(0);
    restored.fs_base = 0xDEAD0000;
    restored
        .load(&mut SnapshotReader::new(&bytes).unwrap())
        .unwrap();
    assert_eq!(restored.fs_base, 0);
    assert_eq!(restored.retired, 37);
    assert_eq!(
        restored.repeat,
        Some(RepeatState {
            pc: ENTRY_POINT,
            eflags: 0xAD7
        })
    );
}

#[test]
fn tls_malformado_imports_invalidos_e_codigo_gravavel_sao_recusados() {
    assert!(Session::load(&xbox_formats::fixtures::xbe_minimo()).is_err());
    let mut bytes = xbe_com_codigo(&[0xC3]).unwrap();
    bytes[0x300..0x304].copy_from_slice(&7u32.to_le_bytes());
    assert!(Session::load(&bytes).is_err());
}
