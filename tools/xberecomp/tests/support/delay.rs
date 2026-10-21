use std::path::Path;
use xbox_formats::fixtures::xbe_com_imports;
use xbox_recomp::{analysis, emit};

/// Programa x86 que mede o próprio `Sleep` com `RDTSC` antes e depois.
///
/// Reproduz o `SleepEx` do nxdk (`UserMode`, não alertável, intervalo relativo) e deixa
/// os dois timestamps em memória guest, para o teste provar que o relógio avançou pelo
/// intervalo e que o guest **enxerga** isso — a única forma de o tempo existir para ele.
pub(super) fn sources(output: &Path) -> String {
    let mut code = Vec::new();
    code.extend([0x0F, 0x31]); // rdtsc
    code.extend([0xA3, 0x00, 0x21, 0x01, 0x00]); // mov [0x12100], eax  (TSC antes, baixo)
    code.extend([0x89, 0x15, 0x04, 0x21, 0x01, 0x00]); // mov [0x12104], edx  (TSC antes, alto)
    code.extend([0x68, 0x20, 0x20, 0x01, 0x00]); // push 0x12020   (&Interval, em .rdata)
    code.extend([0x6A, 0x00]); // push 0         (Alertable = FALSE)
    code.extend([0x6A, 0x01]); // push 1         (WaitMode  = UserMode)
    code.extend([0xFF, 0x15, 0x00, 0x20, 0x01, 0x00]); // call [imports+0]  stdcall/12
    code.extend([0xA3, 0x08, 0x21, 0x01, 0x00]); // mov [0x12108], eax  (status)
    code.extend([0x0F, 0x31]); // rdtsc
    code.extend([0xA3, 0x0C, 0x21, 0x01, 0x00]); // mov [0x1210C], eax  (TSC depois, baixo)
    code.extend([0x89, 0x15, 0x10, 0x21, 0x01, 0x00]); // mov [0x12110], edx  (TSC depois, alto)
    code.push(0xC3); // ret

    let mut image = xbe_com_imports(&code, &[99]).unwrap();
    // `Interval` = -20_000_000 (2 s relativos) em 0x12020, dentro de `.rdata`; 0x2000 é a
    // thunk table com o import. As saídas em 0x12100.. ficam na mesma página de 4 KiB
    // (`0x12000..0x13000`), que o loader mapeia inteira embora `.rdata` declare só 0x40
    // bytes — é o mesmo arranjo que a fixture de IRQL usa.
    image[0x2020..0x2028].copy_from_slice(&(-20_000_000i64).to_le_bytes());
    let program = analysis::analyze(&image).unwrap();
    std::fs::write(
        output.join("src/kernel_delay.rs"),
        emit::rust_source(&program),
    )
    .unwrap();
    std::fs::write(output.join("src/kernel_delay.xbe"), image).unwrap();
    include_str!("../verification_delay.inc").to_owned()
}
