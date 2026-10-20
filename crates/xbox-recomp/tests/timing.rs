use xbox_formats::fixtures::xbe_com_codigo;
use xbox_recomp::{analysis::analyze, ir::Operation};

#[test]
fn rdtsc_tem_ir_propria_e_destinos_edx_eax_sao_implicitos() {
    let p = analyze(&xbe_com_codigo(&[0x0F, 0x31, 0xC3]).unwrap()).unwrap();
    let instruction = &p.instructions[&0x11000];
    assert_eq!(instruction.operation, Operation::ReadTimestamp);
    assert_eq!(instruction.next, 0x11002);
}

#[test]
fn lock_rep_e_rdtscp_continuam_recusados() {
    for code in [
        &[0xF0, 0x0F, 0x31, 0xC3][..],
        &[0xF3, 0x0F, 0x31, 0xC3][..],
        &[0x0F, 0x01, 0xF9, 0xC3][..],
    ] {
        assert!(analyze(&xbe_com_codigo(code).unwrap()).is_err());
    }
}
