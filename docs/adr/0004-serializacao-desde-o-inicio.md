# ADR-0004: Estado serializável e determinismo como requisito da Fase 0

- **Status:** aceito
- **Data:** 2026-10-19
- **Fase:** 0 (aplicado a todas as fases)

## Contexto

Save state, *quick resume* (requisito de UX do mini console) e replay determinístico de
input (ferramenta de regressão) dependem da mesma propriedade: todo o estado emulado
precisa ser capturável e restaurável, e a execução precisa ser reprodutível.

Emuladores que tentam adicionar isso depois geralmente não conseguem: o estado acaba
espalhado em ponteiros do host, handles de SO e decisões tomadas por tempo de parede.

## Alternativas consideradas

1. **Adicionar depois, quando houver jogos rodando** — entrega valor mais rápido;
   custo de refatoração provavelmente proibitivo.
2. **Requisito desde o começo** — atrasa um pouco cada subsistema; mantém a propriedade
   verdadeira continuamente, sem dívida acumulada.

## Decisão

Requisito desde a Fase 0, com quatro regras:

1. Todo estado emulado implementa `Snapshot` (`xbox-core::snapshot`).
2. Nenhuma estrutura do guest guarda ponteiro do host, só índices em arenas.
3. O grafo de objetos do kernel é arena indexada, não um grafo de `Arc`.
4. Nada no núcleo lê o relógio do host; tempo vem de `xbox-core::time`, em ciclos, e
   eventos agendados são enums serializáveis, **não closures**.

## Consequências

- Positivas: save state e replay saem como consequência; snapshot serve de verificação
  arquitetural (se não serializa, alguém guardou um ponteiro).
- Negativas / dívida aceita: escrever serialização à mão para cada subsistema; API de
  agendamento menos ergonômica que uma fila de closures.
- Testes que protegem a decisão, já no `xbox-core`: salvar duas vezes produz bytes
  idênticos, e snapshot de ida e volta é estável.

## Como reverter

Reverter é desistir de save state e replay. Relaxar pontualmente degrada a propriedade
inteira, então é tratado como erro, não como atalho.
