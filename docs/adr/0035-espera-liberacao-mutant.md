# ADR-0035 — Espera e liberação de mutant, sem scheduler

Data: 2026-10-20. Estado: aceito para o perfil de uma thread guest.

## Contexto

O ADR-0032 criou mutants mas não implementou espera nem liberação. A execução real do
`hello` chegou exatamente nisso: `NtWaitForSingleObject` (233), chamado de `0x16B99` com
handle 4, `FALSE` e timeout nulo. O fonte público do nxdk mostra a origem — `mtx_lock`
compara o retorno com `STATUS_WAIT_0` —, e `mtx_unlock` usa `NtReleaseMutant` (221)
logo depois, o que apareceu como o bloqueio seguinte.

O problema de projeto é que "esperar" num emulador com **uma** thread guest e **nenhum**
scheduler não tem resposta óbvia. Bloquear a thread do host seria travar o processo;
devolver sucesso sem adquirir seria o sucesso genérico que o guia proíbe.

## Decisão

A espera só sabe operar em mutant, que é o único objeto que este kernel cria, e
**nunca bloqueia a thread do host**. Com uma thread guest, cada caso tem uma resposta
definida:

| Situação | Resposta |
|---|---|
| mutant livre | adquire, contagem 1, `STATUS_WAIT_0` |
| mutant já da thread atual | contagem sobe (é recursivo), `STATUS_WAIT_0` |
| contagem no limite | `STATUS_MUTANT_LIMIT_EXCEEDED`, sem dar wrap |
| handle fora da tabela | `STATUS_INVALID_HANDLE` |
| mutant de **outra** thread | erro de variante: exigiria scheduler |
| `Alertable` verdadeiro | erro de variante: exigiria APCs |
| `Timeout` não nulo | erro de variante: exigiria relógio de espera |

O caso "de outra thread" é inalcançável hoje: só existe a identidade inicial. Ele é
tratado como erro justamente para que, quando threads existirem, apareça como pendência
e não como comportamento silenciosamente errado.

A liberação devolve `STATUS_MUTANT_NOT_OWNED` quando quem chama não é o proprietário —
inclui o mutex livre — e zera o proprietário quando a contagem chega a zero.

`PreviousCount` não nulo é **recusado**. Ele devolve o estado de sinal anterior do
mutant, que não é a contagem de recursão; a relação entre os dois não foi verificada
contra o alvo, que passa nulo. Escrever um valor derivado de suposição na memória do
título seria resultado errado em silêncio — pior que um erro visível.

`MAX_RECURSION` é `i32::MAX`, porque o campo correspondente no kernel é `LONG`.

## Consequências

- `KMUT` foi para a **v2**: a v1 só representava os estados de criação (`0/0` e `1/1`),
  e a espera produz contagem acima de 1. A v1 continua carregando, com a validação
  restrita que ela própria garantia.
- O homebrew avançou pelo par `mtx_lock`/`mtx_unlock` e parou em
  `RtlInitializeCriticalSection` (291), vindo de um construtor global.
- `8/69` serviços. Continua não sendo percentual de compatibilidade.
- Quando houver scheduler, a identidade da thread tem de vir dele, não da constante
  `INITIAL_THREAD`, e o braço "outra thread" passa a exigir fila de espera.

## Como reverter

Os dois serviços saem da tabela de despacho e de `IMPLEMENTADOS` em um ponto cada. O
`KMUT` v2 permaneceria legível; voltar a gravar v1 exigiria recusar contagem acima de 1.
