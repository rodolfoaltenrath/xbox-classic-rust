# ADR-0067 — PGRAPH serial: trap de software, clear e scanout

Data: 2026-10-23. Estado: aceito no perfil público provisório P17.
Escopo: tarefa 2.2 do PLANO, Fase 5 do ROADMAP.

## Contexto e fontes

O inventário do [ADR-0066](0066-inventario-pgraph-triangle.md) mostra que o pbkit
atualiza os descritores DMA por seu ISR antes de limpar as surfaces. Aplicar o
clear imediatamente, ignorando NOP não zero, usaria bases antigas sobre o XBE.
Os pins, log inicial e CSV de métodos desse ADR continuam sendo a referência.

Fontes primárias consultadas, para fatos de formato/consumidores:

- nxdk `14d5ee97e73347c973f1f57b68b79ec08c9e77f2`,
  [pbkit.c](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/pbkit/pbkit.c):
  pb_gr_handler/pb_subprog, set_draw_buffer, pb_fill, pb_finished e flip no vblank;
  [nv_regs.h](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/pbkit/nv_regs.h):
  campos de surface, flags de clear e máximos inclusivos do retângulo.
- xemu `03b988c9d76553b33ed712ba00822ffdf49f0e7d`,
  [pgraph.c](https://github.com/xemu-project/xemu/blob/03b988c9d76553b33ed712ba00822ffdf49f0e7d/hw/xbox/nv2a/pgraph/pgraph.c):
  NOP não zero captura método/subcanal/parâmetro, trava ERROR e espera ACK;
  [nv2a_regs.h](https://github.com/xemu-project/xemu/blob/03b988c9d76553b33ed712ba00822ffdf49f0e7d/hw/xbox/nv2a/nv2a_regs.h):
  ERROR bit 20, NOTIFICATION bit 0, PMC PGRAPH bit 12 e aliases PARAMETER_A/B.

A referência xemu declara dúvidas sobre a ativação física do trap e o NSOURCE
exato. Este é um **perfil público de compatibilidade**, não medição de hardware
nem prova de bit-exatidão. Não se copia a implementação da referência.

## Decisão

`xbox-gpu::pgraph` contém o estado e o clear próprios. Uma trait de DMA recebe
RAM física linear/PRAMIN; o Bus fornece um adaptador exclusivo verificado. Não
há dependência circular, cópia de RAM nem execução de GPU em thread host.

NOP zero é inócuo. NOP não zero de NV097 captura trap genérico, classe, subcanal
e dados, põe ERROR e suspende o consumidor. A habilitação PGRAPH propaga a
pendência ao PMC, em OR com PCRTC; INTA global controla a linha da CPU. W1C de
ERROR libera o trap e FIFO.ACCESS precisa estar ativo para continuar. O host
não reconhece os códigos PB_B2A/PB_FAB: o ISR guest lê os parâmetros, escreve
PRAMIN e reabilita FIFO por suas próprias instruções. STATUS é idle no
consumidor serial; WAIT_FOR_IDLE mantém essa ordenação, sem criar ciclos GPU.

Métodos 0x180/194/198 guardam instâncias RAMHT; 0x200..214 guardam clip,
formato, pitch e offsets; 0x1D8C/90/98/9C guardam valores/retângulo; 0x1D94
escreve RAM. Formato aceito: A8R8G8B8/Z24S8, pitch linear e uma amostra (0x128),
origem zero, dimensões até 4096 e orçamento de quatro milhões de pixels por
alvo. Retângulos são inclusivos e recortados pelo clip. Máscaras preservam os
canais de cor/depth/stencil não pedidos e o padding. Validam-se descritores,
pitch, limites DMA/RAM e contadores de **ambos** os destinos antes dos stores.
Variantes não suportadas recusam no comando, sem adiantar GET sobre o erro.

INTR/EN, NSOURCE, CTX_SWITCH1, STATUS, TRAPPED_ADDR/DATA, FIFO e PARAMETER_A/B
são DWORDs alinhados no perfil; leituras de status/trap são somente leitura.
PARAMETER_A/B compartilham o estado dos valores de clear, sem cópias divergentes.
Demais offsets continuam com zero declarado e telemetria. Outros métodos ainda
são registrados com efeito gráfico não modelado; a saída distingue o recorte.

PCRTC_START escolhe o endereço físico do scanout. AvSetDisplayMode inicializa
o endereço no perfil novo; a captura usa o scanout e a geometria HLE validada,
sem trocar silenciosamente o modo original. A visão e a restauração conferem
que todo o quadro cabe numa alocação CMEM possuída. MmFreeContiguousMemory
preserva tanto o backing original do modo quanto o scanout ativo, inclusive
com offset dentro de uma alocação e saída em blanking.

## Persistência e migração

BUS0 passa de v8 para **v9**, acrescentando **PGRF v1** após PFFO. PGRF guarda
perfil, trap/IRQ/FIFO, scanout, surface e estatísticas. Payload limitado e
opcionais canônicos, instâncias alinhadas e traps coerentes são validados antes
de publicar o objeto. BUS0 v1–v8 migra para PGRAPH LegacyUnmodeled: nenhum clear,
trap, scanout ou bloqueio novo é introduzido no replay. Regravar esse perfil
continua legado. PFIFO CommandsV1 de v8 continua consumindo só para telemetria.
As fixtures antigas removem explicitamente PGRF antes de rebaixar BUS0.

## Alternativas e limites

Descartar NOPs, interpretar códigos da biblioteca no host ou adiantar GET foram
rejeitados: mascarariam a sincronização que torna os descritores válidos.
Não há ainda rasterização, shader, texturas ou IRQ/timing gráfico completo.
P17 declara a condição de saída; P13/P14 e o escalonamento serial P16 permanecem.

## Validação

Os testes pequenos verificam cor/retângulo, máscaras, padding, depth/stencil,
preflight de dois destinos, limites/formatos, overflow de contadores, trap/W1C,
replay e corrupção. A integração PFIFO faz o trap, reescreve a base DMA no guest,
retoma e compara RAM e snapshot integral. Migração v8 mantém ausência de efeito.
O teste kernel verifica o scanout, sua posse e proteção contra liberação.

Gate: **547 testes**, target/gate/20261023-014538.log; 230 casos gerados
em debug/release (target/plan-22-native-release.log). Rust/Cargo 1.99.0.

Sonda oficial `bash scripts/sonda.sh triangle 320`,
`target/sonda/triangle-20261023-014055.log`: 282 chamadas/zero recusadas,
241241306 instruções/305861656 ciclos (417 ms), orçamento em 0x16C96;
165 MMIO sem modelo (antes 170), 1830 clears/6159326 pixels-alvo.
Framebuffer **1542 pixels/32 linhas**, diferente do diagnóstico 1580/29 e do
preto sem PGRAPH do ADR-0066. Primeira cor de clear **0x00000000**; os clears
menores desenham texto sobre esse fundo. Ainda não há raster de triângulos.

A sonda observacional `target/plan-22-real-clear-probe.log` para após o primeiro
clear de cor: descritor 0x11150 atualizado pelo ISR, base **0x33C000**, todos os
307200 DWORDs BGRA em zero; dois clears/614400 pixels-alvo (cor e zeta),
78186402 instruções/EIP=0x1648F. Snapshot integral e mais dez fatias reproduzem
o mesmo estado. Nenhuma escrita host artificial foi usada para esse avanço.
