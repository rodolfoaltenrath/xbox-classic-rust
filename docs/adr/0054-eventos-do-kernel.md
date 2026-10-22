# ADR-0054 — Eventos do kernel

Data: 2026-10-22. Estado: aceito; espera bloqueante e interrupções pendentes.

## Contexto

Com o ADR-0053 o `triangle` do nxdk passou a executar até `NtCreateEvent` (ordinal 189),
chamado de `0x180C1` durante a inicialização do pbkit. O hello importa os quatro serviços de
evento (`NtCreateEvent`, `NtSetEvent`, `NtClearEvent`, `NtPulseEvent`) mas nunca os chama.

## Decisão

Um evento é um bit de sinalização com um tipo (`NotificationEvent` ou
`SynchronizationEvent`). Os handles saem do **mesmo contador** de mutants, threads e links,
então um número nunca identifica dois objetos; o snapshot confere isso.

- `NtCreateEvent(PHANDLE, POBJECT_ATTRIBUTES, EVENT_TYPE, BOOLEAN)`. Sem `DesiredAccess`,
  como o `NtCreateMutant` do Xbox. Atributos com nome ou diretório viram variante recusada,
  não objeto anônimo. Tipo fora de 0/1 devolve `STATUS_INVALID_PARAMETER`. O store do handle
  é validado inteiro antes de qualquer efeito: uma recusa não consome handle nem publica
  objeto.
- `NtSetEvent`, `NtPulseEvent`: `PreviousState` recebe o estado de sinalização anterior (0 ou
  1). Diferente do `PreviousCount` do mutant, que o ADR-0035 recusou por depender de uma
  convenção não verificada, aqui o valor é a definição do serviço. O store de
  `PreviousState` precede a mudança de estado.
- `NtClearEvent`: limpa.
- `NtPulseEvent` sem threads esperando equivale a sinalizar e limpar: não há espera a
  liberar. **Isto muda no dia em que existir scheduler**, e está registrado como pendência.
- `NtWaitForSingleObject` em evento sinalizado devolve `STATUS_WAIT_0`; o de sincronização é
  limpo pela espera satisfeita. Em evento **não sinalizado** a espera só progrediria se outra
  thread ou uma interrupção o sinalizasse, e nenhuma existe: vira
  `VarianteNaoImplementada` ("exige scheduler ou interrupção"), sem alterar o estado.
- `NtClose` de evento remove o objeto; o handle passa a ser `STATUS_INVALID_HANDLE`.
- Limite de 4096 eventos (`STATUS_INSUFFICIENT_RESOURCES`), como os mutants.

O snapshot `KRNL` passa à v11 com uma seção `KEVT`; v1–v10 migram para a tabela vazia.

## O que não está coberto

- Espera com timeout, alertável ou em vários objetos (`NtWaitForMultipleObjectsEx`).
- Esperar um evento não sinalizado: depende de scheduler ou de interrupção (abaixo).
- `NtOpenEvent`, eventos nomeados, `DesiredAccess`.
- Nenhum caminho real exerceu `NtSetEvent`/`NtClearEvent`/`NtPulseEvent` ainda; eles foram
  implementados junto de `NtCreateEvent` porque compartilham decodificação, estado e
  semântica sem ambiguidade — a regra que o projeto já aplicou às 16 condições de `SETcc`.

## O próximo bloqueio é a interrupção

Com os eventos o `triangle` avança e passa a **programar a GPU**: a telemetria de MMIO mostra
leituras e escritas em `0xFD001804`, `0xFD009140` e `0xFD600140`. Logo depois ele chama
`HalGetInterruptVector(3, &Irql)` (de `0x1AF0C`), o início de
`HalGetInterruptVector` → `KeInitializeInterrupt` → `KeConnectInterrupt` com um DPC. Esse é o
caminho da interrupção da GPU e do vblank.

Isto deixa de ser hipotético e precisa de um ADR próprio antes de código, porque as escolhas
são estruturais: como o Rust gerado, que é uma máquina de blocos sem pilha nativa, entrega
uma rotina de serviço de interrupção guest; o que `KeRaiseIrql`/`KfLowerIrql` passam a
significar; onde fica a fila de DPCs; e quando o vblank dispara (derivado do Clock único, a
59,94 Hz). Nada disso foi desenhado aqui.

## Verificação

11 testes em `crates/xbox-kernel/tests/events.rs`: espera de notificação e de sincronização,
espera não sinalizada como erro identificável sem alterar o estado, `Set`/`Clear`/`Pulse` com
o estado anterior na memória guest, handle inválido e ponteiro ruim sem efeito, criação
inválida sem consumir handle, `NtClose`, contador compartilhado com mutants, limite de 4096,
snapshot de ida e volta e migração v10. 468 testes no workspace; clippy `-D warnings` e fmt
limpos. Serviços implementados: 28 → 32 de 69 (o 69 é o total do hello; o `triangle`
importa 79).

## Como reverter

Os quatro ordinais saem de `IMPLEMENTADOS` e do `match`; `Events` e a seção `KEVT` são
aditivos, e a v11 do `KRNL` seria lida e descartada se a versão voltasse a 10.
