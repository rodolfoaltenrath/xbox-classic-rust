//! Aritmética x87 do runtime AOT contra a FPU física do host (ADR-0088).
//!
//! FMULP, FDIVRP e FSQRT precisam concordar nos 80 bits com a palavra de controle de
//! RESET; FSIN/FCOS têm precisão declarada e são comparados após arredondar para
//! `f64`, com no máximo 1 ulp de diferença.
#![cfg(target_arch = "x86_64")]

use xbox_cpu::oracle::x87::{Operacao, executar};
use xbox_recomp::runtime::x87::{Extended, LIMITE_TRIGONOMETRICO};

/// Gerador determinístico (xorshift64*), para os casos serem reproduzíveis.
struct Gerador(u64);

impl Gerador {
    fn proximo(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Valor estendido: maioria normais em faixa moderada, alguns extremos e especiais.
    fn estendido(&mut self) -> Extended {
        let r = self.proximo();
        let sinal = if r & 1 == 1 { 0x8000 } else { 0 };
        let mantissa = self.proximo() | 1 << 63;
        let campo = match (r >> 1) % 16 {
            0 => {
                return Extended {
                    mantissa: 0,
                    sign_exponent: sinal,
                };
            }
            1 => {
                return Extended {
                    mantissa: 1 << 63,
                    sign_exponent: 0x7FFF | sinal,
                };
            }
            2 => {
                return Extended {
                    mantissa: mantissa | 1 << 62,
                    sign_exponent: 0x7FFF | sinal,
                };
            }
            // Perto do menor normal e do maior finito: subfluxo, denormais e estouro.
            3 => 1 + (r >> 8) as u16 % 64,
            4 => 0x7FFE - (r >> 8) as u16 % 64,
            5 => {
                return Extended {
                    mantissa: mantissa >> (1 + (r >> 8) % 63),
                    sign_exponent: sinal,
                };
            }
            _ => 16383 - 200 + (r >> 8) as u16 % 400,
        };
        Extended {
            mantissa,
            sign_exponent: campo | sinal,
        }
    }
}

fn fisico(op: Operacao, a: Extended, b: Extended) -> Extended {
    Extended::from_bytes(executar(op, a.to_bytes(), b.to_bytes()))
}

#[test]
fn fmulp_fdivrp_e_fsqrt_concordam_nos_80_bits_em_30000_casos() {
    let mut g = Gerador(0x0B51_D1A4_0088_0001);
    for caso in 0..10_000 {
        let (a, b) = (g.estendido(), g.estendido());
        assert_eq!(
            a.multiply(b),
            fisico(Operacao::Fmulp, a, b),
            "#{caso} {a:?} × {b:?}"
        );
        assert_eq!(
            b.divide(a),
            fisico(Operacao::Fdivrp, a, b),
            "#{caso} {b:?} ÷ {a:?}"
        );
        assert_eq!(
            a.square_root(),
            fisico(Operacao::Fsqrt, a, a),
            "#{caso} √{a:?}"
        );
    }
}

#[test]
fn fsin_e_fcos_ficam_a_um_ulp_de_f64_do_hardware() {
    let mut g = Gerador(0x51A0_C05E_0088_0002);
    let mut exatos = 0;
    let mut total = 0;
    for caso in 0..20_000 {
        // Ângulos em várias escalas: |x| de 2^-40 até 2^61, perto do limite do recorte.
        let escala = (g.proximo() % 102) as i32 - 40;
        // [0, 1) exato: mantissa aleatória em [1, 2), menos 1.
        let fracao = f64::from_bits(0x3FF0_0000_0000_0000 | g.proximo() >> 12) - 1.0;
        let mut x = fracao * 2f64.powi(escala.min(61));
        if g.proximo() & 1 == 1 {
            x = -x;
        }
        if x.abs() >= LIMITE_TRIGONOMETRICO {
            continue;
        }
        let a = Extended::from_f64_bits(x.to_bits());
        for (op, nosso) in [(Operacao::Fsin, a.sine()), (Operacao::Fcos, a.cosine())] {
            let nosso = nosso.unwrap().to_f64_bits();
            let hw = fisico(op, a, a).to_f64_bits();
            let ulps = nosso.abs_diff(hw);
            assert!(
                ulps <= 1,
                "#{caso} {op:?}({x:e}): {nosso:016X} × hw {hw:016X}"
            );
            exatos += usize::from(ulps == 0);
            total += 1;
        }
    }
    // Registra a taxa no log do teste; o contrato é o limite de 1 ulp.
    eprintln!("FSIN/FCOS: {exatos}/{total} idênticos ao hardware após arredondar para f64");
    assert!(
        exatos * 10 >= total * 9,
        "menos de 90% idênticos: {exatos}/{total}"
    );
}

#[test]
fn fsin_e_fcos_recusam_fora_do_recorte() {
    for x in [LIMITE_TRIGONOMETRICO, -1e19, 9.3e18] {
        let a = Extended::from_f64_bits(x.to_bits());
        assert_eq!(a.sine(), None, "{x}");
        assert_eq!(a.cosine(), None, "{x}");
    }
}
