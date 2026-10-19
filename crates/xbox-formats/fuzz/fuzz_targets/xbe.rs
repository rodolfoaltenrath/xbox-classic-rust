//! Fuzz do parser de XBE.
//!
//! Propriedade sob teste: nenhuma entrada causa pânico, laço infinito ou alocação
//! ilimitada. Quando o parser aceita a entrada, os acessores também têm que ser
//! seguros — é aí que moram os bugs de validação parcial.
#![no_main]

use libfuzzer_sys::fuzz_target;
use xbox_formats::Xbe;

fuzz_target!(|data: &[u8]| {
    if let Ok(xbe) = Xbe::parse(data) {
        let _ = xbe.titulo();
        let _ = xbe.logo();
        let _ = xbe.debug_pathname();
        let _ = xbe.certificate.title_id_formatado();
        for i in 0..xbe.sections.len() {
            let _ = xbe.section_data(i);
        }
    }
});
