use std::path::Path;
use xbox_formats::fixtures::xbe_com_imports;
use xbox_recomp::{analysis, emit};

pub(super) fn sources(output: &Path) -> String {
    for (name, returns) in [("thread_boot", false), ("thread_ret", true)] {
        let mut code = Vec::new();
        // PsCreateSystemThreadEx: SystemRoutine, flags, contexto, StartRoutine,
        // ThreadId, TlsDataSize, StackSize, extensão, handle (ordem inversa).
        for arg in [
            0x11080u32, 0, 0, 0x12120, 0x11100, 0x12014, 20, 65536, 0, 0x12010,
        ] {
            code.push(0x68);
            code.extend(arg.to_le_bytes());
        }
        code.extend([0xFF, 0x15, 0, 0x20, 1, 0]); // create.
        code.extend([0xA3, 0x18, 0x20, 1, 0]); // pai só grava após a filha terminar.
        code.extend([0x6A, 0, 0x6A, 0, 0xFF, 0x35, 0x10, 0x20, 1, 0]);
        code.extend([0xFF, 0x15, 12, 0x20, 1, 0, 0xC3]); // wait; ret.
        code.resize(0x80, 0xCC);
        if returns {
            code.extend([0xB8, 9, 0, 0, 0, 0xC2, 8, 0]);
        } else {
            code.extend([0x55, 0x89, 0xE5]); // frame do SystemRoutine.
            code.extend([0xFF, 0x15, 4, 0x20, 1, 0]); // KeGetCurrentThread.
            code.extend([0x8B, 0x40, 0x28, 0x8D, 0x50, 4, 0x89, 0x10]); // TLS self-slot.
            code.extend([0xC7, 0x02, 0x78, 0x56, 0x34, 0x12]); // template, pelo guest.
            code.extend([0xFF, 0x75, 12, 0xFF, 0x55, 8]); // StartRoutine(context).
            code.extend([0x50, 0xFF, 0x15, 8, 0x20, 1, 0, 0xCC]); // terminate; INT3 inalcançável.
        }
        code.resize(0x100, 0xCC);
        code.extend([0x64, 0xA1, 4, 0, 0, 0, 0x8B, 0x40, 0xEC, 0x8B, 0]); // TLS via FS.
        code.extend([0xA3, 0, 0x21, 1, 0]);
        code.extend([0x8B, 0x44, 0x24, 4, 0xA3, 4, 0x21, 1, 0]);
        code.extend([0xBB, 0xAD, 0x0B, 0, 0, 0xB8, 42, 0, 0, 0, 0xC2, 4, 0]);
        let image = xbe_com_imports(&code, &[255, 104, 258, 233]).unwrap();
        let program = analysis::analyze_with_entries(&image, &[0x11080, 0x11100]).unwrap();
        std::fs::write(
            output.join(format!("src/{name}.rs")),
            emit::rust_source(&program),
        )
        .unwrap();
        std::fs::write(output.join(format!("src/{name}.xbe")), image).unwrap();
    }
    include_str!("../verification_threads.inc").to_owned()
}
