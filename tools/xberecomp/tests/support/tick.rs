use std::path::Path;
use xbox_formats::fixtures::xbe_com_imports;
use xbox_recomp::{analysis, emit};
pub(super) fn sources(output: &Path) -> String {
    // mov esi,[KeTickCount IAT]; mov ebx,[esi]; poll: mov eax,[esi]; cmp eax,ebx; je poll; ret
    for (name, code) in [
        (
            "tick_poll",
            &[
                0x8B, 0x35, 0, 0x20, 1, 0, 0x8B, 0x1E, 0x8B, 0x06, 0x39, 0xD8, 0x74, 0xFA, 0xC3,
            ][..],
        ),
        ("tick_rep", &[0xF3, 0xAA, 0xC3][..]),
        ("tick_step", &[0xEB, 0, 0xC3][..]),
    ] {
        let bytes = xbe_com_imports(code, &[156]).unwrap();
        let p = analysis::analyze(&bytes).unwrap();
        std::fs::write(output.join(format!("src/{name}.rs")), emit::rust_source(&p)).unwrap();
        std::fs::write(output.join(format!("src/{name}.xbe")), bytes).unwrap();
    }
    include_str!("../verification_tick.inc").to_owned()
}
