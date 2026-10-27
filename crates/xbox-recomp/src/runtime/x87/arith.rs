//! Aritmética x87 em precisão estendida exata (ADR-0088, P9).
//!
//! Multiplicação, divisão e raiz quadrada são calculadas com inteiros de 128 bits e
//! arredondadas **uma vez** para a mantissa de 64 bits, ao par mais próximo: é o que o
//! hardware faz com a palavra de controle de RESET (precisão estendida, arredondamento
//! ao mais próximo, todas as exceções mascaradas). Fonte: Intel SDM vol. 1, §8.1.5 e
//! §4.8.4 (resultados mascarados), vol. 2 (FMUL, FDIVR, FSQRT).
//!
//! Exceções mascaradas produzem a resposta padrão: operação inválida devolve o NaN
//! "indefinido", divisão por zero devolve infinito, estouro arredonda para infinito e
//! subfluxo produz denormal estendido (ou zero). A palavra de estado não existe no
//! modelo, então os flags dessas exceções não ficam registrados (P9).

use super::{EXPONENT_BIAS, Extended, INTEGER_BIT, MAX_EXPONENT, SIGN};

/// NaN indefinido do x87: sinal negativo, expoente máximo, mantissa `1.1000…`.
pub const INDEFINITE: Extended = Extended {
    mantissa: 0xC000_0000_0000_0000,
    sign_exponent: MAX_EXPONENT | SIGN,
};

/// Bit que separa NaN silencioso de sinalizador na fração.
const QUIET: u64 = 1 << 62;

/// Classificação de um operando estendido.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Classe {
    Zero(bool),
    /// `(-1)^sinal × mantissa × 2^(expoente − 63)`, mantissa com o bit 63 ligado.
    Finito {
        sinal: bool,
        expoente: i32,
        mantissa: u64,
    },
    Infinito(bool),
    Nan(Extended),
    /// Expoente não nulo sem o bit inteiro (unnormal/pseudo-NaN/pseudo-infinito): o
    /// 387 em diante rejeita como operação inválida.
    Invalido,
}

impl Extended {
    pub(super) fn sinal(self) -> bool {
        self.sign_exponent & SIGN != 0
    }

    pub(super) fn classe(self) -> Classe {
        let sinal = self.sinal();
        let campo = self.sign_exponent & MAX_EXPONENT;
        let inteiro = self.mantissa & INTEGER_BIT != 0;
        match (campo, inteiro) {
            (MAX_EXPONENT, true) if self.mantissa == INTEGER_BIT => Classe::Infinito(sinal),
            (MAX_EXPONENT, true) => Classe::Nan(self),
            (0, _) if self.mantissa == 0 => Classe::Zero(sinal),
            // Denormal (e pseudo-denormal, com o bit inteiro): valor = m × 2^(1 − viés − 63).
            (0, _) => {
                let deslocamento = self.mantissa.leading_zeros();
                Classe::Finito {
                    sinal,
                    expoente: 1 - EXPONENT_BIAS - deslocamento.cast_signed(),
                    mantissa: self.mantissa << deslocamento,
                }
            }
            (_, false) => Classe::Invalido,
            (_, true) => Classe::Finito {
                sinal,
                expoente: i32::from(campo) - EXPONENT_BIAS,
                mantissa: self.mantissa,
            },
        }
    }

    const fn infinito(sinal: bool) -> Self {
        Self {
            mantissa: INTEGER_BIT,
            sign_exponent: MAX_EXPONENT | if sinal { SIGN } else { 0 },
        }
    }

    const fn zero(sinal: bool) -> Self {
        Self {
            mantissa: 0,
            sign_exponent: if sinal { SIGN } else { 0 },
        }
    }

    /// `ST(i) × ST(0)` (FMUL/FMULP).
    #[must_use]
    pub fn multiply(self, outro: Self) -> Self {
        match (self.classe(), outro.classe()) {
            (Classe::Invalido, _) | (_, Classe::Invalido) => INDEFINITE,
            (Classe::Nan(_), _) | (_, Classe::Nan(_)) => propagar(self, outro),
            (Classe::Infinito(_), Classe::Zero(_)) | (Classe::Zero(_), Classe::Infinito(_)) => {
                INDEFINITE
            }
            (Classe::Infinito(a), Classe::Infinito(b) | Classe::Finito { sinal: b, .. })
            | (Classe::Finito { sinal: a, .. }, Classe::Infinito(b)) => Self::infinito(a ^ b),
            (Classe::Zero(a), Classe::Zero(b) | Classe::Finito { sinal: b, .. })
            | (Classe::Finito { sinal: a, .. }, Classe::Zero(b)) => Self::zero(a ^ b),
            (
                Classe::Finito {
                    sinal: sa,
                    expoente: ea,
                    mantissa: ma,
                },
                Classe::Finito {
                    sinal: sb,
                    expoente: eb,
                    mantissa: mb,
                },
            ) => {
                // ma, mb ∈ [2^63, 2^64): produto ∈ [2^126, 2^128).
                let mut produto = u128::from(ma) * u128::from(mb);
                let mut e2 = ea + eb - 126;
                if produto >> 127 == 0 {
                    produto <<= 1;
                    e2 -= 1;
                }
                empacotar(sa ^ sb, produto, e2)
            }
        }
    }

    /// `self ÷ divisor` (FDIV; FDIVR troca os operandos).
    #[must_use]
    pub fn divide(self, divisor: Self) -> Self {
        match (self.classe(), divisor.classe()) {
            (Classe::Invalido, _) | (_, Classe::Invalido) => INDEFINITE,
            (Classe::Nan(_), _) | (_, Classe::Nan(_)) => propagar(self, divisor),
            (Classe::Infinito(_), Classe::Infinito(_)) | (Classe::Zero(_), Classe::Zero(_)) => {
                INDEFINITE
            }
            // Divisão por zero mascarada: infinito com o sinal do quociente.
            (Classe::Infinito(a) | Classe::Finito { sinal: a, .. }, Classe::Zero(b))
            | (Classe::Infinito(a), Classe::Finito { sinal: b, .. }) => Self::infinito(a ^ b),
            (Classe::Zero(a) | Classe::Finito { sinal: a, .. }, Classe::Infinito(b))
            | (Classe::Zero(a), Classe::Finito { sinal: b, .. }) => Self::zero(a ^ b),
            (
                Classe::Finito {
                    sinal: sa,
                    expoente: ea,
                    mantissa: ma,
                },
                Classe::Finito {
                    sinal: sb,
                    expoente: eb,
                    mantissa: mb,
                },
            ) => {
                // Divisão longa em duas palavras: o quociente Q tem o bit 127 ligado e o
                // resto final vira bit de sticky, o que basta para arredondar ao par.
                let (dividendo, e2) = if ma < mb {
                    (u128::from(ma) << 64, ea - eb - 128)
                } else {
                    (u128::from(ma) << 63, ea - eb - 127)
                };
                let divisor = u128::from(mb);
                let q1 = dividendo / divisor;
                let r1 = dividendo % divisor;
                let q2 = (r1 << 64) / divisor;
                let r2 = (r1 << 64) % divisor;
                let quociente = q1 << 64 | q2 | u128::from(r2 != 0);
                empacotar(sa ^ sb, quociente, e2)
            }
        }
    }

    /// `√ST(0)` (FSQRT).
    #[must_use]
    pub fn square_root(self) -> Self {
        match self.classe() {
            Classe::Invalido | Classe::Infinito(true) | Classe::Finito { sinal: true, .. } => {
                INDEFINITE
            }
            Classe::Nan(_) => propagar(self, self),
            Classe::Zero(_) | Classe::Infinito(false) => self,
            Classe::Finito {
                sinal: false,
                expoente,
                mantissa,
            } => {
                // valor = m × 2^k; escolhe N = m × 2^j com (k − j) par e N ∈ [2^126, 2^128).
                let k = expoente - 63;
                let (n, j) = if (k - 64) % 2 == 0 {
                    (u128::from(mantissa) << 64, 64)
                } else {
                    (u128::from(mantissa) << 63, 63)
                };
                let s = n.isqrt();
                // √N está entre s e s+1; arredonda para cima se √N > s + ½, isto é,
                // N − s² > s (empate impossível: √N inteiro ou irracional).
                let s = s + u128::from(n - s * s > s);
                let e2 = (k - j) / 2;
                if s >> 64 != 0 {
                    // s = 2^64 após o arredondamento: 2^64 × 2^e2 = 2^127 × 2^(e2 − 63).
                    return empacotar(false, 1u128 << 127, e2 - 63);
                }
                empacotar(false, s << 64, e2 - 64)
            }
        }
    }
}

/// Resposta de NaN do x87 com um ou dois operandos NaN: o NaN de maior mantissa (o
/// primeiro em empate), sempre convertido em silencioso (SDM vol. 1, tabela 4-7).
fn propagar(a: Extended, b: Extended) -> Extended {
    let silencioso = |v: Extended| Extended {
        mantissa: v.mantissa | QUIET,
        sign_exponent: v.sign_exponent,
    };
    match (a.classe(), b.classe()) {
        (Classe::Nan(x), Classe::Nan(y)) => {
            if (y.mantissa | QUIET) > (x.mantissa | QUIET) {
                silencioso(y)
            } else {
                silencioso(x)
            }
        }
        (Classe::Nan(x), _) => silencioso(x),
        (_, Classe::Nan(y)) => silencioso(y),
        _ => INDEFINITE,
    }
}

/// Arredonda `q × 2^e2` (q com o bit 127 ligado) para estendido, ao par mais próximo.
fn empacotar(sinal: bool, q: u128, e2: i32) -> Extended {
    debug_assert_eq!(q >> 127, 1);
    let sinal_bits = if sinal { SIGN } else { 0 };
    // Os 64 bits altos formam a mantissa: expoente não enviesado do bit inteiro.
    let expoente = e2 + 127;
    let mut campo = expoente + EXPONENT_BIAS;
    // Normal: descarta 64 bits; denormal: descarta também o que fica abaixo de 2^-16382.
    let descarte = if campo >= 1 {
        64
    } else {
        (64 + 1 - campo).unsigned_abs()
    };
    let (mantido, sobe) = arredondar(q, descarte);
    let mut mantissa = mantido + u128::from(sobe);
    if campo < 1 {
        // Denormal: campo 0; se o arredondamento alcançar o bit inteiro, vira normal 1.
        campo = i32::from(mantissa >> 63 != 0);
        #[allow(clippy::cast_sign_loss)]
        return Extended {
            mantissa: mantissa as u64,
            sign_exponent: campo as u16 | sinal_bits,
        };
    }
    if mantissa >> 64 != 0 {
        mantissa >>= 1;
        campo += 1;
    }
    if campo >= i32::from(MAX_EXPONENT) {
        // Estouro mascarado com arredondamento ao mais próximo: infinito.
        return Extended::infinito(sinal);
    }
    #[allow(clippy::cast_sign_loss)]
    Extended {
        mantissa: mantissa as u64,
        sign_exponent: campo as u16 | sinal_bits,
    }
}

/// `q >> descarte` e se deve somar um (ao par mais próximo).
fn arredondar(q: u128, descarte: u32) -> (u128, bool) {
    if descarte == 0 {
        return (q, false);
    }
    if descarte > 128 {
        return (0, false);
    }
    let mantido = if descarte == 128 { 0 } else { q >> descarte };
    let resto = if descarte == 128 {
        q
    } else {
        q & ((1u128 << descarte) - 1)
    };
    let metade = 1u128 << (descarte - 1);
    (
        mantido,
        resto > metade || (resto == metade && mantido & 1 == 1),
    )
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    fn ext(v: f64) -> Extended {
        Extended::from_f64_bits(v.to_bits())
    }

    fn f64_de(v: Extended) -> f64 {
        f64::from_bits(v.to_f64_bits())
    }

    #[test]
    fn produtos_e_quocientes_exatos_concordam_com_f64() {
        for (a, b) in [
            (1.5, 2.25),
            (-3.0, 0.5),
            (1e10, 1e-10),
            (7.0, -11.0),
            (0.1, 10.0),
        ] {
            assert_eq!(f64_de(ext(a).multiply(ext(b))), a * b, "{a} × {b}");
        }
        for (a, b) in [
            (3.375, 1.5),
            (-1.0, 4.0),
            (1.0, 3.0),
            (2.0, 7.0),
            (1e300, 1e-300),
        ] {
            // Um único arredondamento de 64 → 53 bits concorda com a divisão em f64
            // nesses casos (sem empate duplo).
            assert_eq!(f64_de(ext(a).divide(ext(b))), a / b, "{a} ÷ {b}");
        }
    }

    #[test]
    fn um_terco_tem_a_mantissa_estendida_correta() {
        // 1/3 = 0.0101…₂: mantissa 0xAAAA…AAAB (arredondada para cima), expoente −2.
        let terco = Extended::ONE.divide(Extended::from_i64(3));
        assert_eq!(terco.mantissa, 0xAAAA_AAAA_AAAA_AAAB);
        assert_eq!(terco.sign_exponent, (EXPONENT_BIAS - 2) as u16);
        // 3 × (1/3) = 1 + 2^-65 exato, abaixo de meio ulp de 1: arredonda para 1.
        assert_eq!(Extended::from_i64(3).multiply(terco), Extended::ONE);
    }

    #[test]
    fn raiz_quadrada_exata_e_arredondada() {
        assert_eq!(ext(9.0).square_root(), ext(3.0));
        assert_eq!(ext(0.25).square_root(), ext(0.5));
        assert_eq!(ext(2.0).square_root().mantissa, 0xB504_F333_F9DE_6484);
        assert_eq!(ext(2.0).square_root().sign_exponent, EXPONENT_BIAS as u16);
        assert_eq!(f64_de(ext(2.0).square_root()), 2f64.sqrt());
        assert_eq!(ext(-0.0).square_root(), ext(-0.0));
        assert_eq!(ext(-1.0).square_root(), INDEFINITE);
        assert_eq!(ext(f64::INFINITY).square_root(), ext(f64::INFINITY));
    }

    #[test]
    fn casos_especiais_seguem_a_resposta_mascarada() {
        let inf = ext(f64::INFINITY);
        assert_eq!(inf.multiply(ext(0.0)), INDEFINITE);
        assert_eq!(ext(-2.0).multiply(inf), ext(f64::NEG_INFINITY));
        assert_eq!(ext(0.0).divide(ext(0.0)), INDEFINITE);
        assert_eq!(inf.divide(inf), INDEFINITE);
        assert_eq!(ext(1.0).divide(ext(-0.0)), ext(f64::NEG_INFINITY));
        assert_eq!(ext(-1.0).divide(inf), ext(-0.0));
        assert_eq!(ext(-0.0).multiply(ext(5.0)), ext(-0.0));
        let snan = Extended {
            mantissa: INTEGER_BIT | 1,
            sign_exponent: MAX_EXPONENT,
        };
        assert_eq!(snan.multiply(ext(1.0)).mantissa, INTEGER_BIT | QUIET | 1);
        let unnormal = Extended {
            mantissa: 1,
            sign_exponent: 100,
        };
        assert_eq!(unnormal.multiply(ext(1.0)), INDEFINITE);
    }

    #[test]
    fn estouro_e_subfluxo_do_formato_estendido() {
        let enorme = Extended {
            mantissa: INTEGER_BIT,
            sign_exponent: MAX_EXPONENT - 1,
        };
        assert_eq!(enorme.multiply(ext(2.0)), ext(f64::INFINITY));
        let minimo = Extended {
            mantissa: INTEGER_BIT,
            sign_exponent: 1,
        };
        // Metade do menor normal: denormal com campo 0 e mantissa 0x4000…
        let metade = minimo.divide(ext(2.0));
        assert_eq!(metade.sign_exponent, 0);
        assert_eq!(metade.mantissa, 1 << 62);
        // Denormal volta ao normal ao multiplicar por 2.
        assert_eq!(metade.multiply(ext(2.0)), minimo);
        assert_eq!(minimo.multiply(minimo), Extended::ZERO);
    }
}
