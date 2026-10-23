# ADR-0070 — Esperas bloqueantes e scheduler determinístico

Data: 2026-10-23. Estado: aceito. PLANO 3.2, encerra P3; recorte P19.

## Contexto

O bootstrap do ADR-0045 estacionava o pai até a filha terminar. O ADR-0051
avançava Clock diretamente em KeDelayExecutionThread: correto apenas enquanto
nenhuma outra thread pode executar. A condição de saída de P3 exige que a espera
bloqueie a thread e que o scheduler decida quando avançar tempo. Os consumidores
já são públicos: PsCreateSystemThreadEx/KeDelayExecutionThread/NtWaitForSingleObject
no CRT/nxdk e o triangle fixado nos ADRs anteriores. Não se acrescenta hardware.

A primeira sonda escalonada expôs NtClose(187), chamador 0x29F35, handle 0x18,
após 26 chamadas. Fonte no mesmo pin nxdk: lib/pdclib/platform/xbox/crt0.c
WinMainCRTStartup cria e desanexa a main_thread; functions/threads/thrd_detach.c
chama CloseHandle, que chega a NtClose por lib/winapi/handleapi.c. O caminho era
invisível enquanto o pai ficava estacionado até a filha terminar.

## Decisão

Sessões novas usam TwoThreadV1: thread inicial e uma filha, preservando o limite
de criação já declarado. Cada uma pode estar Running, Ready, Sleeping(deadline),
KernelWait ou Finished. Uma única CPU/Clock/contagem de instruções pertence à
sessão. Contextos inativos guardam GPRs, flags, PC, pilha, FS/REP, XMM/MXCSR e x87;
não copiam Clock nem retired. A criação seleciona a filha; o pai fica Ready.

Delay relativo não alertável confirma o retorno, marca Sleeping e seleciona
outra Ready sem avançar tempo. Sem thread executável, o scheduler escolhe o menor
prazo de espera ou vblank capaz de sinalizar um objeto. Sem causa possível,
WouldBlock conserva a chamada pendente. NtWait em filha viva, evento ou mutant
pode bloquear; uma operação HLE concluída/ISR/DPC torna os waiters Ready para
reavaliar o objeto. Não se inventa sinalização nem sucesso de espera.

Nas fronteiras de execução, uma Ready concorrente recebe a CPU após quantum de
4096 ciclos sintéticos; o orçamento de execução também fica limitado a 4096 blocos
quando a outra thread está Ready/Sleeping. Isso não modela o quantum do Xbox.
Trocas e bloqueios exigem Passive; ISR/DPC são atômicos neste perfil e podem
acordar waiters, mas a troca fica adiada. Terminação sinaliza a filha e retoma o
pai. NtClose escalonado remove a referência do handle, sem terminar a filha ou
liberar pilha/TLS ativos; repetição e wait pelo handle fechado devolvem
STATUS_INVALID_HANDLE. O recurso permanece até teardown da sessão neste recorte,
sem reuso de handle nem criação de nova filha. RET inicial aguarda a filha restante antes de concluir a sessão.

## Persistência e compatibilidade

AOTS v4 acrescenta SCHD v1 após os campos antigos: discriminante de perfil e
payload limitado a 4096 bytes, estados/prazos, identidade ativa, último quantum
e contextos. TCTX v1 envolve TCXT v1 e os registradores SSE/x87. Validar
canonicidade, relógio, contextos, stack/TLS/PC, recursos e fases do kernel.

AOTS v1–v3 migra explicitamente para LegacyBootstrap. Regravar v4 conserva
perfil zero, sem ativar o scheduler. TCXT/CPU/Bus/kernel anteriores mantêm seus
formatos, exceto THRD v2 que acrescenta flag canônica de handle fechado. THRD v1
migra com handle aberto. Fixtures de migração comparam todo o snapshot com o perfil legado
esperado; não descartam CPU, RAM ou estado dos objetos da comparação.

## Limites / P19

Duas threads, prioridade fixa, quantum sintético, apenas esperas já modeladas,
sem APC/alertable/timeout absoluto, criação de múltiplas filhas ou contenção de
RtlCriticalSection. Variantes pendentes continuam recusadas. Reavaliar waiters
após HLE é uma política limitada, não dispatcher geral NT. Condição de saída:
casos guest reais exigindo mais threads/prioridades/waits, com extensão do perfil,
filas precisas e migração/replay dos perfis anteriores. LegacyBootstrap conserva
P3 apenas para reproduzir snapshots antigos.

## Validação

Quatro testes de workspace cobrem fechamento em Ready/Running/Terminated sem
liberar frames, invalidez persistente, THRD v1/v2 e corrupção, além de migração AOTS v3, perfil legado regravado,
roundtrip e corrupção SCHD. Cinco casos nativos executam espera com pai runnable,
dois deadlines, quantum sem kernel e snapshot/replay com GPR/SSE/x87 distintos, e detach/RET do pai com continuação da filha e replay.
A integração mantém os casos legados de espera/interrupção em ambos os perfis.
Resultados completos de gate/sonda registrados no PLANO e passagem de bastão.
