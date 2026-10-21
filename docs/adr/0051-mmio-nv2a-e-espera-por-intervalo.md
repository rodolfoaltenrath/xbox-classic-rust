# ADR-0051 — Janela MMIO do NV2A e espera por intervalo

Data: 2026-10-21. Estado: aceito para o perfil de uma thread ativa.

## Evidência e escopo

Base limpa: `d2e1b60`. Com o comando completo do ADR-0050 (nove raízes, 10676 blocos),
o hello parava em `PC 0x12EEB`, escrita em `0xFD6813C8`, com `Falta { NaoMapeado }`.
Depois de resolvido, parou em `KeDelayExecutionThread` (99), `Interval` em `0x4FE50`.
Foram dois bloqueios, de naturezas diferentes, e este ADR fecha os dois.

Um erro de método meu antes de chegar aqui, registrado porque custou uma volta: o
primeiro diagnóstico foi feito com seis raízes e declarou "jump table" como bloqueio,
quando o comando oficial já incluía `0x14F14` e passava pelo `switch`. A análise
automática de jump tables continua sendo uma melhoria válida, mas **não era o bloqueio**
e não foi implementada aqui.

Fontes públicas, fixadas na revisão `14d5ee97e73347c973f1f57b68b79ec08c9e77f2` do nxdk:

- `lib/hal/video.c`: `XVideoSetGammaRamp` escreve `NV_USER_DAC_WRITE_MODE_ADDRESS`
  (`0x006813C8`) com `0` e depois 768 bytes em `NV_USER_DAC_PALETTE_DATA`
  (`0x006813C9`): 256 entradas × R,G,B;
- `lib/winapi/sync.c`: `Sleep` chama `SleepEx`, que chama
  `KeDelayExecutionThread(UserMode, bAlertable, &duration)` com
  `duration = ms * -10000` (relativo, em 100 ns) e só sai do laço quando o status não é
  `STATUS_ALERTED`.

## Decisão 1 — janela MMIO como identidade sobre o físico

`0xFD000000..0xFE000000` (16 MiB) é mapeada em quatro páginas grandes, RW e nunca
executável, em `loader::mmio::install`, **incondicionalmente**: MMIO é hardware, não
import. Isso cumpre o que a revisão da Fase 2 já havia fixado
(`docs/arch/memoria.md`, janelas virtuais: "`0xFD000000+` MMIO, identidade") e que nenhum
código montava — o bus classificava o físico como dispositivo, mas a MMU respondia
`NaoMapeado`, uma falha de página que o console real nunca produziria ali.

**Só o mapeamento.** O dispositivo continua sem modelo: escritas são descartadas e
leituras devolvem zero **como placeholder declarado**, não como comportamento observado.
O que muda é a visibilidade: toda escrita passa pela telemetria do bus (endereço,
operação, tamanho, primeiro valor, PC quando conhecido).

**Janela inteira, não só as páginas do vídeo.** O `XVideoSetMode` toca quatro
sub-blocos (PMC, PCRTC, PCIO, PRAMDAC). Mapear só o que já se conhece faria cada
registrador novo parar a execução num erro que **não nomeia o registrador**, que é o
ciclo de "rodar, ler o erro, converter o endereço, repetir" que a telemetria elimina.

O `main.rs` gerado passou a imprimir o relatório de telemetria em **todo** caminho de
saída, e o executável do hello mostra, sem nenhuma interpretação:

```
fis:0xFD6813C9 escrita 1B x768  primeiro valor 0x0  de virt:0x00012F0C
fis:0xFD6813C8 escrita 1B x1    primeiro valor 0x0  de virt:0x00012EEB
```

Um registrador de índice escrito uma vez e uma porta de dados escrita 768 vezes: a rampa
completa, exatamente como o fonte do nxdk descreve.

A janela também está reservada em `validate_startup_layout`: uma seção de imagem que a
sobreponha é recusada.

## Decisão 2 — `KeDelayExecutionThread` e o avanço de tempo

O serviço **não avança relógio nenhum**. O ADR-0025 fixou um Clock único, da sessão, e
que o HLE não avança ciclos por si; também previu que o escalonador "avança até
deadlines quando todas as threads estiverem esperando". Com uma thread executável,
esperar `T` e avançar o Clock em `T` são a mesma coisa observável — nada mais corre, e
o guest só enxerga tempo pelos relógios derivados do Clock. Nenhum `sleep` do host.

O dispatcher valida e deixa um `PedidoDeTempo`; `Session::atender_kernel` o consome com
`take_time_request`, o mesmo desenho de `start_ready_thread`. Por que um canal próprio e
não um campo em `Atendimento`: o struct é construído literalmente em 59 pontos de 13
arquivos, e acrescentar um campo seria um diff enorme e arriscado para a necessidade de
um serviço.

A sessão aplica o avanço **antes** de `apply_kernel_return`, com a checagem de overflow:
se o relógio estourar, `TimeExhausted` sai com a CPU intacta na chamada pendente, como
qualquer recusa. O pedido é consumido mesmo na falha — deixá-lo pendente faria o
atendimento seguinte aplicar um avanço velho.

| Entrada | Resposta |
|---|---|
| `Interval < 0` relativo, `Alertable` falso | `STATUS_SUCCESS` + pedido de avanço |
| `Alertable` verdadeiro | recusa como variante (exigiria APCs) |
| `Interval >= 0` | recusa como variante (absoluto exige tempo de sistema; zero seria yield) |
| `WaitMode` fora de 0/1 | argumento inválido |
| `Interval` nulo, não mapeado, MMIO ou transbordo | argumento inválido |
| módulo do intervalo não cabe em ciclos `u64` | argumento inválido |

`Clock::hundred_ns_to_cycles` é nova e **verificada**: arredonda para cima (duração
pedida nunca vence cedo, a regra do ADR da Fase 0) e devolve `None` em vez de truncar. A
`scale_ceil` existente faz `as u64` no fim e truncaria em silêncio; ela recebe constantes
do próprio emulador, esta recebe o intervalo de um `Sleep` do guest, que é entrada não
confiável.

O pedido é transiente e **não entra em snapshot**: a sessão o aplica antes de qualquer
ponto de salvamento, e `load` o zera explicitamente, para que carregar um snapshot não
ressuscite um avanço já aplicado na sessão de origem. Nenhuma seção mudou de versão.

## Resultado no hello real

```
Framebuffer guest: modo 0x4010101, saída habilitada, 1208 pixels coloridos em 48 linhas
  região desenhada: x 26..=119, y 26..=88
Error: "execução interrompida após 64000000 blocos; 47 chamadas atendidas, 0 recusadas;
        instruções=17452569, relógio guest=5884119233 ciclos (8023 ms)"
```

O hello **executa seu `main`**: `XVideoSetMode`, depois o laço `debugPrint` + `Sleep(2000)`.
O relógio guest em 8023 ms é 4 × 2000 ms mais ~23 ms de custo de instruções
(17,45 M a 1 ciclo cada, a 733 MHz). O framebuffer, renderizado em ASCII a partir da
`FramebufferView`, contém "Hello nxdk!" legível, uma vez por `Sleep`. A parada final é o
**orçamento do `main` gerado** (64 M de blocos), não um erro: o alvo não retorna
espontaneamente.

O orçamento acaba depois de ~4 iterações porque o `debugPrint` do nxdk renderiza o texto
pixel a pixel (~4,4 M instruções por chamada).

## Limites que continuam valendo

- O dispositivo NV2A **não tem modelo**. Nada aqui é emulação de GPU, de CRTC ou de DAC;
  a rampa de gamma é escrita e descartada, e o hello a configura mas ninguém a aplica.
- **Vblank por interrupção não existe.** O hello não o usa neste caminho
  (`XVideoWaitForVBlank` instala o ISR só sob demanda), então não bloqueou; um título que
  o use vai exigir ISR, DPC e eventos.
- A espera avança o relógio, o que **só** é correto enquanto não houver outra thread
  executável. O pai está estacionado enquanto a filha roda (ADR-0045); com escalonamento
  de verdade, este serviço precisa passar a bloquear a thread.
- Não há apresentação: o framebuffer existe em RAM guest e é inspecionado, mas nenhuma
  janela o mostra. Isso é o item de apresentação da entrega 6.
- Variantes não modeladas permanecem recusadas: espera alertável, intervalo absoluto e
  intervalo zero.

## Validação

Testes novos: `tests/delay.rs` (11, serviço isolado), `tests/mmio.rs` (8, janela e
telemetria), `time.rs` (2, conversão e overflow) e `verification_delay.inc` (4, no Rust
**gerado** e compilado: o guest mede o próprio `Sleep` com `RDTSC`, o fatiamento da
execução não altera o tempo visto, snapshot com a chamada pendente restaura ao mesmo
estado final e `TimeExhausted` deixa a CPU intacta). **420 testes workspace/doctests** e
**181 casos gerados** em debug/release (165 nos demais hosts). Clippy `-D warnings` e fmt
limpos. O registro vai para **28/69 serviços**; isso não mede compatibilidade.

Duas correções de teste minhas, registradas por honestidade: um teste de intervalo que
cruza páginas gravava na fixture da imagem, cuja `.rdata` mapeia só uma página (movido
para a pilha, contígua); e outro comparava o snapshot inteiro, que inclui os contadores
`atendidas`/`recusadas` e **devem** mudar numa recusa — passou a comparar o estado dos
serviços separadamente dos contadores, e a afirmar que a recusa é contada.

## Como reverter

A janela MMIO é instalada em um ponto (`mmio::install`, chamado de `prepare_aot`) e
reservada em outro (`validate_startup_layout`); removê-la devolve o `NaoMapeado` no
primeiro registrador. O serviço de espera sai da tabela de despacho e de `IMPLEMENTADOS`
em um ponto cada, e o consumo do pedido é um bloco em `Session::atender_kernel`.
