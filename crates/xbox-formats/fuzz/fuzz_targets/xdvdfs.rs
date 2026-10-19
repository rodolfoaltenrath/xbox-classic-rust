//! Fuzz do leitor de XDVDFS.
//!
//! Além de pânico, este alvo procura **não terminação**: a árvore de diretórios vem do
//! arquivo e pode ter ciclos, então `walk` precisa sempre terminar.
#![no_main]

use libfuzzer_sys::fuzz_target;
use xbox_formats::{Xbe, Xdvdfs};

fuzz_target!(|data: &[u8]| {
    if let Ok(fs) = Xdvdfs::open(data) {
        let _ = fs.root();
        let _ = fs.walk();
        if let Ok(entrada) = fs.default_xbe() {
            if let Ok(bytes) = fs.read(&entrada) {
                let _ = Xbe::parse(bytes);
            }
        }
    }
});
