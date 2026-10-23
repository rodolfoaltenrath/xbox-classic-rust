# ADR-0062 — Liberação de memória física contígua

Data: 2026-10-23. Base de implementação: `f83343f`; fluxo de entrega atualizado até `87d2937`.
Estado: aceito para alocações CMEM comuns em Passive; DMA/PFIFO continuam pendentes.

## Evidência e decisão

Após a reserva de boot do ADR-0061, triangle executa ISR/DPC e chega ao polling
DMA_GET. Sem processamento DMA, pb_init/pb_kill expiram as esperas. A limpeza
parava em MmFreeContiguousMemory (171), retorno 0x17520, BaseAddress=0x80189000,
trampolim 0x7FFE0558. O recurso já existia em CMEM; faltava sua liberação real.

Fontes públicas fixadas, sem implementação copiada:

- [nxdk, declaração do kernel](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/xboxkrnl/xboxkrnl.h):
  VOID/NTAPI, um argumento BaseAddress. O catálogo existente registra stdcall/4.
- [nxdk, pb_kill](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/pbkit/pbkit.c):
  libera buffers DMA e pushbuffer após retirar a interrupção e o DPC; depois
  aguarda estados PFIFO de cache/runout vazios.
- [Cxbx, exportação MmFreeContiguousMemory](https://github.com/Cxbx-Reloaded/Cxbx-Reloaded/blob/585c49a50af1255ab155099e06f24505f9c5a800/src/core/kernel/exports/EmuKrnlMm.cpp)
  encaminha a liberação ao gerenciador de memória.
- [Cxbx, DeallocateContiguousMemory](https://github.com/Cxbx-Reloaded/Cxbx-Reloaded/blob/585c49a50af1255ab155099e06f24505f9c5a800/src/core/kernel/memory-manager/VMManager.cpp)
  devolve frames e remove PTEs/registro da região. É referência pública de
  compatibilidade, não medição do hardware.

A decisão é liberar o recurso completo registrado, incluindo frames e mapeamentos,
e permitir seu reuso pelo alocador existente. Não basta remover metadados ou
retornar sucesso. Isso corrige a limpeza, sem resolver a causa do timeout DMA.

## Contrato e limites

MmFreeContiguousMemory aceita em Passive a VA base exata de uma alocação CMEM.
NULL, endereço físico, ponteiro interior/desalinhado, endereço desconhecido e
liberação repetida produzem ArgumentoInvalido. Outros IRQLs produzem
VarianteNaoImplementada com nome/ordinal/chamador/argumentos. Essas recusas são
limites explícitos do perfil HLE, não uma afirmação sobre falhas do kernel original.

Antes de alterar recursos, valida toda a posse CMEM, boot, bitmap, tradução,
permissões e ausência de aliases externos. Prepara MMU/bitmap em cópias, remove
as páginas da janela contígua e libera todos os frames; então publica e remove
o registro CMEM. As primitivas de remoção são infalíveis após essa validação.
Recusas preservam recursos; leituras do frame ABI podem marcar A/D normalmente.

Os bytes da arena não são zerados pela liberação. A CPU perde o mapeamento;
DMA físico continua vendo a mesma arena até seu reuso. A próxima alocação zera
os frames pela política local existente, sem prometer limpeza no hardware.
Alocações vizinhas permanecem intactas. VOID usa EAX=0 pela convenção do HLE;
a sessão preserva flags, demais registradores e Clock, aplicando stdcall/4.
O runtime já valida/captura a continuação antes dos efeitos do serviço.

A página D3D de boot tem dono separado no Bus e não é uma base CMEM liberável.
O pool GPU provisionado (P13) e a alocação que contém o framebuffer configurado
recusam com VarianteNaoImplementada enquanto referenciados. Isso inclui saída
em blanking: o modo ainda depende do backing. Trocar para outro framebuffer
permite liberar o anterior. Não se limpam silenciosamente GINS/AVST nem se
inventam protocolos de teardown desses recursos. PRAMIN/P14 permanece independente.

Não muda formato de snapshot: CMEM v1 já representa a ausência de uma alocação;
RAM/MMU/frames persistem como antes. BUS0 v6/BTPG v1, KRNL v13/GINS v1, AOTC v8 e
AOTS v3 conservam versões e migrações. Restore cruza os mesmos contratos de posse.
Nenhum novo relógio, thread, unsafe, dependência ou modelo de hardware foi criado.

## Verificação

Seis testes de kernel cobrem liberação de múltiplas páginas, vizinhos, bytes
preservados/reuso zerado, ponteiros inválidos/double free, IRQL, corrupção de
bitmap/permissões/alias, boot, pool GPU, framebuffer com offset/blanking/troca
de modo e snapshot/reuso determinístico.

Três casos compilam e executam Rust emitido a partir de x86 real: stdcall/4 com
continuação e recursos, replay integral antes/depois de free, e retorno inválido
ou base interior sem liberar nem avançar a CPU pendente.

Gate Linux/Rust 1.98.1: 519 testes workspace/doctests; 224 casos gerados em debug
e release, este com warnings negados (contador esperado 208 fora de x86_64); fmt/Clippy
all-targets. Ordinal-report: hello 38/69, triangle 42/75 serviços. Isso não mede
compatibilidade geral. Scripts não mudaram; Win32 não foi revalidado.

A reprodução real e o próximo bloqueio estão registrados na passagem de bastão
e no guia do homebrew, com logs locais `target/free-triangle-probe.log`.

A sonda real liberou quatro alocações: VA=0x80189000/0x8018A000/0x8018B000,
4 KiB cada, e VA=0x8018C000/0x82000 bytes, total de 133 frames. Conferiu ausência
de todos os mapas/frames liberados e preservou framebuffer PA=0x5D000/0x12C000,
pool GPU e boot. Parou por solicitação do host logo após a limpeza, EIP=0x1506B,
174079287 retired/ciclos, 87 chamadas/zero recusadas. Snapshot integral e replay
de mais 2 M de blocos coincidiram byte a byte. DMA_GET teve as mesmas 12402025
leituras e o DPC executou 12 vezes antes da limpeza.

O próximo bloqueio está no esvaziamento do PFIFO em pb_kill/pb_fifo_handler:
CACHE1_STATUS (0xFD003214) permanece zero, sem o bit EMPTY; a primeira leitura
foi em 0x14D36. A sonda oficial `bash scripts/sonda.sh triangle 320` termina por
orçamento nesse PC: 93 chamadas/zero recusadas, 252079732 retired/ciclos e 209
endereços MMIO sem modelo. Framebuffer permanece diagnóstico, 1580 pixels/29
linhas. Log: `target/sonda/triangle-20261023-001300.log`. Não foi concluída a
limpeza inteira do pbkit nem renderizado o triângulo.

A execução inicial com 4000 fatias foi interrompida pelo host após repetir o
laço; seu log incompleto não é evidência de aceite. A sonda de recursos usa uma
parada explícita após free, e a sonda oficial tem orçamento menor para documentar
o bloqueio. O gate oficial passou com o Rust fixado 1.98.1: `GATE OK: 519 testes`,
log `target/gate/20261023-001255.log`, baseline atualizado para 519. AGENTS.md e
PLANO.md surgiram durante esta entrega; o plano permanece pausado. A tarefa
0.0 de compatibilidade com Clippy 1.99 não foi executada nem marcada concluída.
