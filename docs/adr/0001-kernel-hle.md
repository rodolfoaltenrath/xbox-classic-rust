# ADR-0001: Kernel HLE em vez de boot do BIOS

- **Status:** aceito
- **Data:** 2026-10-19
- **Fase:** transversal (define a Fase 4)

## Contexto

Um emulador de Xbox precisa que o `default.xbe` do jogo encontre um kernel
(`xboxkrnl`) do outro lado da sua *thunk table*. Há dois caminhos: bootar a ROM MCPX +
BIOS retail e deixar o kernel original rodar (LLE), ou reimplementar os ~380 ordinais
do kernel em Rust (HLE).

## Alternativas consideradas

1. **LLE (bootar o BIOS)** — compatibilidade e precisão quase de graça; exige que o
   usuário forneça um dump de BIOS, e exige emular ATA, SMC, MMU completa e a cadeia de
   boot criptografada. Custo de CPU alto em I/O, threads e heap.
2. **HLE (reimplementar o kernel)** — nenhum dump exigido; I/O, scheduler e heap rodam
   como código nativo; compatibilidade passa a depender da nossa cobertura de ordinais.

## Decisão

Kernel HLE. O emulador carrega o `.xbe` diretamente e resolve a thunk table para
trampolins nativos.

## Consequências

- Positivas: usuário não precisa de BIOS; performance muito melhor no hardware-alvo
  (objetivo O2); save state projetável desde o início; nada proprietário no projeto.
- Negativas / dívida aceita: cobertura de ordinais é trabalho incremental e sem fim
  natural; edge cases de semântica do NT enxuto são nossos para acertar.
- Mitigação obrigatória: telemetria de ordinal não implementado + `ordinal-report`,
  para que "o jogo X não roda" vire uma lista de funções a escrever.

## Como reverter

Caro: um caminho LLE exigiria emular o hardware de boot inteiro. Em vez de reverter,
o plano B realista é HLE com stubs mais fiéis em casos específicos.
