use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_formats::fixtures::xbe_com_codigo;
use xbox_recomp::{
    Error,
    analysis::analyze,
    ir::Operation,
    runtime::{CpuProfile, State},
};

#[test]
fn rdmsr_tem_ir_propria_sem_indice_constante_obrigatorio() {
    let p = analyze(&xbe_com_codigo(&[0x0F, 0x32, 0xC3]).unwrap()).unwrap();
    assert_eq!(p.instructions[&0x11000].operation, Operation::ReadMsr);
    assert_eq!(p.instructions[&0x11000].next, 0x11002);
}

#[test]
fn rdmsr_recusa_lock_rep_e_wrmsr_ainda_sem_modelo() {
    for code in [
        &[0xF0, 0x0F, 0x32, 0xC3][..],
        &[0xF2, 0x0F, 0x32, 0xC3][..],
        &[0xF3, 0x0F, 0x32, 0xC3][..],
        &[0x0F, 0x30, 0xC3][..],
    ] {
        assert!(analyze(&xbe_com_codigo(code).unwrap()).is_err());
    }
}

#[test]
fn perfil_expoe_configuracao_nominal_e_tsc_do_clock_sem_efeitos() {
    let mut state = State::new(0x11000);
    state.clock.advance(0xABCD_EF01_2345_6789);
    let before = state.clone();
    assert_eq!(state.cpu_profile, CpuProfile::XboxRetailV1);
    // Esperado independente: bus 133 MHz nos bits 19:18, ratio 5,5 nos 25:22.
    assert_eq!(state.read_msr(0x2A).unwrap(), 0x0104_0000);
    assert_eq!(state.read_msr(0x10).unwrap(), 0xABCD_EF01_2345_6789);
    for index in [0, 0x11, 0x1B, 0x8000_002A, u32::MAX] {
        assert!(
            matches!(state.read_msr(index), Err(Error::UnsupportedMsr { pc:0x11000, index:i }) if i == index)
        );
    }
    assert_eq!(state, before);
}

#[test]
fn snapshot_atual_preserva_perfil_e_rejeita_id_desconhecido() {
    let mut state = State::new(0x11000);
    state.clock.advance(1234567890123);
    state.breakpoint = Some(0x10FFF);
    let mut out = SnapshotWriter::new();
    state.save(&mut out);
    let bytes = out.finish();
    assert_eq!(u32::from_le_bytes(bytes[16..20].try_into().unwrap()), 10);
    let mut restored = State::new(0);
    let mut reader = SnapshotReader::new(&bytes).unwrap();
    restored.load(&mut reader).unwrap();
    assert_eq!(reader.remaining(), 0);
    assert_eq!(restored, state);
    let mut bad_version = bytes.clone();
    bad_version[16..20].copy_from_slice(&0u32.to_le_bytes());
    assert!(
        State::new(0)
            .load(&mut SnapshotReader::new(&bad_version).unwrap())
            .is_err()
    );
    for id in [0u32, 2, u32::MAX] {
        let mut bad = bytes.clone();
        // O id do perfil vem antes de MXCSR (4), XMM (8 × 16) e a pilha x87 (TOP, tags e
        // 8 × 12 bytes): 4 + 128 + 104 bytes depois dele.
        let tail = bad.len() - (4 + 4 + 128 + 104 + 3);
        bad[tail..tail + 4].copy_from_slice(&id.to_le_bytes());
        assert!(matches!(
            State::new(0).load(&mut SnapshotReader::new(&bad).unwrap()),
            Err(xbox_core::SnapshotError::BadValue {
                section: "AOTC",
                ..
            })
        ));
    }
}

#[test]
fn snapshots_v1_a_v7_migram_para_perfil_guest_e_sem_estado_de_ponto_flutuante() {
    for version in 1..=7 {
        // Payload antigo construído independentemente de State::save.
        let mut out = SnapshotWriter::new();
        out.section("AOTC", version);
        for r in 0..8 {
            out.u32(r);
        }
        out.u32(0x11000);
        out.u32(0xAD7);
        out.u64(999);
        out.bool(false);
        if version >= 2 {
            out.bool(false);
        }
        if version >= 3 {
            out.u32(0x7FFD0000);
        }
        if version >= 4 {
            out.u32(1);
            let mut clock = xbox_core::Clock::new();
            clock.advance(12345);
            clock.save(&mut out);
        }
        if version >= 5 {
            out.bool(false);
        }
        if version >= 6 {
            out.bool(true);
            out.u32(0x10FFF);
        }
        if version >= 7 {
            out.u32(1); // XboxRetailV1
        }
        let bytes = out.finish();
        let mut state = State::new(0);
        let mut reader = SnapshotReader::new(&bytes).unwrap();
        state.load(&mut reader).unwrap();
        assert_eq!(reader.remaining(), 0);
        assert_eq!(state.cpu_profile, CpuProfile::XboxRetailV1);
        assert_eq!(state.clock.tsc(), if version >= 4 { 12345 } else { 0 });
        assert_eq!(
            state.breakpoint,
            if version >= 6 { Some(0x10FFF) } else { None }
        );
        assert_eq!(state.read_msr(0x2A).unwrap(), 0x0104_0000);
        // v1–v7 não tinham SSE nem x87: o estado migra para o de RESET.
        assert_eq!(state.xmm, [[0; 4]; 8]);
        assert_eq!(state.mxcsr, xbox_recomp::runtime::MXCSR_RESET);
        assert_eq!(state.fpu.depth(), 0);
    }
}
