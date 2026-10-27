//! FSIN e FCOS com a redução de argumento do hardware (ADR-0088, P9).
//!
//! O x87 reduz o argumento com uma aproximação de π de **66 bits** (Intel SDM vol. 1,
//! §8.3.10): perto de múltiplos de π o resultado difere do seno matemático, e é isso que
//! o console calcula. O modelo reproduz a redução exatamente, em inteiros de 128 bits,
//! e avalia o argumento reduzido com os polinômios do fdlibm (`__kernel_sin`/
//! `__kernel_cos`, domínio público da Sun) em precisão dupla-dupla:
//!
//! - `|x| < 2^-32`: seno devolve `x` e cosseno devolve 1, exatos em 64 bits de mantissa
//!   (o termo seguinte da série fica abaixo de meio ulp);
//! - `|x| < 2^62`: `x − n·π₆₆/2` exato e polinômio em `f64` com cauda. O erro fica abaixo
//!   de 1 ulp de `f64`; os bits baixos da mantissa estendida não são reproduzidos;
//! - acima disso o modelo recusa. O hardware aceita até 2^63 e acima disso marca C2
//!   sem alterar ST(0), mas a palavra de estado não existe no modelo.
//!
//! Em Rust puro e determinístico em qualquer host; a libm do sistema não é usada.

use super::Extended;
use super::arith::{Classe, INDEFINITE};

/// π com 66 bits significativos, escalado por 2^64 (`0x3.243F6A8885A308D3`). O bit
/// seguinte de π é zero, então truncar e arredondar coincidem.
const PI66: i128 = 0x3_243F_6A88_85A3_08D3;

// Coeficientes de `k_sin.c`/`k_cos.c` do fdlibm, pelo padrão de bits dos comentários
// hexadecimais da fonte (o teste confere cada um contra o decimal original).
const S1: f64 = f64::from_bits(0xBFC5_5555_5555_5549); // -1.66666666666666324348e-01
const S2: f64 = f64::from_bits(0x3F81_1111_1110_F8A6); // 8.33333333332248946124e-03
const S3: f64 = f64::from_bits(0xBF2A_01A0_19C1_61D5); // -1.98412698298579493134e-04
const S4: f64 = f64::from_bits(0x3EC7_1DE3_57B1_FE7D); // 2.75573137070700676789e-06
const S5: f64 = f64::from_bits(0xBE5A_E5E6_8A2B_9CEB); // -2.50507602534068634195e-08
const S6: f64 = f64::from_bits(0x3DE5_D93A_5ACF_D57C); // 1.58969099521155010221e-10

const C1: f64 = f64::from_bits(0x3FA5_5555_5555_554C); // 4.16666666666666019037e-02
const C2: f64 = f64::from_bits(0xBF56_C16C_16C1_5177); // -1.38888888888741095749e-03
const C3: f64 = f64::from_bits(0x3EFA_01A0_19CB_1590); // 2.48015872894767294178e-05
const C4: f64 = f64::from_bits(0xBE92_7E4F_809C_52AD); // -2.75573143513906633035e-07
const C5: f64 = f64::from_bits(0x3E21_EE9E_BDB4_B1C4); // 2.08757232129817482790e-09
const C6: f64 = f64::from_bits(0xBDA8_FAE9_BE88_38D4); // -1.13596475577881948265e-11

/// Limite do recorte: `|x| < 2^62` (o hardware vai até 2^63).
pub const LIMITE: f64 = 4_611_686_018_427_387_904.0;

// Nomes do fdlibm (x, y, z, w, r, v), para conferir linha a linha com a fonte.
#[allow(clippy::many_single_char_names)]
fn kernel_sin(x: f64, y: f64, com_cauda: bool) -> f64 {
    let z = x * x;
    let w = z * z;
    let r = S2 + z * (S3 + z * S4) + z * w * (S5 + z * S6);
    let v = z * x;
    if com_cauda {
        x - ((z * (0.5 * y - v * r) - y) - v * S1)
    } else {
        x + v * (S1 + z * r)
    }
}

// Nomes do fdlibm, como em `kernel_sin`.
#[allow(clippy::many_single_char_names)]
fn kernel_cos(x: f64, y: f64) -> f64 {
    let z = x * x;
    let w = z * z;
    let r = z * (C1 + z * (C2 + z * C3)) + w * w * (C4 + z * (C5 + z * C6));
    let hz = 0.5 * z;
    let w = 1.0 - hz;
    w + (((1.0 - w) - hz) + (z * r - x * y))
}

/// Argumento reduzido `r = x − n·π₆₆/2` como par dupla-dupla `(hi, lo)` e o
/// quadrante `n mod 4`. `x = (-1)^sinal × mantissa × 2^(expoente − 63)`, `|x| < 2^62`.
fn reduzir(sinal: bool, expoente: i32, mantissa: u64) -> (u8, f64, f64) {
    let (inteiro, escala, n) = if expoente < -1 {
        // |x| < 1/2 < π/4: sem redução; x exato na escala da própria mantissa.
        (i128::from(mantissa), expoente - 63, 0i128)
    } else {
        // Escala comum 2^-65: x = X·2^-65 e π₆₆/2 = PI66·2^-65.
        let x = i128::from(mantissa) << (expoente + 2);
        let n = (x + PI66 / 2) / PI66;
        (x - n * PI66, -65, n)
    };
    let inteiro = if sinal { -inteiro } else { inteiro };
    let n = if sinal { -n } else { n };
    let escala = 2f64.powi(escala);
    #[allow(clippy::cast_precision_loss)]
    let hi_inteiro = inteiro as f64;
    #[allow(clippy::cast_possible_truncation)]
    let resto = inteiro - hi_inteiro as i128;
    #[allow(clippy::cast_precision_loss)]
    let lo = resto as f64 * escala;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    (n.rem_euclid(4) as u8, hi_inteiro * escala, lo)
}

fn seno(quadrante: u8, hi: f64, lo: f64) -> f64 {
    match quadrante {
        0 => kernel_sin(hi, lo, true),
        1 => kernel_cos(hi, lo),
        2 => -kernel_sin(hi, lo, true),
        _ => -kernel_cos(hi, lo),
    }
}

fn cosseno(quadrante: u8, hi: f64, lo: f64) -> f64 {
    match quadrante {
        0 => kernel_cos(hi, lo),
        1 => -kernel_sin(hi, lo, true),
        2 => -kernel_cos(hi, lo),
        _ => kernel_sin(hi, lo, true),
    }
}

/// `2^-32` como limiar do caminho exato.
const MINUSCULO: i32 = -32;

impl Extended {
    fn trig(self, seno: bool) -> Option<Self> {
        match self.classe() {
            Classe::Invalido | Classe::Infinito(_) => Some(INDEFINITE),
            Classe::Nan(_) => Some(Self {
                mantissa: self.mantissa | 1 << 62,
                sign_exponent: self.sign_exponent,
            }),
            Classe::Zero(_) => Some(if seno { self } else { Self::ONE }),
            Classe::Finito { expoente, .. } if expoente < MINUSCULO => {
                Some(if seno { self } else { Self::ONE })
            }
            Classe::Finito { expoente, .. } if expoente >= 62 => None,
            Classe::Finito {
                sinal,
                expoente,
                mantissa,
            } => {
                let (quadrante, hi, lo) = reduzir(sinal, expoente, mantissa);
                let y = if seno {
                    self::seno(quadrante, hi, lo)
                } else {
                    cosseno(quadrante, hi, lo)
                };
                Some(Self::from_f64_bits(y.to_bits()))
            }
        }
    }

    /// FSIN. `None` quando `|ST(0)| ≥ LIMITE`, fora do recorte declarado.
    pub fn sine(self) -> Option<Self> {
        self.trig(true)
    }

    /// FCOS. `None` quando `|ST(0)| ≥ LIMITE`, fora do recorte declarado.
    pub fn cosine(self) -> Option<Self> {
        self.trig(false)
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    fn ext(v: f64) -> Extended {
        Extended::from_f64_bits(v.to_bits())
    }

    fn ulps(a: f64, b: f64) -> u64 {
        a.to_bits().abs_diff(b.to_bits())
    }

    #[test]
    fn coeficientes_conferem_com_os_decimais_do_fdlibm() {
        for (bits, decimal) in [
            (S1, "-1.66666666666666324348e-01"),
            (S2, "8.33333333332248946124e-03"),
            (S3, "-1.98412698298579493134e-04"),
            (S4, "2.75573137070700676789e-06"),
            (S5, "-2.50507602534068634195e-08"),
            (S6, "1.58969099521155010221e-10"),
            (C1, "4.16666666666666019037e-02"),
            (C2, "-1.38888888888741095749e-03"),
            (C3, "2.48015872894767294178e-05"),
            (C4, "-2.75573143513906633035e-07"),
            (C5, "2.08757232129817482790e-09"),
            (C6, "-1.13596475577881948265e-11"),
        ] {
            assert_eq!(bits, decimal.parse::<f64>().unwrap(), "{decimal}");
        }
    }

    #[test]
    fn longe_dos_zeros_concorda_com_a_biblioteca_do_host() {
        // A libm do host reduz com π exato; longe dos múltiplos de π a diferença da
        // redução por π₆₆ some no arredondamento para f64.
        let mut x = -40.0f64;
        while x < 40.0 {
            let (s, c) = (ext(x).sine().unwrap(), ext(x).cosine().unwrap());
            let (s, c) = (
                f64::from_bits(s.to_f64_bits()),
                f64::from_bits(c.to_f64_bits()),
            );
            if x.sin().abs() > 0.1 {
                assert!(ulps(s, x.sin()) <= 2, "sin({x})");
            }
            if x.cos().abs() > 0.1 {
                assert!(ulps(c, x.cos()) <= 2, "cos({x})");
            }
            x += 0.013_7;
        }
    }

    #[test]
    fn perto_de_pi_aparece_o_erro_de_pi66() {
        // sin(π_f64) ≈ π − π_f64. Com π exato: 1.2246467991e-16. Com π₆₆, que termina
        // em …08D3, a diferença é exatamente 0x8D3·2^-64 = 1.2246063538e-16.
        let s = f64::from_bits(ext(std::f64::consts::PI).sine().unwrap().to_f64_bits());
        assert!(ulps(s, 2259.0 * 2f64.powi(-64)) <= 1, "{s:e}");
        // A libm do Windows pode usar FSIN, com a mesma redução por pi66.
        // Referência matemática independente: Machin em 120 casas decimais,
        // sin(pi_f64) = sin(pi - pi_f64), arredondado uma vez para f64.
        let seno_matematico = f64::from_bits(0x3CA1_A626_3314_5C07);
        assert!(ulps(s, seno_matematico) > 1_000_000_000, "{s:e}");
    }

    #[test]
    fn valores_notaveis_e_casos_especiais() {
        assert_eq!(ext(0.0).sine().unwrap(), ext(0.0));
        assert_eq!(ext(-0.0).sine().unwrap(), ext(-0.0));
        assert_eq!(ext(0.0).cosine().unwrap(), Extended::ONE);
        let pequeno = Extended {
            mantissa: 0x8000_0000_0000_0001,
            sign_exponent: (super::super::EXPONENT_BIAS - 40) as u16,
        };
        assert_eq!(
            pequeno.sine().unwrap(),
            pequeno,
            "bits abaixo de f64 preservados"
        );
        assert_eq!(pequeno.cosine().unwrap(), Extended::ONE);
        assert_eq!(ext(f64::INFINITY).sine().unwrap(), INDEFINITE);
        assert_eq!(ext(std::f64::consts::FRAC_PI_2).sine().unwrap(), ext(1.0));
        assert_eq!(ext(LIMITE).sine(), None);
        assert_eq!(ext(-1e20).cosine(), None);
    }
}
