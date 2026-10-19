//! Fuzz do leitor de FATX.
//!
//! O alvo aqui é a cadeia de clusters: uma lista ligada vinda do disco. Além de
//! pânico, este alvo procura **não terminação** e alocação ilimitada.
#![no_main]

use libfuzzer_sys::fuzz_target;
use xbox_formats::fatx::Fatx;

fuzz_target!(|data: &[u8]| {
    if let Ok(fs) = Fatx::open(data) {
        let _ = fs.walk();
        if let Ok(e) = fs.find("savedata.bin") {
            let _ = fs.read(&e);
        }
    }
});
