# ADR-0071 — Publicação de KeTickCount a cada milissegundo guest

Data: 2026-10-23. Estado: aceito. PLANO 3.3; encerra P7 para sessões novas.

## Contexto e contrato

O export de dados ordinal 156 é volatile ULONG no xboxkrnl.h do nxdk fixado
em 14d5ee97e73347c973f1f57b68b79ec08c9e77f2. O ADR-0051 liga seu valor aos
milissegundos do Clock determinístico, mas publicava apenas ao terminar fatias
ou esperas. Polling guest sem chamada HLE via o valor parado durante a execução.
Não é necessário acrescentar um registrador de hardware ou timer host.

## Decisão

O Rust emitido chama State::finish_time_step depois de cada passo bem sucedido.
O custo continua AOT_STEP_CYCLES, inclusive para cada elemento REP parcial.
ClockLinkedV1 compara os milissegundos antes/depois e publica quando eles mudam.
A atualização não aposenta instrução adicional nem adiciona ciclos. As preflights
de tempo/memória e operações continuam antes do commit do passo; falhas não
cobram o custo. A função fica fora de linha para não multiplicar seu corpo pelos
milhares de blocos AOT.

O dado permanece na página RO guest instalada pelo loader. A publicação existente
traduz a VA e escreve no backing físico RAM do kernel; sem região instalada,
não cria mapeamento. ULONG trunca para 32 bits, com wrap, não saturação. Saltos
ociosos do scheduler continuam publicando imediatamente. Clock/contagem são
globais da sessão, não parte dos contextos de cada thread.

## Snapshot e compatibilidade

AOTC v9 acrescenta o discriminante TickPublication. Valores desconhecidos recusam.
v1–v8 migram para BoundariesOnly e regravar não ativa ClockLinkedV1. Assim, os
replays antigos mantêm a granularidade original. O emissor novo respeita esse
perfil; projetos AOT já gerados precisam ser traduzidos novamente para emitir a
chamada de publicação. Mexer manualmente no Clock público para diagnóstico não
é execução guest: quem o fizer publica explicitamente antes de retomar, se precisar
observar o valor imediatamente.

## Validação

Dois testes de workspace cobrem roundtrip/corrupção v9 e migração v8 sem promoção.
Quatro casos nativos executam polling sem HLE/fim de fatia, perfil legado com
publicação explícita, REP ainda não aposentado cruzando 1 ms e snapshot/replay,
e a volta de ULONG em 2^32 ms. A escrita guest no dado continua falhando por RO.
Os testes anteriores de faltas/overflow de Clock continuam verificando CPU intacta.
Gate e contagem debug/release ficam no PLANO e passagem de bastão.
