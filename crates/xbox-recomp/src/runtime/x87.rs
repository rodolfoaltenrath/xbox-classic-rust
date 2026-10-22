//! Pilha x87: valores de 80 bits **exatos**, sem passar por `f64`.
//!
//! O Rust não tem tipo de 80 bits, e converter para `f64` na pilha mudaria o resultado de
//! qualquer cadeia com mais de um arredondamento. Por isso cada registrador guarda a
//! mantissa de 64 bits e o par sinal/expoente como o hardware, e só a **saída** para
//! 32/64 bits arredonda — uma vez, ao par mais próximo, como a palavra de controle de
//! RESET manda. O que está implementado é o que alvos reais usaram: carga de inteiro e de
//! ponto flutuante, `FLDZ`/`FLD1`, e `FST`/`FSTP`. Aritmética x87 não existe ainda e é
//! recusada na análise com o nome da instrução.
//!
//! **Fora do modelo (ADR-0053, P9):** palavras de controle e de estado (`FLDCW`, `FNSTCW`,
//! `FNSTSW`), exceções x87, precisão de 24/53 bits, não normais e pseudo-denormais.

use xbox_core::snapshot::{SnapshotReader, SnapshotWriter};

use crate::Error;

const SIGN: u16 = 0x8000;
const MAX_EXPONENT: u16 = 0x7FFF;
const EXPONENT_BIAS: i32 = 16383;
const INTEGER_BIT: u64 = 1 << 63;

/// Um valor de precisão estendida do x87.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Extended {
    /// Mantissa com o bit inteiro explícito no bit 63.
    pub mantissa: u64,
    /// Bit 15 = sinal; bits 0..15 = expoente com viés 16383.
    pub sign_exponent: u16,
}

impl Extended {
    pub const ZERO: Self = Self {
        mantissa: 0,
        sign_exponent: 0,
    };
    pub const ONE: Self = Self {
        mantissa: INTEGER_BIT,
        sign_exponent: EXPONENT_BIAS as u16,
    };

    /// Inteiro de 64 bits com sinal: sempre exato (cabe nos 64 bits da mantissa).
    pub fn from_i64(value: i64) -> Self {
        if value == 0 {
            return Self::ZERO;
        }
        let magnitude = value.unsigned_abs();
        let lead = magnitude.leading_zeros();
        let exponent = EXPONENT_BIAS + 63 - lead.cast_signed();
        Self {
            mantissa: magnitude << lead,
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            sign_exponent: exponent as u16 | if value < 0 { SIGN } else { 0 },
        }
    }

    /// Alarga um `f32` (bits crus) sem perda.
    pub fn from_f32_bits(bits: u32) -> Self {
        Self::widen(u64::from(bits), 23, 8, 127)
    }

    /// Alarga um `f64` (bits crus) sem perda.
    pub fn from_f64_bits(bits: u64) -> Self {
        Self::widen(bits, 52, 11, 1023)
    }

    fn widen(raw: u64, fraction_bits: u32, exponent_bits: u32, bias: i32) -> Self {
        let sign = if raw >> (fraction_bits + exponent_bits) & 1 == 1 {
            SIGN
        } else {
            0
        };
        let fraction = raw & ((1 << fraction_bits) - 1);
        let exponent = (raw >> fraction_bits) & ((1 << exponent_bits) - 1);
        let all_ones = (1 << exponent_bits) - 1;
        if exponent == all_ones {
            // Infinito (fração nula) ou NaN: expoente máximo, bit inteiro ligado.
            return Self {
                mantissa: INTEGER_BIT | fraction << (63 - fraction_bits),
                sign_exponent: MAX_EXPONENT | sign,
            };
        }
        if exponent == 0 {
            if fraction == 0 {
                return Self {
                    mantissa: 0,
                    sign_exponent: sign,
                };
            }
            // Subnormal: normaliza, porque o x87 representa todo valor como normal.
            // valor = fração × 2^(1 − viés − bits_da_fração); o bit mais alto da fração
            // fica em `p` e vira o bit inteiro da mantissa de 64 bits.
            let lead = fraction.leading_zeros();
            let p = 63 - lead.cast_signed();
            let unbiased = p + 1 - bias - fraction_bits.cast_signed();
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            return Self {
                mantissa: fraction << lead,
                sign_exponent: (unbiased + EXPONENT_BIAS) as u16 | sign,
            };
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        Self {
            mantissa: INTEGER_BIT | fraction << (63 - fraction_bits),
            sign_exponent: (exponent as i32 - bias + EXPONENT_BIAS) as u16 | sign,
        }
    }

    /// Arredonda para `f32` ao par mais próximo e devolve os bits.
    pub fn to_f32_bits(self) -> u32 {
        #[allow(clippy::cast_possible_truncation)]
        let bits = self.narrow(23, 8, 127) as u32;
        bits
    }

    /// Arredonda para `f64` ao par mais próximo e devolve os bits.
    pub fn to_f64_bits(self) -> u64 {
        self.narrow(52, 11, 1023)
    }

    /// Os 10 bytes de `FSTP m80`: mantissa e depois sinal/expoente, little-endian.
    pub fn to_bytes(self) -> [u8; 10] {
        let mut out = [0; 10];
        out[..8].copy_from_slice(&self.mantissa.to_le_bytes());
        out[8..].copy_from_slice(&self.sign_exponent.to_le_bytes());
        out
    }

    /// Inverso de [`Extended::to_bytes`].
    pub fn from_bytes(bytes: [u8; 10]) -> Self {
        Self {
            mantissa: u64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]),
            sign_exponent: u16::from_le_bytes([bytes[8], bytes[9]]),
        }
    }

    fn narrow(self, fraction_bits: u32, exponent_bits: u32, bias: i32) -> u64 {
        let sign = u64::from(self.sign_exponent & SIGN != 0) << (fraction_bits + exponent_bits);
        let exponent_field = self.sign_exponent & MAX_EXPONENT;
        let all_ones = (1u64 << exponent_bits) - 1;
        if exponent_field == MAX_EXPONENT {
            let fraction = (self.mantissa & !INTEGER_BIT) >> (63 - fraction_bits);
            // NaN precisa de fração não nula; um NaN cuja fração alta é zero vira quiet NaN.
            let nan = self.mantissa & !INTEGER_BIT != 0;
            let fraction = if nan {
                fraction | 1 << (fraction_bits - 1)
            } else {
                0
            };
            return sign | all_ones << fraction_bits | fraction;
        }
        if self.mantissa == 0 {
            return sign;
        }
        let exponent = i32::from(exponent_field) - EXPONENT_BIAS;
        let minimum = 1 - bias;
        let maximum = bias;
        // Quantos bits baixos da mantissa de 64 saem: 63 - fraction_bits para normal;
        // subnormais descartam mais.
        let mut drop = 63 - fraction_bits;
        if exponent < minimum {
            drop += (minimum - exponent).unsigned_abs();
        }
        let mantissa = u128::from(self.mantissa);
        let kept = if drop >= 128 { 0 } else { mantissa >> drop };
        let remainder = if drop >= 128 {
            mantissa
        } else {
            mantissa & ((1u128 << drop) - 1)
        };
        let half = if drop >= 128 {
            u128::MAX
        } else {
            1u128 << (drop - 1)
        };
        let round_up = remainder > half || (remainder == half && kept & 1 == 1);
        let kept = kept + u128::from(round_up);
        if exponent < minimum {
            // Subnormal (ou zero): `kept` já é a fração; carry para o expoente é natural.
            #[allow(clippy::cast_possible_truncation)]
            return sign | kept as u64;
        }
        // Normal: `kept` tem o bit inteiro em `fraction_bits`; carry pode torná-lo 2^(f+1).
        let mut exponent = exponent;
        let mut kept = kept;
        if kept >> (fraction_bits + 1) != 0 {
            kept >>= 1;
            exponent += 1;
        }
        if exponent > maximum {
            return sign | all_ones << fraction_bits; // overflow mascarado: infinito
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let biased = (exponent + bias) as u64;
        #[allow(clippy::cast_possible_truncation)]
        let fraction = kept as u64 & ((1u64 << fraction_bits) - 1);
        sign | biased << fraction_bits | fraction
    }
}

/// Falha de pilha x87: sobre/subfluxo (#IS no hardware).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StackFault {
    Overflow,
    Underflow,
}

/// A pilha de 8 registradores. `top` é o índice físico de ST(0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fpu {
    top: u8,
    /// Bit `n` ligado = registrador físico `n` ocupado.
    valid: u8,
    regs: [Extended; 8],
}

impl Default for Fpu {
    fn default() -> Self {
        Self::new()
    }
}

impl Fpu {
    pub const fn new() -> Self {
        Self {
            top: 0,
            valid: 0,
            regs: [Extended::ZERO; 8],
        }
    }

    /// Quantos valores a pilha tem.
    pub const fn depth(&self) -> u32 {
        self.valid.count_ones()
    }

    pub fn push(&mut self, value: Extended) -> Result<(), StackFault> {
        let next = self.top.wrapping_sub(1) & 7;
        if self.valid >> next & 1 == 1 {
            return Err(StackFault::Overflow);
        }
        self.top = next;
        self.valid |= 1 << next;
        self.regs[usize::from(next)] = value;
        Ok(())
    }

    /// ST(0) sem retirá-lo.
    pub fn peek(&self) -> Result<Extended, StackFault> {
        if self.valid >> self.top & 1 == 0 {
            return Err(StackFault::Underflow);
        }
        Ok(self.regs[usize::from(self.top)])
    }

    pub fn pop(&mut self) -> Result<Extended, StackFault> {
        let value = self.peek()?;
        self.valid &= !(1 << self.top);
        self.top = (self.top + 1) & 7;
        Ok(value)
    }

    pub(super) fn save(&self, out: &mut SnapshotWriter) {
        out.u32(u32::from(self.top));
        out.u32(u32::from(self.valid));
        for r in &self.regs {
            out.u64(r.mantissa);
            out.u32(u32::from(r.sign_exponent));
        }
    }

    pub(super) fn load(
        &mut self,
        inp: &mut SnapshotReader<'_>,
    ) -> Result<(), xbox_core::error::SnapshotError> {
        let bad = |what| xbox_core::error::SnapshotError::BadValue {
            section: "AOTC",
            what,
        };
        let top = u8::try_from(inp.u32()?).map_err(|_| bad("TOP do x87 fora do intervalo"))?;
        let valid = u8::try_from(inp.u32()?).map_err(|_| bad("tags do x87 fora do intervalo"))?;
        if top > 7 {
            return Err(bad("TOP do x87 fora do intervalo"));
        }
        let mut regs = [Extended::ZERO; 8];
        for r in &mut regs {
            r.mantissa = inp.u64()?;
            r.sign_exponent =
                u16::try_from(inp.u32()?).map_err(|_| bad("expoente do x87 fora do intervalo"))?;
        }
        self.top = top;
        self.valid = valid;
        self.regs = regs;
        Ok(())
    }
}

impl From<StackFault> for Error {
    fn from(fault: StackFault) -> Self {
        Self::FpuStack(fault)
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp, clippy::cast_precision_loss)] // comparações exatas são o contrato
mod tests {
    use super::*;

    #[test]
    fn inteiro_de_64_bits_e_exato_e_normalizado() {
        assert_eq!(Extended::from_i64(0), Extended::ZERO);
        assert_eq!(Extended::from_i64(1), Extended::ONE);
        let tres = Extended::from_i64(3);
        assert_eq!(tres.mantissa, 0xC000_0000_0000_0000);
        assert_eq!(tres.sign_exponent, 16384);
        assert_eq!(Extended::from_i64(-1).sign_exponent, 16383 | SIGN);
        // i64::MIN = -2^63: mantissa só com o bit inteiro, expoente 63.
        let minimo = Extended::from_i64(i64::MIN);
        assert_eq!(minimo.mantissa, INTEGER_BIT);
        assert_eq!(minimo.sign_exponent, (16383 + 63) | SIGN);
    }

    #[test]
    fn f64_ida_e_volta_preserva_todos_os_bits_representaveis() {
        for v in [
            0.0f64,
            -0.0,
            1.0,
            -1.5,
            0.1,
            1e300,
            -1e-300,
            f64::MAX,
            f64::MIN_POSITIVE,
            5e-324,                                // menor subnormal
            f64::from_bits(0x000F_FFFF_FFFF_FFFF), // maior subnormal
            f64::INFINITY,
            f64::NEG_INFINITY,
        ] {
            let ida_e_volta = Extended::from_f64_bits(v.to_bits()).to_f64_bits();
            assert_eq!(ida_e_volta, v.to_bits(), "{v:e}");
        }
        let nan = Extended::from_f64_bits(f64::NAN.to_bits()).to_f64_bits();
        assert!(f64::from_bits(nan).is_nan());
    }

    #[test]
    fn f32_ida_e_volta_preserva_todos_os_bits_representaveis() {
        for v in [
            0.0f32,
            -0.0,
            1.0,
            -2.5,
            0.1,
            f32::MAX,
            f32::MIN_POSITIVE,
            1e-45,
            f32::INFINITY,
        ] {
            assert_eq!(
                Extended::from_f32_bits(v.to_bits()).to_f32_bits(),
                v.to_bits(),
                "{v:e}"
            );
        }
    }

    #[test]
    fn inteiro_grande_arredonda_uma_vez_ao_par_mais_proximo() {
        // 2^53 + 1 não cabe em f64: empate entre 2^53 e 2^53+2, vai para o par (2^53).
        let v = (1i64 << 53) + 1;
        assert_eq!(Extended::from_i64(v).to_f64_bits(), (v as f64).to_bits());
        assert_eq!(
            Extended::from_i64(v).to_f64_bits(),
            ((1u64 << 53) as f64).to_bits()
        );
        // 2^53 + 3: empate entre 2^53+2 e 2^53+4, vai para o par (2^53+4).
        let v = (1i64 << 53) + 3;
        assert_eq!(Extended::from_i64(v).to_f64_bits(), (v as f64).to_bits());
        // Casos arbitrários concordam com o `as f64` do Rust, que arredonda ao par.
        for v in [
            i64::MAX,
            i64::MIN,
            123_456_789_012_345_678,
            -987_654_321_098_765_432,
            16_777_217,
        ] {
            assert_eq!(
                Extended::from_i64(v).to_f64_bits(),
                (v as f64).to_bits(),
                "{v}"
            );
            assert_eq!(
                Extended::from_i64(v).to_f32_bits(),
                (v as f32).to_bits(),
                "{v} f32"
            );
        }
    }

    #[test]
    fn estouro_e_subfluxo_ao_estreitar() {
        // 2^200 estoura f32 (infinito) mas cabe em f64.
        let grande = Extended {
            mantissa: INTEGER_BIT,
            sign_exponent: (EXPONENT_BIAS + 200) as u16,
        };
        assert_eq!(grande.to_f32_bits(), f32::INFINITY.to_bits());
        assert_eq!(grande.to_f64_bits(), 2f64.powi(200).to_bits());
        // 2^-2000 some em f64.
        let pequeno = Extended {
            mantissa: INTEGER_BIT,
            sign_exponent: (EXPONENT_BIAS - 2000) as u16,
        };
        assert_eq!(pequeno.to_f64_bits(), 0);
    }

    #[test]
    fn pilha_empilha_desempilha_e_detecta_sobre_e_subfluxo() {
        let mut fpu = Fpu::new();
        assert_eq!(fpu.pop(), Err(StackFault::Underflow));
        assert_eq!(fpu.peek(), Err(StackFault::Underflow));
        for i in 0..8 {
            fpu.push(Extended::from_i64(i)).unwrap();
        }
        assert_eq!(fpu.depth(), 8);
        assert_eq!(fpu.push(Extended::ONE), Err(StackFault::Overflow));
        for i in (0..8).rev() {
            assert_eq!(fpu.pop().unwrap(), Extended::from_i64(i));
        }
        assert_eq!(fpu.depth(), 0);
    }

    #[test]
    fn bytes_de_80_bits_ida_e_volta() {
        let v = Extended::from_i64(-12345);
        assert_eq!(Extended::from_bytes(v.to_bytes()), v);
    }

    #[test]
    fn snapshot_ida_e_volta_e_recusa_valores_invalidos() {
        let mut fpu = Fpu::new();
        fpu.push(Extended::from_i64(42)).unwrap();
        fpu.push(Extended::from_f64_bits(1.5f64.to_bits())).unwrap();
        let mut out = SnapshotWriter::new();
        out.section("AOTC", 8);
        fpu.save(&mut out);
        let bytes = out.finish();
        let mut restored = Fpu::new();
        let mut reader = SnapshotReader::new(&bytes).unwrap();
        reader.section("AOTC", 8).unwrap();
        restored.load(&mut reader).unwrap();
        assert_eq!(restored, fpu);

        let mut ruim = SnapshotWriter::new();
        ruim.section("AOTC", 8);
        ruim.u32(9); // TOP > 7
        let bytes = ruim.finish();
        let mut reader = SnapshotReader::new(&bytes).unwrap();
        reader.section("AOTC", 8).unwrap();
        assert!(Fpu::new().load(&mut reader).is_err());
    }
}
