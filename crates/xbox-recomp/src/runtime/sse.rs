//! Semântica de SSE escalar de precisão simples que o Rust não dá de graça.
//!
//! A soma, subtração, multiplicação, divisão e raiz do Rust em `f32` são IEEE-754 com
//! arredondamento ao mais próximo — o mesmo que o MXCSR de RESET manda o SSE fazer. O que
//! difere, e vive aqui, são as conversões para inteiro, `MIN`/`MAX` e as flags de comparação.
//!
//! **Fora do modelo (ADR-0053, P8):** flags de exceção e máscaras do MXCSR, modos de
//! arredondamento diferentes de "mais próximo", FTZ/DAZ, e o padrão de bits de NaN gerado
//! por operações inválidas (vem do host; em x86-64 coincide com o do SSE).

use crate::Error;

const ZF: u32 = 1 << 6;
const PF: u32 = 1 << 2;
const CF: u32 = 1;
const OF_SF_AF: u32 = (1 << 11) | (1 << 7) | (1 << 4);

/// "Indefinido inteiro": o que `CVT*SS2SI` devolve para NaN ou fora do intervalo.
pub const INTEGER_INDEFINITE: u32 = 0x8000_0000;

/// `CVTTSS2SI`: trunca em direção a zero. NaN e fora de `i32` dão [`INTEGER_INDEFINITE`].
///
/// `as i32` do Rust satura, o que seria um resultado errado sem erro algum.
pub fn cvttss2si(value: f32) -> u32 {
    to_integer(value.trunc())
}

/// `CVTSS2SI` com MXCSR de RESET: arredonda ao par mais próximo (empates para o par).
pub fn cvtss2si(value: f32) -> u32 {
    to_integer(value.round_ties_even())
}

fn to_integer(rounded: f32) -> u32 {
    // 2^31 é exato em f32; o intervalo válido é [-2^31, 2^31).
    if rounded.is_nan() || rounded >= 2_147_483_648.0 || rounded < -2_147_483_648.0 {
        INTEGER_INDEFINITE
    } else {
        #[allow(clippy::cast_possible_truncation)]
        (rounded as i32).cast_unsigned()
    }
}

/// `MINSS`: se qualquer lado é NaN ou os dois são zero/iguais, devolve a **origem**.
pub fn minss(destination: f32, source: f32) -> f32 {
    if destination < source {
        destination
    } else {
        source
    }
}

/// `MAXSS`: mesma regra de [`minss`].
pub fn maxss(destination: f32, source: f32) -> f32 {
    if destination > source {
        destination
    } else {
        source
    }
}

/// EFLAGS após `UCOMISS`/`COMISS`: ZF/PF/CF codificam a relação; OF, SF e AF zeram.
///
/// Não ordenado (algum NaN) liga os três, o que faz `JP` separar NaN de igual.
pub fn compare_flags(eflags: u32, left: f32, right: f32) -> u32 {
    let relation = if left.is_nan() || right.is_nan() {
        ZF | PF | CF
    } else if left > right {
        0
    } else if left < right {
        CF
    } else {
        ZF
    };
    (eflags & !(ZF | PF | CF | OF_SF_AF)) | relation
}

/// Acesso de 128 bits a endereço que não é múltiplo de 16: #GP no hardware.
pub fn check_aligned(pc: u32, address: u32) -> Result<(), Error> {
    if address.is_multiple_of(16) {
        Ok(())
    } else {
        Err(Error::MisalignedSse { pc, address })
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // comparações exatas são o contrato: bits de IEEE-754
mod tests {
    use super::*;

    #[test]
    fn truncamento_e_em_direcao_a_zero() {
        assert_eq!(cvttss2si(1.9), 1);
        assert_eq!(cvttss2si(-1.9), (-1i32).cast_unsigned());
        assert_eq!(cvttss2si(0.99), 0);
        assert_eq!(cvttss2si(-0.0), 0);
    }

    #[test]
    fn arredondamento_vai_ao_par_mais_proximo() {
        assert_eq!(cvtss2si(0.5), 0);
        assert_eq!(cvtss2si(1.5), 2);
        assert_eq!(cvtss2si(2.5), 2);
        assert_eq!(cvtss2si(-1.5), (-2i32).cast_unsigned());
    }

    #[test]
    fn fora_do_intervalo_e_nan_dao_indefinido_em_vez_de_saturar() {
        for v in [
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            2_147_483_648.0,
            3.0e9,
            -3.0e9,
        ] {
            assert_eq!(cvttss2si(v), INTEGER_INDEFINITE, "{v}");
        }
        // Limites válidos exatos.
        assert_eq!(cvttss2si(-2_147_483_648.0), INTEGER_INDEFINITE); // = i32::MIN, coincide
        assert_eq!(cvttss2si(2_147_483_520.0), 2_147_483_520);
    }

    #[test]
    fn min_max_devolvem_a_origem_quando_nao_ordenado_ou_igual() {
        assert_eq!(minss(1.0, 2.0), 1.0);
        assert_eq!(minss(2.0, 1.0), 1.0);
        assert_eq!(minss(f32::NAN, 1.0).to_bits(), 1.0f32.to_bits());
        assert!(minss(1.0, f32::NAN).is_nan());
        assert!(maxss(1.0, f32::NAN).is_nan());
        // +0 e -0 são iguais: a origem vence.
        assert!(minss(0.0, -0.0).is_sign_negative());
        assert!(minss(-0.0, 0.0).is_sign_positive());
        assert_eq!(maxss(3.0, 2.0), 3.0);
    }

    #[test]
    fn flags_de_comparacao_codificam_a_relacao() {
        let base = 0x0000_0FD7 | 2; // flags de arbitrárias ligadas, inclusive OF/SF/AF
        let maior = compare_flags(base, 2.0, 1.0);
        assert_eq!(maior & (ZF | PF | CF), 0);
        assert_eq!(compare_flags(base, 1.0, 2.0) & (ZF | PF | CF), CF);
        assert_eq!(compare_flags(base, 1.0, 1.0) & (ZF | PF | CF), ZF);
        assert_eq!(
            compare_flags(base, f32::NAN, 1.0) & (ZF | PF | CF),
            ZF | PF | CF
        );
        assert_eq!(
            compare_flags(base, 1.0, f32::NAN) & (ZF | PF | CF),
            ZF | PF | CF
        );
        // OF/SF/AF sempre zeram; as demais flags ficam como estavam.
        for r in [maior, compare_flags(base, 1.0, 2.0)] {
            assert_eq!(r & OF_SF_AF, 0);
            assert_eq!(
                r & !(ZF | PF | CF | OF_SF_AF),
                base & !(ZF | PF | CF | OF_SF_AF)
            );
        }
    }

    #[test]
    fn alinhamento_de_16() {
        assert!(check_aligned(0, 0x1000).is_ok());
        assert!(check_aligned(0, 0x1010).is_ok());
        for a in [0x1001, 0x1004, 0x1008, 0x100F] {
            assert!(matches!(
                check_aligned(7, a),
                Err(Error::MisalignedSse { pc: 7, address }) if address == a
            ));
        }
    }
}
