//! Cabeçalhos públicos do pushbuffer NV2A; sem leitura de memória ou efeitos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Header {
    Jump(u32),
    Call(u32),
    Return,
    Methods {
        method: u16,
        subchannel: u8,
        count: u16,
        non_incrementing: bool,
    },
}
/// Formas do NV2A documentadas no inventário e ADR-0065; palavra já little-endian.
pub const fn decode(word: u32) -> Result<Header, &'static str> {
    if word & 0xE000_0003 == 0x2000_0000 {
        return Ok(Header::Jump(word & 0x1FFF_FFFF));
    }
    match word & 3 {
        1 => return Ok(Header::Jump(word & !3)),
        2 => return Ok(Header::Call(word & !3)),
        _ => (),
    }
    if word == 0x0002_0000 {
        return Ok(Header::Return);
    }
    if matches!(word & 0xE003_0003, 0 | 0x4000_0000) {
        return Ok(Header::Methods {
            method: (word & 0x1FFC) as u16,
            subchannel: ((word >> 13) & 7) as u8,
            count: ((word >> 18) & 0x7FF) as u16,
            non_incrementing: word & 0x4000_0000 != 0,
        });
    }
    Err("cabeçalho de pushbuffer reservado")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn formas_jump_call_return_e_reservado() {
        assert_eq!(decode(0x1235), Ok(Header::Jump(0x1234)));
        assert_eq!(decode(0x20001234), Ok(Header::Jump(0x1234)));
        assert_eq!(decode(0x1236), Ok(Header::Call(0x1234)));
        assert_eq!(decode(0x20000), Ok(Header::Return));
        assert!(decode(0xDEADBEEF).is_err());
    }
    #[test]
    fn cabecalhos_incremento_e_nao_incremento_preservam_campos() {
        for non_incrementing in [false, true] {
            let word =
                (2047 << 18) | (7 << 13) | 0x1FFC | if non_incrementing { 1 << 30 } else { 0 };
            assert_eq!(
                decode(word),
                Ok(Header::Methods {
                    method: 0x1FFC,
                    subchannel: 7,
                    count: 2047,
                    non_incrementing
                })
            );
        }
        assert_eq!(
            decode(0),
            Ok(Header::Methods {
                method: 0,
                subchannel: 0,
                count: 0,
                non_incrementing: false
            })
        );
        assert!(decode(0x00010000).is_err());
    }
}
