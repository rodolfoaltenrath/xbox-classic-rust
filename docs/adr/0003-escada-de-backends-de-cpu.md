# ADR-0003: Três backends de CPU, entregues em escada

- **Status:** prioridade revisada pelo [ADR-0013](0013-recompilacao-estatica-rust.md); AOT vem primeiro
- **Data:** 2026-10-19
- **Fase:** define a Fase 3

## Contexto

O Xbox é x86-32 e o host é x86-64: a paridade de arquitetura abre caminhos que um
emulador de console exótico não tem. Mas cada caminho tem custo de depuração
diferente, e um emulador novo precisa primeiro ser **depurável**.

## Alternativas consideradas

1. **Só interpretador** — simples e correto, mas ~50-150 ciclos de host por instrução
   do guest; não atinge 60fps no hardware-alvo.
2. **Só virtualização (KVM/WHPX)** — custo de CPU quase zero, mas o código do guest
   executa fora do nosso controle: sem step, sem trace, sem oráculo. Depurar um jogo
   que não boota fica muito mais difícil. Exige VT-x/AMD-V.
3. **Escada: interpretador, JIT e hypervisor**, todos atrás do mesmo trait.

## Decisão

Escada. Interpretador primeiro (oráculo e depuração), JIT como caminho de produção,
hypervisor como caminho experimental com fallback automático.

## Consequências

- Positivas: sempre existe um backend em que se pode dar step; o JIT pode ser validado
  por execução diferencial contra o interpretador; no Linux, o KVM serve como **oráculo
  de teste de ISA** (executa a instrução na CPU real e compara estado).
- Negativas / dívida aceita: três implementações para manter em paridade semântica;
  o trait precisa ser desenhado para não vazar detalhe de nenhuma delas.
- O interpretador **não** é descartável: ele é infraestrutura de teste permanente.

## Como reverter

Cada backend é independente; abandonar o hypervisor é remover uma feature. Abandonar o
JIT significaria desistir do objetivo O2.
