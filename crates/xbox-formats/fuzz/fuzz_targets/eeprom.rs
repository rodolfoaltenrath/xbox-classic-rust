//! Fuzz da EEPROM.
//!
//! A EEPROM não tem assinatura: qualquer sequência de 256 bytes é estruturalmente
//! válida. A propriedade sob teste é que ler, consultar e reserializar nunca entra em
//! pânico e nunca altera o bloco cifrado.
#![no_main]

use libfuzzer_sys::fuzz_target;
use xbox_formats::eeprom::{self, Eeprom};

fuzz_target!(|data: &[u8]| {
    if data.len() != eeprom::TAMANHO {
        return;
    }
    if let Ok(e) = Eeprom::parse(data) {
        let _ = e.checksums_conferem();
        let bytes = e.bytes();
        assert_eq!(&bytes[0x14..0x30], &data[0x14..0x30]);
    }
});
