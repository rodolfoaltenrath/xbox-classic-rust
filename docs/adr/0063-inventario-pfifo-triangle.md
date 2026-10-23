# ADR-0063: Inventário PFIFO/NV_USER/PRAMIN do triangle

- **Status:** aceito como inventário; não implementa emulação.
- **Data:** 2026-10-23
- **Fase:** 5 / PLANO 1.1
- **Base:** `44d582b`

## Contexto

Triangle inicializa PRAMIN e escreve o bootstrap DMA, mas GET permanece zero.
Após os timeouts, a limpeza libera CMEM e fica no esvaziamento do PFIFO.
Antes de implementar registradores, é preciso conhecer as escritas efetivas e
separar controles de programação de resultados produzidos pelo dispositivo.

## Fontes públicas fixadas

1. **N**: [nxdk outer.h](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/pbkit/outer.h)
   define offsets absolutos no NV2A e campos consumidos pelo SDK.
2. **P**: [nxdk pbkit.c](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/pbkit/pbkit.c)
   mostra os consumidores pb_init, pb_set_fifo_channel, pb_create_dma_ctx,
   pb_bind_channel, pb_start e pb_fifo_handler.
3. **X**: [xemu pfifo.c](https://github.com/xemu-project/xemu/blob/03b988c9d76553b33ed712ba00822ffdf49f0e7d/hw/xbox/nv2a/pfifo.c)
   e [user.c](https://github.com/xemu-project/xemu/blob/03b988c9d76553b33ed712ba00822ffdf49f0e7d/hw/xbox/nv2a/user.c)
   distinguem controles persistentes, W1C e os aliases USER do canal selecionado.
4. **D**: [xemu nv2a.c, nv_dma_load](https://github.com/xemu-project/xemu/blob/03b988c9d76553b33ed712ba00822ffdf49f0e7d/hw/xbox/nv2a/nv2a.c)
   e [nv2a_regs.h](https://github.com/xemu-project/xemu/blob/03b988c9d76553b33ed712ba00822ffdf49f0e7d/hw/xbox/nv2a/nv2a_regs.h)
   descrevem o formato de descritores DMA, instâncias de 16 bytes e campos de pacotes.

Nenhuma implementação dessas fontes foi copiada. São referências públicas de
compatibilidade, sem medição de reset físico do Xbox ou uso de BIOS proprietária.
Os valores da tabela vêm da execução local, não de defaults emprestados.

## Método e alternativas

A telemetria normal agrega acessos não modelados e guarda apenas a primeira
escrita; ela não registra o armazenamento PRAMIN já modelado. Inspecionar só esse
relatório perderia sequências e estruturas relevantes. Foram instrumentados os
acessos escalares de um artefato Rust gerado em diretório ignorado, com um wrapper
que encaminha cada operação ao mesmo Bus e registra endereço físico, largura,
valor e PC. Nenhum valor de leitura ou estado de emulação foi alterado.

O recorte começa no startup e termina logo após a primeira leitura USER DMA_GET
em pb_init. Inclui a inicialização anterior a pb_init, identificável pelo PC.
A inspeção do artefato e da desassemblagem confirma que os acessos às três faixas
nesse recorte são escalares. Acesso fora delas continua apenas no Bus normal.
Escritas de zeros em blocos PRAMIN são agrupadas por faixa na tabela; os logs
preservam cada endereço e valor. A execução oficial sem instrumentação fornece
uma referência independente de chamadas, framebuffer e MMIO sem modelo.

## Decisão para a tarefa 1.2

Modelar primeiro o armazenamento dos controles PFIFO efetivamente tocados e os
aliases USER PUT/GET do canal DMA selecionado. Programar GET por escrita CPU é
uma operação distinta de avançá-lo por consumo DMA: a tarefa 1.2 não processa
comandos e não deve inferir GET=PUT. INTR_0 será W1C, sem criar pendências de GPU
não implementadas. Perfis antigos conservam leitura zero/descarte com telemetria.

CACHE1_STATUS, RUNOUT_STATUS e DEBUG_0 ficam sem modelo nessa etapa: EMPTY não
será atribuído para satisfazer os loops. PGRAPH, PTIMER e outros blocos continuam
com seus recortes atuais. PRAMIN continua o armazenamento independente P14;
RAMFC e RAMHT não ganham cópias privadas. Cada alias USER deve compartilhar o
mesmo estado do registrador PFIFO correspondente, validando canal/modo/largura.

O inventário detalhado abaixo determina os offsets escolhidos; controle guardado
não significa temporizador, arbitragem, cache ou mecanismo DMA implementado.
O futuro consumidor deverá interpretar o descritor em PRAMIN e a RAM real antes
de atualizar GET, preservando erros e comandos não atendidos de forma explícita.

## Consequências

- O próximo recorte terá limites verificáveis por snapshot e sonda, com perfil
  provisório explícito e migração de BUS0.
- Readback pode mudar caminhos antes do polling, especialmente a preservação de
  canal em RAMFC. A próxima sonda precisa identificar esses efeitos.
- O diagnóstico atual não é um teste de renderização. Somente consumir os pacotes
  e aplicar os métodos NV097 permitirá chegar ao aceite de imagem do plano.

## Como reverter

Esta entrega altera apenas documentação e o estado da tarefa no plano. Não há
estado de dispositivo novo para migrar; os artefatos de investigação são locais.

## Inventário observado até o primeiro GET

Log completo: `target/sonda/triangle-pfifo-inventory-20261023.log`.

| Físico / registrador | Operações | Valores escritos distintos, em ordem da primeira ocorrência | Fonte / consumidor | 1.2 |
|---|---|---|---|---|
| 0xFD002040 DELAY_0 | W32 | 0x000000FF | N/P; pb_init/timing | controle persistente |
| 0xFD002044 DMA_TIMESLICE | W32 | 0x0101FFFF | N/P; pb_init/timing | controle persistente |
| 0xFD00204C TIMESLICE | W32 | 0x001FFFFF | N/P; pb_set_fifo_channel | controle persistente |
| 0xFD002100 INTR_0 | W32 | 0xFFFFFFFF | N/P; pb_init/ack | controle persistente |
| 0xFD002140 INTR_EN_0 | W32 | 0x01111111 | N/P; pb_init/IRQ | controle persistente |
| 0xFD002210 RAMHT | W32 | 0x03000100 | N/P; pb_init/RAMHT | controle persistente |
| 0xFD002214 RAMFC | W32 | 0x00890110 | N/P; pb_init/RAMFC | controle persistente |
| 0xFD002410 RUNOUT_PUT | W32 | 0x00000000 | N/P; pb_init + backup/restore de canal | controle persistente |
| 0xFD002420 RUNOUT_GET | W32 | 0x00000000 | N/P; pb_init + backup/restore de canal | controle persistente |
| 0xFD002500 CACHES | W32/R32 | 0x00000000, 0x00000001 | N/P; pb_init + backup/restore de canal | controle persistente |
| 0xFD002504 MODE | W32 | 0x00000000, 0x00000001 | N/P; pb_init + backup/restore de canal | controle persistente |
| 0xFD002508 DMA | W32 | 0x00000000 | N/P; pb_init + backup/restore de canal | controle persistente |
| 0xFD00250C SIZE | W32 | 0x00000000 | N/P; pb_init + backup/restore de canal | controle persistente |
| 0xFD003000 CACHE0_PUSH0 | W32 | 0x00000000 | N/P; pb_init + backup/restore de canal | controle persistente |
| 0xFD003050 CACHE0_PULL0 | W32 | 0x00000000 | N/P; pb_init + backup/restore de canal | controle persistente |
| 0xFD003058 CACHE0_HASH | W32 | 0x00000000 | N/P; pb_init + backup/restore de canal | controle persistente |
| 0xFD003200 CACHE1_PUSH0 | W32/R32 | 0x00000000, 0x00000001 | N/P; pb_init + backup/restore de canal | controle persistente |
| 0xFD003204 CACHE1_PUSH1 | R32/W32 | 0x00000001, 0x00000000, 0x00000100 | N/P; pb_init + backup/restore de canal | controle persistente |
| 0xFD003210 CACHE1_PUT | W32 | 0x00000000 | N/P; pb_init + backup/restore de canal | controle persistente |
| 0xFD003220 CACHE1_DMA_PUSH | W32 | 0x00000000, 0x00000001 | N/P; pb_init + backup/restore de canal | controle persistente |
| 0xFD003224 CACHE1_DMA_FETCH | W32/R32 | 0x000F0078, 0x00000000, 0x00086078 | N/P; pb_init + backup/restore de canal | controle persistente |
| 0xFD003228 CACHE1_DMA_STATE | W32/R32 | 0x00000000 | N/P; pb_init + backup/restore de canal | controle persistente |
| 0xFD00322C CACHE1_DMA_INSTANCE | R32/W32 | 0x00000000, 0x0000111C | N/P; pb_set_fifo_channel/pb_init | controle persistente |
| 0xFD003230 CACHE1_DMA_CTL | W32 | 0x00000000 | N/P; pb_init + backup/restore de canal | controle persistente |
| 0xFD003240 CACHE1_DMA_PUT | W32/R32 | 0x00000000 | N/P; pb_init + backup/restore de canal | controle persistente |
| 0xFD003244 CACHE1_DMA_GET | W32/R32 | 0x00000000 | N/P; pb_init + backup/restore de canal | controle persistente |
| 0xFD003248 CACHE1_REF | R32/W32 | 0x00000000 | N/P; pb_set_fifo_channel | controle persistente |
| 0xFD00324C CACHE1_DMA_SUBROUTINE | R32/W32 | 0x00000000 | N/P; pb_set_fifo_channel | controle persistente |
| 0xFD003250 CACHE1_PULL0 | W32/R32 | 0x00000000, 0x00000001 | N/P; pb_init + backup/restore de canal | controle persistente |
| 0xFD003254 CACHE1_PULL1 | R32/W32 | 0x00000000 | N/P; pb_set_fifo_channel | controle persistente |
| 0xFD003258 CACHE1_HASH | W32 | 0x00000000 | N/P; pb_init + backup/restore de canal | controle persistente |
| 0xFD003260 CACHE1_ACQUIRE_0 | R32/W32 | 0x00000000 | N/P; pb_set_fifo_channel | controle persistente |
| 0xFD003264 CACHE1_ACQUIRE_1 | R32/W32 | 0x00000000 | N/P; pb_set_fifo_channel | controle persistente |
| 0xFD003268 CACHE1_ACQUIRE_2 | R32/W32 | 0x00000000 | N/P; pb_set_fifo_channel | controle persistente |
| 0xFD00326C CACHE1_SEMAPHORE | R32/W32 | 0x00000000 | N/P; pb_set_fifo_channel | controle persistente |
| 0xFD003270 CACHE1_GET | W32 | 0x00000000 | N/P; pb_init + backup/restore de canal | controle persistente |
| 0xFD003280 CACHE1_ENGINE | R32/W32 | 0x00000000 | N/P; pb_init + backup/restore de canal | controle persistente |
| 0xFD800040 USER_DMA_PUT | W32 | 0x0018C000 | N/P/X; pb_start | alias PFIFO |
| 0xFD800044 USER_DMA_GET | R32 | — (leu zero declarado) | N/P/X; pb_init/polling | alias PFIFO |

São 37 controles PFIFO e dois endereços USER. A captura terminou por solicitação
do host na primeira leitura, PC de leitura=0x18F38, próximo EIP=0x18F3B,
retired/ciclos=24826654, 54 chamadas/zero recusadas. Leitura zero é a resposta
atual declarada do runtime, não uma observação do hardware.

### PRAMIN: todas as células tocadas, agrupadas por estrutura

N/P identificam a janela e seus escritores; D documenta os campos DMA.
Todos os acessos observados são DWORD alinhados. O recorte toca exatamente os
5120 DWORDs de 0xFD710000 a 0xFD714FFC, 12183 acessos no total. O primeiro passe
zera os 20 KiB; os demais montam e consultam as estruturas abaixo. O log completo
preserva a sequência por célula, incluindo sobrescritas e consultas.

| Faixa física inclusiva | Estrutura / valor observado | Fonte / consumidor | 1.2 |
|---|---|---|---|
| 0xFD710000–0xFD710FFF | RAMHT; 15 entradas não nulas para handles 2..14,16,17; contexto com bit válido, engine e instância | N/P; pb_bind_channel | PRAMIN existente; RAMHT_BASE guardado |
| 0xFD711000–0xFD71107F | RAMFC dos canais 0/1; canal 0 +0x0C=0x111C e +0x14=0x86078; demais DWORDs finais zero | N/P; pb_set_fifo_channel/pb_init | mesma PRAMIN; RAMFC_BASE guardado |
| 0xFD711080–0xFD71109F | bloco reservado U1, zero | P; pb_init | armazenamento existente |
| 0xFD7110A0–0xFD71111F | tabela de contextos gráficos; primeira entrada=0x111D, demais zero | P; pb_init | armazenamento existente |
| 0xFD711120–0xFD7111CF | onze descritores DMA de 16 bytes, tabela abaixo | P/D; pb_create_dma_ctx | armazenamento existente, sem tradução DMA ainda |
| 0xFD7111D0–0xFD7149BF | contexto gráfico principal; zero inicial, overlays por pb_3D_init | P; pb_init/pb_3D_init | armazenamento existente; PGRAPH fora do recorte |
| 0xFD7149C0–0xFD714CEF | objeto gráfico classe 0x97; primeiro DWORD=0x97, segundo=0xA00 | P; pb_create_gr_ctx | armazenamento existente |
| 0xFD714CF0–0xFD714CFF | objeto 0x39; primeiro DWORD=0x01000039 | P; pb_create_gr_ctx | armazenamento existente |
| 0xFD714D00–0xFD714D0F | objeto 0x9F; primeiro DWORD=0x9F | P; pb_create_gr_ctx | armazenamento existente |
| 0xFD714D10–0xFD714D1F | objeto 0x62; primeiro DWORD=0x62 | P; pb_create_gr_ctx | armazenamento existente |
| 0xFD714D20–0xFD714FFF | restante zerado, sem overlays nesse recorte | P; pb_init | armazenamento existente |

| Instância (×16 + PRAMIN) | Handle | Flags | Limit | Frame0=Frame1 | Fonte / consumidor |
|---|---|---|---|---|---|
| 0x1112 | 3 | 0xB03D | 0x03FFAFFF | 3 | P/D; pb_create_dma_ctx/bind |
| 0x1113 | 5 | 0xB002 | 0x03FFAFFF | 3 | P/D; idem |
| 0x1114 | 4 | 0xB003 | 0x03FFAFFF | 3 | P/D; idem |
| 0x1115 | 9 | 0xB03D | 0x03FFAFFF | 3 | P/D; idem |
| 0x1116 | 10 | 0xB03D | 0x03FFAFFF | 3 | P/D; idem |
| 0x1117 | 11 | 0xB03D | 0x03FFAFFF | 3 | P/D; idem |
| 0x1118 | 2 | 0x2B003 | 0x1F | 0x18A003 | P/D; idem |
| 0x1119 | 7 | 0x2B03D | 0x1F | 0x18B003 | P/D; idem |
| 0x111A | 12 | 0x2B03D | 0x10000000 | 3 | P/D; bootstrap em RAM zero |
| 0x111B | 8 | 0x2B03D | 0x20 | 0x189003 | P/D; pb_DmaBuffer8 |
| 0x111C | 6 | 0xB002 | 0x03FFAFFF | 3 | P/D; instância ativa do pusher |

A instância ativa 0x111C aponta para RAM desde zero. O guest escreve 0x18C001 na
página de boot; isso é um comando jump para 0x18C000, não uma razão para atribuir
esse endereço a GET sem buscar/decodificar a palavra. O decoder da tarefa 1.3
precisa fazer a leitura física, validar o descritor e processar o comando.

### Fora do recorte escolhido para 1.2

CACHE1_STATUS (0xFD003214), RUNOUT_STATUS (0xFD002400) e DEBUG_0 (0xFD002080)
não aparecem antes do primeiro GET desta captura; DEBUG_0/STATUS aparecem no
caminho de IRQ/limpeza do log oficial. Continuam zero com telemetria até existir
estado de filas que justifique outro resultado. Endereços PFIFO que não constam
na tabela, outros canais USER, PIO e métodos de PGRAPH também não ganham um
modelo genérico por pertencerem ao mesmo bloco.

## Verificação da tarefa

Gate Rust/Cargo 1.99: `GATE OK: 519 testes`, log
`target/gate/20261023-002549.log`. A sonda oficial sem instrumentação,
`target/sonda/triangle-20261023-002421.log`, reproduz o baseline de 320 fatias:
93 chamadas/zero recusadas, EIP=0x14D36 por orçamento, 209 MMIO sem modelo e
framebuffer diagnóstico de 1580 pixels/29 linhas. PUT e GET constam como sem
modelo. Não houve mudança de código emulado, formato ou provisório nessa tarefa.
