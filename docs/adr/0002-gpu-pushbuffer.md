# ADR-0002: Emular a GPU no nível do pushbuffer, não HLE de D3D8

- **Status:** aceito
- **Data:** 2026-10-19
- **Fase:** define a Fase 5

## Contexto

No Xbox, a biblioteca D3D8 é **linkada estaticamente dentro do XBE**. O "driver
gráfico" roda dentro do jogo e escreve comandos direto no pushbuffer do NV2A, em user
mode. Então existem dois pontos onde interceptar: na API D3D8 (dentro do jogo) ou no
hardware (o command stream).

## Alternativas consideradas

1. **HLE de D3D8** — detectar as funções da biblioteca dentro do XBE por assinatura e
   substituí-las por implementação nativa. Rápido de pôr de pé; exige banco de
   assinaturas por versão de biblioteca e quebra quando um título linka uma build
   diferente.
2. **Pushbuffer/MMIO** — interpretar o command stream do NV2A. Mais trabalho inicial;
   uma interface só, definida pelo hardware, igual para todos os títulos.

## Decisão

Emular no nível do pushbuffer e dos registradores MMIO do NV2A.

## Consequências

- Positivas: nenhum patch por título; o D3D8 original do jogo roda sem modificação;
  manutenção não cresce com o número de jogos.
- Negativas / dívida aceita: é preciso acertar PFIFO, semáforos, notifiers, register
  combiners e vertex programs antes de ver o primeiro triângulo correto. A curva de
  "primeiro resultado visível" é mais longa.
- Impede: atalhos de performance que só a camada D3D8 permitiria.

## Como reverter

Não é reversível de forma barata; seria um subsistema gráfico novo. É a decisão mais
estrutural do projeto depois do ADR-0001.
