use std::path::Path;
use xbox_formats::fixtures::{xbe_com_codigo, xbe_com_imports};
use xbox_recomp::{analysis, emit};

pub(super) fn sources(output: &Path) -> String {
    let code = &[
        0x68, 0x22, 0x11, 0, 0, // push 0x1122
        0xFF, 0x15, 0, 0x20, 1, 0, // call [imports+0]: stdcall/4
        0xA3, 0x30, 0x20, 1, 0, 0x6A, 0x33, 0xFF, 0x15, 4, 0x20, 1, 0, // cdecl: caller cleans
        0x83, 0xC4, 4, 0xB9, 0x44, 0, 0, 0, 0xBA, 0x55, 0, 0, 0, 0x6A, 0x66, 0xFF, 0x15, 8, 0x20,
        1, 0, // fastcall/12
        0x8B, 0x0D, 0x10, 0x20, 1, 0, // ecx = _tls_index
        0x64, 0x8B, 0x15, 4, 0, 0, 0, // edx = FS:[4]
        0x8B, 0x14, 0x8A, // edx = [edx+ecx*4]
        0x8B, 0x1A, 0x43, // ebx = [edx]; inc ebx
        0x64, 0x89, 0x1D, 0x14, 0, 0, 0, // FS store
        0x64, 0x8D, 0x70, 8, // LEA ignores FS base
        0xC3,
    ];
    let mut image = xbe_com_imports(code, &[181, 8, 21]).unwrap();
    image[0x12C..0x130].copy_from_slice(&0x103C0u32.to_le_bytes());
    for (index, value) in [0x12020u32, 0x12024, 0x12010, 0, 13, 0x00300000]
        .into_iter()
        .enumerate()
    {
        image[0x3C0 + index * 4..0x3C4 + index * 4].copy_from_slice(&value.to_le_bytes());
    }
    image[0x2020..0x2024].copy_from_slice(&0x44332211u32.to_le_bytes());
    write(output, "kernel_abi", &image);
    write(
        output,
        "kernel_tail",
        &xbe_com_imports(&[0xFF, 0x25, 0, 0x20, 1, 0], &[8]).unwrap(),
    );

    // Chamada a um serviço **implementado**: RtlInitAnsiString (289), stdcall/8.
    // Stdcall empilha da direita para a esquerda, então SourceString vai primeiro.
    // A `.rdata` da fixture tem 0x40 bytes: o thunk fica em 0x12000, o destino em
    // 0x12010 e a string de origem em 0x12020, sem sobreposição.
    let mut dispatch_image = xbe_com_imports(
        &[
            0x68, 0x20, 0x20, 1, 0, // push 0x12020 (SourceString)
            0x68, 0x10, 0x20, 1, 0, // push 0x12010 (DestinationString)
            0xFF, 0x15, 0, 0x20, 1, 0, // call [imports+0]
            0xC3,
        ],
        &[289],
    )
    .unwrap();
    dispatch_image[0x2020..0x2020 + 15].copy_from_slice(b"D:\\default.xbe\0");
    write(output, "kernel_dispatch", &dispatch_image);
    write(
        output,
        "kernel_mutant",
        &xbe_com_imports(
            &[
                0x6A, 0, 0x6A, 0, // push FALSE; push NULL
                0x68, 0x10, 0x20, 1, 0, // output 0x12010
                0xFF, 0x15, 0, 0x20, 1, 0, 0x6A, 1, 0x6A, 0, // push TRUE; push NULL
                0x68, 0x14, 0x20, 1, 0, // output 0x12014
                0xFF, 0x15, 0, 0x20, 1, 0, 0xC3,
            ],
            &[192],
        )
        .unwrap(),
    );
    write(
        output,
        "fs_fault",
        &xbe_com_codigo(&[0x64, 0xA1, 4, 0, 0, 0, 0xC3]).unwrap(),
    );
    include_str!("../verification_kernel.inc").to_owned()
}

fn write(output: &Path, name: &str, image: &[u8]) {
    let program = analysis::analyze(image).unwrap();
    std::fs::write(
        output.join(format!("src/{name}.rs")),
        emit::rust_source(&program),
    )
    .unwrap();
    std::fs::write(output.join(format!("src/{name}.xbe")), image).unwrap();
}
