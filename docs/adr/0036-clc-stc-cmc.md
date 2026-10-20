# ADR-0036 — CLC, STC e CMC

Data: 2026-10-20. Estado: aceito.

## Contexto

Com o construtor global incluído entre as raízes, a tradução encontrou `CLC` em
`0x14598`. É a primeira instrução de manipulação direta de flag sem operando fora de
`CLD`/`STD`, que já existiam como `Operation::Direction`.

## Decisão

Modelar as três como `Operation::Carry(CarryOp)`. Só `CLC` apareceu no fluxo descoberto;
`STC` e `CMC` entram junto porque são a mesma família de decodificação, com semântica
mecânica e sem ambiguidade — o mesmo critério usado para fazer as 16 condições de
`SETcc` de uma vez (ADR-0024).

A emissão manipula o bit 0 de EFLAGS direto, sem passar pelas funções de flag
aritmética, porque nenhuma outra flag é afetada.

**Detalhe que importa mais que a instrução:** as três entraram no mesmo braço de
`analysis::targets` que `Nop` e `Direction`, o que impede que elas invalidem o
rastreamento de registradores. Sem isso, um `CLC` entre o carregamento de um ponteiro e
uma transferência indireta apagaria o valor rastreado e **perderia um destino
descoberto** — e descoberta de destino é justamente o que estava bloqueando a execução
quando esta instrução apareceu.

## Consequências

- O teste nativo compara o valor **inteiro** de EFLAGS a cada passo, não só CF: isso
  prova que nenhuma outra flag é tocada, e não apenas que CF ficou certo.
- Depois de `CLC`, a tradução avançou e o bloqueio voltou a ser de serviço de kernel.

## Como reverter

Remover o braço do decoder, da emissão e de `targets`. Nenhum outro subsistema depende
da variante.
