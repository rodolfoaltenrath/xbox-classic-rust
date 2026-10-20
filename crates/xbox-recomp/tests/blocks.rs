use xbox_formats::fixtures::xbe_com_codigo;
use xbox_recomp::analysis::{MAX_BLOCK_INSTRUCTIONS, analyze};

#[test]
fn sequencia_longa_e_partida_em_blocos_limitados_sem_perder_instrucoes() {
    let mut code = [0x01, 0xC8].repeat(1000);
    code.push(0xC3);
    let p = analyze(&xbe_com_codigo(&code).unwrap()).unwrap();
    assert_eq!(p.instructions.len(), 1001);
    assert_eq!(p.blocks.len(), 1001usize.div_ceil(MAX_BLOCK_INSTRUCTIONS));
    let pcs: Vec<_> = p
        .blocks
        .values()
        .flat_map(|block| {
            assert!(block.len() <= MAX_BLOCK_INSTRUCTIONS);
            block.iter().map(|i| i.pc)
        })
        .collect();
    assert_eq!(pcs, p.instructions.keys().copied().collect::<Vec<_>>());
}
