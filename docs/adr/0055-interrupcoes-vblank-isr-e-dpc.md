# ADR-0055 — Interrupções: vblank, ISR e DPC

Data: 2026-10-22. Estado: aceito; GPU além do vblank pendente.

## Contexto

O `triangle` do nxdk (ADR-0053, ADR-0054) parou em `HalGetInterruptVector`: o `pbkit`
instala uma interrupção da GPU (IRQ 3) com um ISR e um DPC, e `pb_wait_for_vbl` espera um
evento que o DPC pulsa. O código recompilado é uma máquina de blocos sem pilha nativa, então
a pergunta não é "como o host chama uma rotina", e sim "como o guest é desviado para ela e
volta".

## Decisão

### O contrato com o guest

`KINTERRUPT` e `KDPC` vivem na memória do guest, nos layouts de `xboxkrnl.h` do nxdk. O
título as aloca; o kernel as preenche e **lê de volta** na entrega. O host guarda só o que o
guest não guarda: quais interrupções estão conectadas e a fila de DPCs, listas de endereços
(snapshot `KINT`, validado contra a memória restaurada).

- `HalGetInterruptVector(nível, &Irql)`, `KeInitializeInterrupt`, `KeConnectInterrupt`,
  `KeDisconnectInterrupt`, `KeInitializeDpc`, `KeInsertQueueDpc`, `KeRemoveQueueDpc`.
- `KeInsertQueueDpc` devolve FALSE se o DPC já está na fila (e preserva os argumentos do
  primeiro); `Inserted` volta a 0 **antes** de o DPC rodar, para ele poder se reenfileirar.
- Conectar exige objeto inicializado e rotina em memória executável.
- `HalRegisterShutdownNotification`: registra e remove; nenhum caminho de desligamento chama
  as rotinas ainda. O `pbkit` desregistra sem ter registrado, o que é inofensivo.

### Entrega como chamada guest aninhada

`Session::conduzir` ganha, no topo de cada volta, `deliver_interrupts`: empilha na pilha do
guest os argumentos e um endereço de retorno sentinela, aponta o EIP para a rotina, roda o
código nativo até ela retornar ao sentinela e restaura o contexto interrompido — o mesmo
truque do retorno da thread de sistema. ISR com `(Interrupt*, ServiceContext)`, DPC com
`(Dpc*, DeferredContext, Arg1, Arg2)`, ambos `stdcall`.

**ISR e DPC são atômicos para o host.** Rodam até o fim dentro de uma única chamada; o host
nunca vê a CPU no meio de uma rotina de interrupção, então o snapshot não precisa de um
quadro interrompido. Uma rotina que não retorna em 64 fatias vira `InterruptStuck`; uma que
tenta esperar vira `BlockedInInterrupt`. O DPC roda em DISPATCH_LEVEL; DPCs que se
reenfileiram são limitados a 256 por ponto de entrega.

### O vblank

Os quatro registradores que o `pbkit` usa para o vblank têm modelo no `Bus`
(`xbox-mem::nv2a_irq`): `PMC_INTR_0` (derivado), `PMC_INTR_EN_0`, `PCRTC_INTR`
(write-1-to-clear) e `PCRTC_INTR_EN`. A pendência trava independente da habilitação — a
habilitação só decide se ela chega ao PMC —, e é por isso que o `pbkit` limpa antes de
habilitar. **Só acessos de 32 bits** são modelados; outra largura cai na telemetria como
qualquer registrador sem modelo.

Um vblank a cada `frame_cycles(59,94 Hz)` do Clock único. Vários períodos sem atendimento
viram um (a pendência trava). O snapshot da sessão (`AOTS` v3) guarda a contagem.

### A espera que bloqueia

`NtWaitForSingleObject` num evento não sinalizado, **quando há uma interrupção da GPU
conectada e o vblank chega até a linha**, deixa de ser erro: o serviço marca o bloqueio e a
sessão **não aplica o retorno** (a CPU segue na chamada pendente), deixa o relógio correr até
o próximo vblank, entrega ISR e DPC no topo da volta e repete a chamada. Sem fonte de sinal a
espera continua sendo `VarianteNaoImplementada`, como antes.

O pulso passa a ter o significado que tem com um waiter: `NtPulseEvent` sobre o evento pelo
qual a chamada bloqueada espera a **libera** sem deixar o evento sinalizado. Antes, sem
waiter, ele só limpava (ADR-0054). O waiter e a liberação entram no snapshot (`KEVT` v2).

## Provisório declarado — P10, P11, P12

| | Estado | Condição de saída |
|---|---|---|
| **P10** | `HalGetInterruptVector` devolve vetor `0x30 + nível` e `KIRQL = 27 − nível` (convenção do HAL x86 do NT, não verificada no Xbox); o ISR roda com o IRQL **inalterado**, porque o perfil só representa PASSIVE/APC/DISPATCH (ADR-0042) | um título que leia o vetor ou o IRQL dentro do ISR; ampliar `Irql` para DIRQL |
| **P11** | a entrega é só nas fronteiras de `conduzir` (≈1 fatia de latência, sem preempção no meio de um bloco); a flag IF **não é consultada** (o AOT não tem `STI` e o `main` começa com IF desligada) | modelar IF e `STI`/`CLI` como máscara real de entrega |
| **P12** | só os quatro registradores de vblank do NV2A têm modelo; o resto lê zero | modelo do NV2A (Fase 5); ver abaixo |

## Onde o `triangle` para agora

Ele **instala** o ISR e o DPC (o que antes parava em `HalGetInterruptVector`) e segue para a
inicialização da GPU. Lá, `pb_init` lê `NV_PRAMDAC_NVPLL_COEFF`, recebe zero (o placeholder
declarado), conclui "PLL da GPU inválido" e **retorna -5**; o `triangle` não confere o
retorno e, mais adiante, escreve em `[0 + 0x40]` (`pb_DmaUserAddr` nunca foi atribuído):
`falta em virt:0x00000040 (instrução em EIP=0x00016EA2)`.

Isto não é um defeito do mecanismo de interrupção: é o primeiro ponto onde o título exige
**valores de hardware** que ninguém modelou — o PLL, `NV_PFB_CFG0`, `MmClaimGpuInstanceMemory`
e, depois, PFIFO/PGRAPH. Inventar esses valores a partir da memória de outros emuladores
seria exatamente o "hardware inventado sem evidência" que a telemetria existe para evitar, e é
o começo da Fase 5, não um ajuste.

## Verificação

- 8 casos no Rust **gerado** (`support/interrupt.rs`): o ciclo completo vblank → ISR → DPC →
  pulso → espera satisfeita com código x86 escrito à mão; os argumentos recebidos pelo ISR e
  pelo DPC; o preenchimento de `KINTERRUPT`/`KDPC`; o tempo (entre 1 e 2 quadros) e a linha
  limpa; a pilha intacta depois; espera sem vblank habilitado ainda erro identificável; ISR
  que não retorna; snapshot da sessão com ida e volta idêntica. Contador gerado 198 → 206.
- 11 testes dos serviços (`tests/interrupts.rs`), 2 do `Bus` (`tests/mmio.rs`) e 5 unitários
  dos registradores.
- Quatro testes de migração de snapshot antigos (contíguo, links, vídeo e `KEVT` v10)
  precisaram acompanhar o formato novo: eles construíam um snapshot antigo cortando o atual, e
  agora também precisam rebaixar a versão do `AOTS`.
- Serviços do hello: 32 → 37 de 69.

## O que não está coberto

- Nenhum alvo real executou o ciclo: ele foi exercido por programas escritos à mão e pelo
  `pbkit` só até a instalação. A primeira entrega num título real continua sendo evidência
  que falta.
- O DPC de um `KTIMER`, `KeSetTimer` e APCs.
- Mais de uma interrupção por nível; só a primeira conectada ao IRQ 3 é entregue.
- `KfRaiseIrql`/`KfLowerIrql` e a máscara de interrupção por IRQL.

## Como reverter

Os serviços saem de `IMPLEMENTADOS` e do `match`; `Interrupts`, `Nv2aIrq` e a chamada de
`deliver_interrupts` em `conduzir` são aditivos. As seções `KINT`, `KEVT` v2, `AOTS` v3 e
`BUS0` v2 seriam lidas e descartadas se as versões voltassem.
