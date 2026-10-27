# ADR-0088 — Aritmética x87 do mesh

Data: 2026-10-26. Estado: aceito. PLANO 9.2, provisório P9.

## Contexto

O ADR-0087 mostrou que o `mesh` para na tradução em `fmulp st(1), st`
(0x1269E) e que, no binário inteiro, faltam apenas `FMULP ST(1),ST`,
`FDIVRP ST(1),ST`, `FSQRT`, `FSIN` e `FCOS`. Não há `FLDCW`/`FNSTSW`: a
palavra de controle do RESET (precisão estendida, arredondamento ao mais
próximo, exceções mascaradas) é a única usada. A pilha já guarda valores de 80
bits exatos (ADR-0053).

## Decisão

1. **FMULP, FDIVRP e FSQRT exatos.** `runtime/x87/arith.rs` desempacota os
   operandos (normais, denormais e pseudo-denormais com o mesmo valor que o
   hardware dá a eles), multiplica com produto de 128 bits, divide por divisão
   longa em duas palavras com bit de sticky e extrai a raiz com `u128::isqrt`
   mais a correção do arredondamento. O resultado é arredondado uma vez, ao par,
   para 64 bits de mantissa. As respostas mascaradas seguem o SDM: NaN
   indefinido para inválidas (∞×0, 0/0, ∞/∞, raiz de negativo, unnormal),
   infinito para divisão por zero e estouro, denormal ou zero no subfluxo, e
   propagação de NaN pela maior mantissa, sempre silenciado.
2. **FSIN/FCOS com a redução do hardware.** O x87 reduz o argumento com π de 66
   bits (SDM vol. 1 §8.3.10). O modelo calcula `x − n·π₆₆/2` exato em inteiros
   de 128 bits e avalia o argumento reduzido com os polinômios do fdlibm em
   precisão dupla-dupla, em Rust puro, sem a libm do host. Para `|x| < 2^-32`
   devolve `x` ou 1 exatos. Recorte: `|x| < 2^62`. Acima disso a execução
   recusa com `Error::X87OutOfProfile { pc, instruction }` e deixa a pilha
   intacta; o hardware iria até 2^63 e marcaria C2, mas a palavra de estado não
   existe no modelo.
3. **IR e emissão.** `X87::MultiplyPop(i)`, `X87::DivideReversePop(i)` e
   `X87::Unary(SquareRoot|Sine|Cosine)`. Os operandos são lidos (e a recusa
   decidida) antes de qualquer escrita, então falhas não deixam efeito parcial.
   Snapshot inalterado: o formato da pilha não mudou.

Qualquer outra instrução aritmética x87 continua recusada na análise com o nome.

## Evidência

- `xbox-cpu::oracle::x87` executa a instrução na FPU física do host com a
  palavra de controle 0x037F carregada no próprio bloco (Windows x64 usa 53 bits
  por padrão; Linux, 64) e restaurada ao sair.
- `crates/xbox-recomp/tests/x87_hardware.rs`: **30000 casos** gerados por
  xorshift fixo (normais em ±200 de expoente, perto do menor normal e do maior
  finito, denormais, zeros, infinitos e NaNs) com FMULP, FDIVRP e FSQRT
  **idênticos nos 80 bits** ao hardware.
- **40000 casos** de FSIN/FCOS com `|x|` de 2^-40 a 2^61: todos a no máximo 1
  ulp de `f64` do hardware, **39246 idênticos** após arredondar para `f64`. A
  primeira versão, com a redução por π exato do fdlibm, errava 2 ulps em
  sin(562,348) ≈ −0,003, perto de um zero; foi o que levou à redução por π₆₆.
  sin(π_f64) dá 0x8D3·2^-64, a diferença π₆₆ − π_f64, como no hardware.
- Testes unitários: 1/3 com mantissa `0xAAAA…AAAB`, √2 com `0xB504F333F9DE6484`,
  casos especiais, estouro/subfluxo e acesso ST(i).
- Teste nativo: o programa x86 escrito à mão com as cinco instruções é
  traduzido, compilado e executado (fmulp = 3, fdivrp = ST(0)/ST(1) = 1/3, fsqrt,
  fsin e fcos iguais ao runtime). FSIN de 1e19 recusa com a pilha intacta. Total
  de casos gerados: 271 em x86-64.
- `bash scripts/sonda.sh mesh`: o mesh traduz e executa 320 fatias, com 281
  chamadas de kernel e **zero recusas**; o clear `0xFF202020` cobre a tela.
  Nenhum triângulo ainda: os desenhos chegam por `INDEX_DATA` (0x1800, 5832
  palavras), sem modelo no PGRAPH (log `target/sonda/mesh-20261026-220157-QsTArM0Z.log`).

## Consequências

P9 deixa de ser "só carga e armazenamento": as cinco instruções do mesh têm
modelo verificado no hardware. Seguem pendentes palavras de controle/estado,
flags de exceção, precisão 24/53 e o resto da aritmética x87. O próximo passo do
mesh é o PGRAPH (desenho indexado, textura, opcodes de VP e combiner).
