# ADR-0099: esperas com timeout e por vários objetos; prioridades bloqueadas

Data: 2026-11-04. Estado: aceito (12.4c). 12.4b registrada como BLOQUEADA.

## Contexto e evidência

O hello importa NtWaitForSingleObjectEx (234) e NtWaitForMultipleObjectsEx (235).
O nxdk fixado documenta o contrato em
[xboxkrnl.h](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/xboxkrnl/xboxkrnl.h):
`Timeout` opcional (NULL espera sem limite) e "se um timeout explícito de zero é
especificado, nenhuma espera ocorre se não puder ser satisfeita de imediato";
o retorno da espera múltipla é o índice do objeto que a satisfez, ou
`STATUS_TIMEOUT`, `STATUS_ALERTED`, `STATUS_USER_APC`. Os consumidores reais estão em
`lib/winapi/sync.c` (`WaitForSingleObjectEx` e `WaitForMultipleObjectsEx` passam
`ms * -10000`, UserMode) e em `lib/pdclib/.../cnd_wait_timedwait.c` e `sync.c`
(variável de condição: `NtWaitForMultipleObjectsEx(2, eventHandles, WaitAny, ...)`).
`ntstatus.h` define `STATUS_TIMEOUT` (0x102). `WAIT_TYPE` é `WaitAll = 0`, `WaitAny = 1`.

A implementação de referência do Cxbx-Reloaded (cópia local não versionada em
`target/hello-kernel-reference`, lida para conferir o contrato, não copiada)
encaminha `NtWaitForSingleObject` e `NtWaitForSingleObjectEx` para
`NtWaitForMultipleObjectsEx` com contagem 1. O motor daqui faz o mesmo.

## Decisão (12.4c)

Um motor só (`dispatch/waits.rs`) atende os três ordinais; uma espera simples é uma
múltipla de um objeto, tratada como "qualquer". Ele recebe o Clock da sessão.

Validação, nesta ordem: espera alertável é recusada (sem APC, como em
KeDelayExecutionThread); `WaitMode` acima de 1, contagem 0 ou acima de 64, `WaitType`
acima de 1 devolvem INVALID_PARAMETER; lista de handles ou timeout fora da RAM,
ACCESS_VIOLATION; timeout absoluto (valor positivo) é recusado; handles repetidos são
recusados; handle desconhecido devolve INVALID_HANDLE antes de consumir qualquer coisa.
Eventos, semáforos, mutants e a thread filha (sinalizada quando termina) são aguardáveis;
arquivos e links continuam fora do perfil.

Satisfação: "qualquer" escolhe o primeiro sinalizado da lista, consome só esse e devolve
`STATUS_WAIT_0 + índice`; "todos" só consome, e de uma vez, quando todos estão sinalizados
(uma tentativa que falha não toca em nenhum). Evento de sincronização volta a não
sinalizado, semáforo perde uma unidade, mutant é adquirido. Um `NtPulseEvent` durante o
bloqueio libera a espera por qualquer cujo evento foi pulsado, sem deixá-lo sinalizado;
o motor registra todos os eventos esperados (KEVT v3 guarda a lista e qual foi liberado;
v1 e v2 migram).

Timeout: ponteiro zero espera sem limite; valor zero não espera e devolve STATUS_TIMEOUT
mesmo sem scheduler; valor negativo é intervalo relativo em 100 ns, convertido com o
mesmo `Clock::hundred_ns_to_cycles` do sono. Um intervalo que estoura 64 bits de ciclos
vale sem limite. O **prazo nasce na primeira tentativa** e é guardado no registro da
espera pendente (`WAIT` v1: thread e prazo, KRNL v17), de modo que a repetição da chamada
depois de acordar não reinicia o relógio. Ao repetir: satisfeita vence o timeout; no prazo
sem objeto, STATUS_TIMEOUT e a pendência some; fechar um objeto esperado devolve
INVALID_HANDLE e limpa os registros.

Bloquear exige quem possa sinalizar: o scheduler (thread e mutant) ou o scheduler/uma
interrupção (evento e semáforo). Em "qualquer" basta um objeto com sinalizador; em "todos"
todos os não sinalizados precisam. Senão, a variante é recusada com a mesma mensagem de
antes. Espera com timeout relativo exige o scheduler de duas threads (sem ele não há quem
faça o tempo correr de forma determinística).

O scheduler ganha `KernelWaitUntil(prazo)`: acorda por sinal ou no prazo e repete a
chamada. SCHD continua v1 com uma tag nova (6) que só aparece em snapshots novos. Uma
única espera pendente por sessão continua valendo (P19), agora também para 234 e 235, e o
kernel a impõe, não só a sessão.

## Validação

Dez testes de kernel (`tests/waits.rs`): parâmetros inválidos sem efeito, variantes
recusadas, índice e consumo em "qualquer", atomicidade de "todos", poll sem scheduler,
prazo que não recomeça, sinal antes do prazo, pulso liberando um de vários eventos,
fechamento de objeto esperado e snapshot (espera pendente, KEVT v2 e KRNL v16). O teste
antigo que exigia recusa de timeout em NtWaitForSingleObject passou a exigir só a recusa
do alertável e do timeout absoluto, porque o timeout relativo deixou de ser recusado.
No Rust emitido (três casos): timeout que expira, WaitAny satisfeito por uma filha que
dorme e sinaliza, WaitAll com timeout zero, tempo guest entre 3 e 50 ms, e replay de
snapshot a partir de **toda** fronteira de fatia (inclusive com o pai bloqueado em
KernelWaitUntil), chegando ao mesmo estado final; mais corrupção do registro `WAIT`.
Fixtures legadas que cortam o snapshot na altura de SEMA passaram a descartar WAIT.
Hello 53/69, triangle 57/75 (ordinal-report).

## 12.4b: KeSetBasePriorityThread (143) — BLOQUEADA

Não há contrato verificável para o efeito e o retorno:

- O nxdk fixado só declara `LONG KeSetBasePriorityThread(PKTHREAD, LONG Increment)`. O
  único consumidor, `SetThreadPriority` (`lib/winapi/thread.c`), descarta o retorno e
  obtém o `PKTHREAD` por `ObReferenceObjectByHandle` (ordinal 246), que pertence ao
  12.3b, também bloqueado.
- A implementação de referência do Cxbx-Reloaded fixado é um stub: grava
  `Thread->Priority = Priority` e devolve o valor **novo**, sem aritmética de incremento
  nem política de escalonamento. Não serve de evidência do comportamento do console.
- A função não tem página na documentação do WDK (o endereço da API devolve 404), e não
  há medição de prioridade padrão, faixa, prioridade-base do processo ou regras de
  preempção do kernel do Xbox.
- A única `KTHREAD` modelada é a da filha (via `KeGetCurrentThread`); a da thread
  inicial não existe (ADR-0070), então o pai não poderia nem ser alvo.

Implementar exigiria inventar o valor devolvido, a aritmética e o critério de preempção.
Fica registrado, sem código: o scheduler continua com prioridades fixas e iguais. Para
retomar é preciso uma referência externa identificada (comportamento medido ou fonte do
kernel do Xbox) e um consumidor que dependa do efeito.

## Limites e P27

Uma espera pendente por sessão; sem esperas alertáveis; sem timeout absoluto; sem
objetos de arquivo; uma ordem fixa de preferência (índice) em "qualquer". Condição de
saída: consumidor ou referência para timeout absoluto e alertável, e filas por thread
para mais de uma espera pendente.

## Consequências e reversão

Reverter exige manter o leitor de KRNL v17/WAIT e KEVT v3, e a tag 6 do scheduler;
não reinterpretar snapshots que já têm espera pendente.
