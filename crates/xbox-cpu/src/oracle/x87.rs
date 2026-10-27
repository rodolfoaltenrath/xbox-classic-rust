//! Referência x87 na FPU física do host (ADR-0088).
//!
//! Executa uma instrução fixa sobre valores de 80 bits com a palavra de controle de
//! RESET (0x037F: precisão estendida, arredondamento ao mais próximo, exceções
//! mascaradas), carregada dentro do próprio bloco. Não depende do padrão do sistema
//! (Linux usa 0x037F, Windows x64 usa 53 bits) e restaura a palavra do host ao sair.

use core::arch::asm;

/// Instruções x87 comparadas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operacao {
    /// `FMULP ST(1), ST(0)` com ST(1) = `a`, ST(0) = `b`: devolve `a × b`.
    Fmulp,
    /// `FDIVRP ST(1), ST(0)` com ST(1) = `a`, ST(0) = `b`: devolve `b ÷ a`.
    Fdivrp,
    /// `FSQRT` sobre `a`.
    Fsqrt,
    /// `FSIN` sobre `a`.
    Fsin,
    /// `FCOS` sobre `a`.
    Fcos,
}

/// Executa `operacao` e devolve os 10 bytes de ST(0) (para FSIN/FCOS fora de faixa,
/// o hardware deixa o operando inalterado e marca C2; isso é devolvido como está).
pub fn executar(operacao: Operacao, a: [u8; 10], b: [u8; 10]) -> [u8; 10] {
    let mut saida = [0u8; 10];
    let controle: u16 = 0x037F;
    let mut anterior: u16 = 0;
    macro_rules! binaria {
        ($instrucao:literal) => {
            // SAFETY: os ponteiros apontam para locais vivos do tamanho exato (2 e 10
            // bytes). A pilha x87 começa e termina vazia (duas cargas, uma retirada pela
            // instrução e uma pelo FSTP); os oito registradores são declarados
            // destruídos, a palavra de controle do host é restaurada e FNCLEX limpa os
            // flags de exceção mascarada antes de devolver ao código Rust (que usa SSE).
            unsafe {
                asm!(
                    "fnstcw word ptr [{anterior}]",
                    "fldcw word ptr [{controle}]",
                    "fld tbyte ptr [{a}]",
                    "fld tbyte ptr [{b}]",
                    $instrucao,
                    "fstp tbyte ptr [{saida}]",
                    "fnclex",
                    "fldcw word ptr [{anterior}]",
                    anterior = in(reg) &raw mut anterior,
                    controle = in(reg) &raw const controle,
                    a = in(reg) &raw const a,
                    b = in(reg) &raw const b,
                    saida = in(reg) &raw mut saida,
                    out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _,
                    out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _,
                    options(nostack),
                );
            }
        };
    }
    macro_rules! unaria {
        ($instrucao:literal) => {
            // SAFETY: mesmas invariantes de `binaria`, com uma carga e um FSTP.
            unsafe {
                asm!(
                    "fnstcw word ptr [{anterior}]",
                    "fldcw word ptr [{controle}]",
                    "fld tbyte ptr [{a}]",
                    $instrucao,
                    "fstp tbyte ptr [{saida}]",
                    "fnclex",
                    "fldcw word ptr [{anterior}]",
                    anterior = in(reg) &raw mut anterior,
                    controle = in(reg) &raw const controle,
                    a = in(reg) &raw const a,
                    saida = in(reg) &raw mut saida,
                    out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _,
                    out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _,
                    options(nostack),
                );
            }
        };
    }
    match operacao {
        Operacao::Fmulp => binaria!("fmulp st(1), st"),
        Operacao::Fdivrp => binaria!("fdivrp st(1), st"),
        Operacao::Fsqrt => unaria!("fsqrt"),
        Operacao::Fsin => unaria!("fsin"),
        Operacao::Fcos => unaria!("fcos"),
    }
    saida
}

#[cfg(test)]
mod tests {
    use super::{Operacao, executar};

    fn ext(v: f64) -> [u8; 10] {
        // 1.5 = mantissa 0xC000…, expoente 0x3FFF; montado à mão para não depender
        // de conversões do runtime AOT.
        let bits = v.to_bits();
        let expoente = ((bits >> 52) & 0x7FF) as u16;
        let fracao = bits & ((1 << 52) - 1);
        let mut b = [0u8; 10];
        let mantissa = (1u64 << 63) | (fracao << 11);
        b[..8].copy_from_slice(&mantissa.to_le_bytes());
        let campo = (expoente + 16383 - 1023) | if v < 0.0 { 0x8000 } else { 0 };
        b[8..].copy_from_slice(&campo.to_le_bytes());
        b
    }

    #[test]
    fn operacoes_exatas_na_fpu_fisica() {
        assert_eq!(executar(Operacao::Fmulp, ext(1.5), ext(2.0)), ext(3.0));
        assert_eq!(executar(Operacao::Fdivrp, ext(2.0), ext(3.0)), ext(1.5));
        assert_eq!(executar(Operacao::Fsqrt, ext(9.0), [0; 10]), ext(3.0));
        assert_eq!(executar(Operacao::Fcos, ext(1e-300), [0; 10]), ext(1.0));
    }
}
