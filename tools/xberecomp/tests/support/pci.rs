use std::path::Path;
use xbox_formats::fixtures::xbe_com_imports;
use xbox_recomp::{analysis, emit};

pub(super) fn sources(output: &Path) -> String {
    let mut code = Vec::new();
    call(&mut code, [0, 0x60, 0x6C, 0x12100, 4, 0]);
    // Frequência calculada PELO guest: cristal (64 bits) * PLL.mult * 55 /
    // (PLL.div * 10000). Neste perfil div=1, coincide com a ordem do nxdk.
    code.extend([
        0xA1, 0, 0x21, 1, 0, // mov eax,[0x12100]
        0x89, 0xC7, 0x81, 0xE7, 0xFF, 0, 0, 0, // edi=divisor
        0xC1, 0xE8, 8, 0x25, 0xFF, 0, 0, 0, // eax=multiplicador
        0x6B, 0xC8, 55, // imul ecx,eax,55
        0x69, 0xFF, 0x10, 0x27, 0, 0, // imul edi,edi,10000
        0xB8, 0xAA, 0x26, 0x69, 0xE1, // eax=cristal.low
        0xF7, 0xE1, // mul ecx
        0x6B, 0xD9, 3, 0x01, 0xDA, // ebx=cristal.high*ecx; edx+=ebx
        0xF7, 0xF7, // div edi
        0xA3, 0, 0x22, 1, 0, // save frequency
    ]);
    call(&mut code, [0, 0x60, 0x6D, 0x12110, 2, 0]);
    code.push(0xC3);
    let image = xbe_com_imports(&code, &[46]).unwrap();
    let program = analysis::analyze(&image).unwrap();
    std::fs::write(
        output.join("src/kernel_pci.rs"),
        emit::rust_source(&program),
    )
    .unwrap();
    std::fs::write(output.join("src/kernel_pci.xbe"), image).unwrap();
    include_str!("../verification_pci.inc").to_owned()
}

fn call(code: &mut Vec<u8>, args: [u32; 6]) {
    for word in args.into_iter().rev() {
        code.push(0x68);
        code.extend(word.to_le_bytes());
    }
    code.extend([0xFF, 0x15, 0, 0x20, 1, 0]);
}
