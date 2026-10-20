# ADR-0030 — BSF/BSR e política determinística para origem zero

Data: 2026-10-20. Estado: aceito no perfil inteiro AOT atual.

## Contexto

Após CMOVcc (ADR-0029), a análise do nxdk hello parava em BSR no PC `0x1657C`,
bytes `0F BD CB` (EBX para ECX), seguido de JZ em `0x1657F`. O caminho precisa
de ZF correto para distinguir origem zero. A referência Intel da geração do
alvo deixa destino em zero e as demais flags aritméticas indefinidos.

## Decisão

Adicionar `BitScan::{Forward, Reverse}` e `Operation::BitScan { kind, dst, src }`
para **BSF e BSR de 16/32 bits**, com destino registrador e origem
registrador/memória da mesma largura. Capturar a origem antes de alterar
destino ou flags. O operando word é mascarado antes do scan; bits altos do
registrador de origem não participam.

- Origem não zero: BSF grava o índice do bit 1 menos significativo; BSR grava
  o do mais significativo. ZF=0. O índice começa em zero; word preserva os
  16 bits altos do destino.
- Origem zero: ZF=1 e **preservar o destino inteiro**, como política local
  determinística para o caso indefinido da referência clássica.
- CF/PF/AF/SF/OF: **normalizar a zero** em ambos os casos. São indefinidos nessa
  referência; não copiar o resultado incidental da CPU física do host.
- Preservar flags não aritméticas, inclusive IF/DF. Uma leitura que falha não
  altera CPU nem cobra o passo, mantendo os efeitos de leitura já previstos
  pelo bus. Aliases usam o valor/endereço anterior à escrita do destino.

Não chamar `leading_zeros`/`trailing_zeros` como se zero tivesse índice válido;
guardar o caso zero antes do cálculo. Em valores word não zero, o scan em u32
mascarado produz o índice correto de 0 a 15. Em dword, o intervalo é 0 a 31.

LOCK, REP/REPNE, TZCNT/LZCNT, segmentos fora do perfil e memória com endereço
de 16 bits continuam recusados. Não há operação byte ou modo guest de 64 bits.
A descoberta indireta permanece conservadora e não propaga constantes pelo scan.

Não há estado ou fila nova: **AOTC permanece v6** e o perfil de tempo permanece
1. Cada instrução concluída, inclusive com origem zero, cobra um passo sintético;
o limite é verificado antes de alterar registradores/flags.

## Verificação

O oráculo opcional de `xbox-cpu` executa BSF/BSR físicos apenas em registradores
locais e usa pilha balanceada para capturar RFLAGS. O novo unsafe fica restrito
a esse assembly de teste, com comentário SAFETY; não recebe ponteiros guest ou
escreve flags de controle. O AOT, runtime e bus não ganharam unsafe.

Há **262144 comparações físicas** do Rust realmente emitido:

- BSF/BSR word: todos os 65536 valores de 16 bits, com bits altos de origem
  preenchidos para verificar o mascaramento;
- BSF/BSR dword: 65536 entradas por direção, incluindo zero, cada bit isolado,
  todos os bits e valores pseudoaleatórios;
- comparar destino apenas se a origem efetiva não for zero; comparar **somente
  ZF** entre as flags físicas. Nos casos zero, não usar o destino físico como
  contrato. A política de preservação/normalização tem testes próprios.

Seis testes portáveis adicionais cobrem índices, zero, flags, memória RO,
faltas/leitura cruzando página, aliases, FS/SIB, SP/ESP, branch por ZF,
snapshot/replay, fatias, orçamento zero e limite do Clock. O conjunto gerado
passa de 103 para **110 testes em x86_64** (94 em outros hosts).

## Resultado integrado

A análise do hello real passou com 8725 instruções/blocos. Sua primeira execução
expôs um estouro de pilha host no emissor monolítico, corrigido no
[ADR-0031](0031-blocos-aot-pilha-host.md). O trabalho final inclui essa correção,
um teste adicional de pilha pequena e a entrada explícita `--entry 0x19110`
para o callback observado em `0x209D2`.

Com essa raiz, 8769 instruções/blocos foram traduzidos, compilados e executados
nativamente até **NtCreateMutant, ordinal 192**, retorno reportado `0x18CFA`,
argumentos `0x00028EEC, 0, 0`. O fonte fixado `threads/mtx_init.c` confirma a
criação do mutex. Não chegou ao aceite do hello/framebuffer; kernel continua
com 1/69 serviços implementados. A BIOS não foi usada.

Aceite final conjunto dos ADRs 0030/0031: **262 testes do workspace/doctests** e
**111 testes do Rust gerado em debug** (incluindo 262144 comparações físicas).
A repetição em release e os demais checks estão registrados no ADR-0031.

## Referências

- [Intel SDM, volume 2A, BSF/BSR, seções 3-68/3-70](https://www.intel.com/content/dam/support/us/en/documents/processors/pentium4/sb/25366621.pdf):
  índices, ZF, destino/flags indefinidos na referência clássica. Preservar o
  destino e normalizar as demais flags é a política declarada deste runtime.
- [Análise](../../crates/xbox-recomp/src/analysis.rs) e
  [emissão](../../crates/xbox-recomp/src/emit.rs).
- [Oráculo opcional](../../crates/xbox-cpu/src/oracle/scan.rs).
- [Testes do Rust gerado](../../tools/xberecomp/tests/verification_scan.inc).
