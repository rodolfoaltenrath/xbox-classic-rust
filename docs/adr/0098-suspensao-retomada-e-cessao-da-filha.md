# ADR-0098: suspensão, retomada e cessão da CPU da filha guest

Data: 2026-11-04. Estado: aceito. Subentrega 12.4a do PLANO.

## Contexto e evidência

O hello importa NtSuspendThread (231), NtResumeThread (224) e NtYieldExecution
(238). O nxdk fixado
[lib/winapi/thread.c](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/winapi/thread.c)
mostra os consumidores reais: `SuspendThread` e `ResumeThread` repassam o
`PreviousSuspendCount` (ULONG) e devolvem `-1` se o status não é sucesso;
`SwitchToThread` testa `NtYieldExecution() != STATUS_NO_YIELD_PERFORMED`.
`ntstatus.h` do mesmo pin define `STATUS_NO_YIELD_PERFORMED` (0x40000024) e
`STATUS_SUSPEND_COUNT_EXCEEDED` (0xC000004A). O `xboxkrnl.h` documenta os
serviços: suspender/retomar devolvem a contagem anterior opcionalmente.

O comportamento de erro de NtSuspendThread/NtResumeThread (thread terminada,
saída opcional, retomar sem suspensão) vem da implementação de referência do
Cxbx-Reloaded usada nas fases anteriores (`EmuKrnlNt.cpp`, cópia local não
versionada em `target/hello-kernel-reference`). Foi lida para conferir o contrato,
não copiada. `STATUS_THREAD_IS_TERMINATING` (0xC000004B) e o limite de 127
suspensões (`MAXIMUM_SUSPEND_COUNT`) são os valores públicos do NT; o nxdk
fixado não define nenhum dos dois. Estão declarados no P26.

## Decisão

Só a filha tem handle. A contagem de suspensão (0..=127) é estado do objeto
thread no kernel (THRD v3, um byte depois de `handle_closed`).

- NtSuspendThread: Passive, exige o scheduler de duas threads. Handle desconhecido
  ou fechado devolve INVALID_HANDLE; saída inválida devolve ACCESS_VIOLATION antes
  de qualquer efeito; filha terminada devolve THREAD_IS_TERMINATING; contagem em 127
  devolve SUSPEND_COUNT_EXCEEDED sem escrever a saída nem alterar a contagem. No
  sucesso escreve a contagem anterior (se pedida) e soma um.
- NtResumeThread: mesmas regras de handle e saída. Subtrai um se a contagem é
  positiva; retomar thread não suspensa é sucesso com anterior zero, sem efeito.
- NtYieldExecution: devolve um EAX provisório de sucesso e **pede** a cessão. A
  sessão decide: se a outra thread está Ready e elegível, troca para ela depois de
  aplicar o retorno e o status é STATUS_SUCCESS; senão o status é
  STATUS_NO_YIELD_PERFORMED. Não há sucesso artificial.
- PsCreateSystemThreadEx aceita CreateSuspended no perfil escalonado (contagem
  inicial 1); sem scheduler continua recusada.
- Os três serviços e a criação suspensa em `LegacyBootstrap` viram
  `VarianteNaoImplementada`; dentro de ISR/DPC viram `BlockedInInterrupt`.

O scheduler trata a filha suspensa como **inelegível em qualquer estado**: ela não
recebe a CPU por quantum, cessão ou espera concluída, e não limita o orçamento do
pai. Os estados de espera (Sleeping/KernelWait) continuam evoluindo; ao retomar, a
filha reavalia a espera. A filha que se suspende a si mesma aplica o retorno da
chamada, fica Ready e cede a CPU a quem puder rodar; sem ninguém o resultado é
`WouldBlock`, o deadlock identificável do ADR-0070. Retomar uma filha nunca
iniciada a inicia imediatamente, a mesma política da criação não suspensa; retomar
uma já iniciada só a torna elegível.

## Persistência

THRD v3 acrescenta a contagem; v1 e v2 migram para zero sem herdar o estado do
destino. O load recusa contagem acima de 127, contagem sem filha e filha terminada
suspensa. A validação do scheduler passa a aceitar uma única forma de filha
existente com `child_started` falso: fase Ready com contagem positiva (criada
suspensa, nunca iniciada), e recusa filha suspensa como thread ativa. SCHD não
muda de formato: a elegibilidade deriva da contagem do kernel.

## Validação

Oito testes de kernel (`thread_control.rs`) cobrem recusa sem scheduler,
contagem e anterior, limite de 127, handle alheio/fechado e saída inválida sem
efeito, filha terminada, criação suspensa só iniciando após zerar a contagem,
pedido de cessão consumido e snapshot v3. O teste de migração THRD ganhou v2.
No Rust emitido, cinco casos: ciclo completo (cria suspensa, cede sem ninguém,
retoma, a filha roda e se suspende, cede sem poder, retoma, cede de verdade,
espera a filha terminada); replay a partir da filha nunca iniciada; replay com a
filha suspensa depois de rodar; contagem corrompida recusada; deadlock com a filha
suspensa e o pai esperando. Um teste antigo que usava CreateSuspended para provocar
uma recusa passou a usar o argumento de depurador, porque a variante deixou de ser
recusada no perfil escalonado.

## Limites e P26

Uma filha, handle único, sem pseudo-handle de thread atual, prioridades fixas
(12.4b), sem esperas com timeout ou em vários objetos (12.4c), sem APC. A
convenção NT para THREAD_IS_TERMINATING e para o limite 127 não foi medida no
Xbox. Condição de saída: consumidor ou referência externa do Xbox para esses
valores, e mais threads com filas por thread.

## Consequências e reversão

Hello 51/69 e triangle 55/75 variantes disponíveis, medidos pelo ordinal-report. As
chamadas do loop original não mudam. Reverter exige manter o leitor de THRD v3
ou migrar explicitamente; não reinterpretar snapshots que já têm contagem.
