# Changelog

Formato baseado em [Keep a Changelog](https://keepachangelog.com/pt-BR/1.1.0/).
Versionamento: SemVer 0.x — a API interna muda livremente até a 1.0.

Cada entrada deve referenciar a fase do [ROADMAP.md](ROADMAP.md) e, quando houver,
o ADR correspondente em [docs/adr/](docs/adr/).

## [Não publicado]

### Corrigido

- Build/testes: treze asserções de coleções ajustadas para o lint assert_is_empty
  do Clippy 1.99, preservando as verificações e compatibilidade com 1.98.1.
  Gate Fedora/Rust 1.99: 519 testes; Clippy all-targets também passa no toolchain
  fixado. Plano retomado em modo autônomo (tarefa 0.0).

### Adicionado

- Fase 3/P3: scheduler TwoThreadV1 com Delay bloqueante, pai runnable, espera
  por filha/evento/mutant e escolha do menor deadline quando ocioso. Quantum
  sintético em Passive e contexto GPR/FS/REP/SSE/x87 por thread, Clock global.
  AOTS v4/SCHD v1 preserva LegacyBootstrap ao migrar snapshots antigos.
  NtClose da filha remove o handle sem destruir a thread/pilha ativa (THRD v2).
  Quatro testes de persistência/ciclo de vida e cinco casos nativos; limites P19 (ADR-0070).

- Fase 3-AOT/P5: descoberta de tabelas readonly com limites CMP/SUB/JA, índices
  preservados por aliases de pilha, loops de inicializadores com guarda e passo
  explícitos e candidatos de callbacks escapados. Runtime mantém cargas reais
  e UnknownTarget; não há lista de endereços do sample no analisador. Triangle
  traduz sem --entry manual: 19707 blocos, incluindo as 14 raízes anteriores.
  Dez testes de análise e três casos nativos novos (ADR-0069). Gate 566 testes/
  233 nativos debug/release; execução automática conserva 282 chamadas/zero
  recusadas e quadro idêntico de 305922 pixels, com snapshot/replay estável.

- Fase 5/raster: arrays F32 por descritores reais, vertex program interpretado
  por opcodes/mux/swizzle/máscaras e listas TRIANGLES escrevendo cor/depth na RAM.
  Culling, depth LESS/LEQUAL/ALWAYS, máscaras e clip inclusivo no perfil software;
  combiner diffuse passthrough com recusa explícita de configurações fora do
  recorte. Golden PPM 4×4 calculada analiticamente, replay de upload/BEGIN e
  migração PGRF v1 -> v2/PRND v1 sem promover raster em sessões antigas.
  Precisão e limites provisórios P18 (ADR-0068, tarefa 2.3). Gate 556 testes,
  230 nativos debug/release; sonda 305922 pixels/479 linhas, regiões R/G/B e
  dez draws/30 triângulos, 282 chamadas/zero recusadas. Quadro real inspecionado
  e snapshot/replay estável; passagem de bastão reescrita.

- Fase 5/PGRAPH: traps genéricos de NOP, IRQ compartilhada com PCRTC e pausa
  por FIFO/ACK; o ISR guest atualiza PRAMIN antes do clear. Surface BGRA8/Z24S8
  linear, retângulos inclusivos e máscaras escrevem RAM com preflight integral.
  Captura acompanha PCRTC_START; backing ativo e modo inicial são protegidos.
  BUS0 v9/PGRF v1 migra v1–v8 sem promover efeitos gráficos. Perfil público P17
  declara incerteza de NOP/NSOURCE e limites (ADR-0067, tarefa 2.2).

- Fase 5/inventário gráfico: ADR-0066 e CSV das 274 chaves NV097 recebidas pelo
  triangle, com fontes públicas, valores e efeitos planejados. 14 raízes/19391
  blocos permitem o loop: 226 chamadas/zero recusadas e 40 submissões de nove
  vértices, ainda sem pixels. Recorte inclui IRQ de software para atualizar
  descritores antes do clear, scanout e vertex program por opcodes. Gate 538
  testes, sem emulação gráfica nova nesta tarefa 2.1.

- Fase 5/snapshots: replay do PFIFO no meio de pacote/parâmetros e com call ativo,
  comparando GET final, métodos e estado integral com execução contínua. Corrupções
  PCMD/bindings/contadores recusam sem publicar PFIFO. Formatos mantidos; gate
  538 testes Linux/230 casos gerados. Tarefa 1.4, adendo ADR-0065.

- Fase 5/PFIFO: consumidor limitado de pushbuffer pela PRAMIN/RAM reais, jump
  antigo/novo, call/return e pacotes inc/non-inc, RAMHT e bindings, contagem de
  métodos sem efeito gráfico. Conduzir serve até 4096 palavras por fronteira;
  estados EMPTY derivam do consumo, sem copiar PUT para GET. BUS0 v8/PFFO v2
  preserva pacotes/bindings/telemetria, com migração v7 sem ativar DMA. Triangle
  passa o polling por GET=PUT e envia NV097; 13 raízes, próximo alvo de texto
  0x1E9B8. Sonda: 66 chamadas/zero recusadas, 257 chaves de métodos. Gate 535
  testes e 230 nativos debug/release. Perfil serial provisório P16 (ADR-0065).

- Fase 5/PFIFO: 37 controles persistentes e aliases NV_USER PUT/GET do canal DMA
  zero, com W1C de INTR_0, validação de largura/canal e preflight. BUS0 v7/PFFO v1
  preserva o estado; v1–v6 migram sem PFIFO. Não avança GET nem produz EMPTY.
  Sonda triangle: MMIO sem modelo 209 -> 170, PUT/GET saem da lista; 93 chamadas/
  zero recusadas, framebuffer diagnóstico preservado. Gate: 526 testes Linux e
  227 nativos debug/release. Perfil provisório P15 (ADR-0064, tarefa 1.2).

- Fase 5/investigação: inventário PFIFO/NV_USER/PRAMIN até o primeiro polling do
  triangle, com valores de todas as escritas escalares e fontes públicas fixadas.
  ADR-0063 delimita 37 controles persistentes e aliases USER para a tarefa 1.2;
  estados EMPTY/DEBUG e execução DMA continuam fora desse recorte. Sonda base:
  209 MMIO sem modelo, 93 chamadas/zero recusadas; gate 519 testes, sem emulação nova.

- Fase 5/kernel: MmFreeContiguousMemory (171), stdcall/4, remove mapeamentos e
  devolve os frames da alocação CMEM inteira em Passive. Valida posse/aliases,
  recusa bases inválidas e preserva boot e backings de vídeo/instância GPU.
  Snapshots mantêm formatos e reproduzem liberação/reuso. Triangle libera quatro
  recursos (133 frames) após o timeout DMA e chega ao esvaziamento PFIFO; sonda
  com 320 fatias registra 93 chamadas/zero recusadas, ainda sem triângulo.
  GATE OK: 519 testes Linux, 224 casos gerados debug/release; hello 38/69,
  triangle 42/75 serviços (ADR-0062). Gate com Rust fixado 1.98.1; compatibilidade
  Clippy 1.99 permanece em tarefa própria do PLANO.md.

- Fase 5/memória: reserva da página D3D no boot AOT antes dos frames XBE,
  PA zero/VA=0x80000000, RW sem execução e com posse distinta de CMEM. Headers
  ficam em PA=0x1000/RO; código continua sem alias RW. Validação exclui a região
  de TLS e recusa colisões/aliases. BUS0 v6/BTPG v1 preserva a reserva; v1–v5
  mantêm layout antigo sem movê-lo. Triangle passa a escrita/WBINVD e chega ao
  polling DMA_GET. Com 12 raízes ISR/DPC executam; timeout leva a pb_kill e à falta
  do ordinal 171 na limpeza; ainda sem PFIFO/renderização. 513 testes Linux e 221 nativos
  debug/release; serviços mantidos em hello 37/69 e triangle 41/75 (ADR-0061).

- Fase 5: armazenamento PRAMIN de 1 MiB com perfil independente explícito (P14),
  preservando RAMHT/contextos por VA/aliases/físico. Acessos DWORD alinhados,
  validação de larguras/bordas e preflight sem stores parciais. BUS0 v5/PRAM v1
  guarda bytes/perfil; v1–v4 migram sem modelo PRAMIN. Triangle preserva tabelas,
  mas ainda para na preparação DMA em VA=0x80000000: PA zero pertence aos headers
  XBE, exigindo reserva prévia no loader. 506 testes Linux e 218 nativos
  debug/release; serviços mantidos em hello 37/69 e triangle 41/75 (ADR-0060).

- Fase 5/kernel: consulta MmClaimGpuInstanceMemory (168), retail 64 MiB/Passive,
  com reserva de 32 frames reais, fronteira 0x83FF0000 e padding de 64 KiB.
  Repetição preserva bytes; colisões retornam NULL; resize/devkit recusam.
  Bootstrap provisório P13, KRNL v13/GINS v1 e migração v1–v12 sem estado ativo.
  Triangle passa da consulta, toca PRAMIN sem modelo e falha depois em escrita
  VA=0x80000000/EIP=0x18F19; ainda sem renderização. Gate Linux: 500 testes
  workspace/doctests, 215 nativos debug/release e sete Python separados.
  Hello permanece 37/69 serviços; triangle 41/75 (ADR-0059).

- Fase 5: PFB_CFG0/CFG1 com perfil de firmware público Cromwell, distinguindo
  arenas de 64/128 MiB e mantendo readback por VA/alias/físico. BUS0 v4/PFBM v1
  guarda configuração; snapshots v1–v3 mantêm PFB sem modelo. Acessos inválidos
  recusam antes de stores parciais. Triangle passa do guard de PFB e chega a
  MmClaimGpuInstanceMemory (168); migração v3 reproduz o bloqueio anterior.
  499 testes workspace/doctests e 212 casos gerados debug/release. Não implementa
  geometria, PRAMIN ou GPU 3D (ADR-0058); serviços do hello permanecem 37/69.

- Fase 5: recorte PRAMDAC/NVPLL com perfil PublicReferenceV1, coeficiente
  programável de 32 bits e frequência nominal derivada, sem novo relógio.
  BUS0 v3/NPLL v1 preserva perfil/coeficiente; v1/v2 migram para o comportamento
  legado sem modelo, sem alterar silenciosamente o replay. Larguras inválidas
  recusam antes de efeitos parciais. Triangle passa do PLL e rejeita PFB_CFG0;
  a falta em 0x40 ocorre na limpeza de pb_kill, antes do retorno previsto de
  pb_init. 492 testes workspace/doctests e 209 casos gerados debug/release
  (ADR-0057). Registro permanece 37/69 serviços; ainda sem GPU 3D.

- Fase 5/preparação: scripts reproduzíveis para samples nxdk e investigação de
  raízes AOT, preservando cada tentativa/log e propagando falhas. Build confere
  SDK/submódulos fixados e grava manifesto/hashes; sete testes do orquestrador.
  Inventário de evidência do PLL NV2A com revisões públicas e distinção entre
  perfil de emulador e hardware medido; nenhum valor novo adotado no runtime
  (ADR-0056).

- Fase 4/alpha: **interrupções: vblank, ISR e DPC** (ADR-0055). `HalGetInterruptVector`,
  `KeInitializeInterrupt`, `KeConnectInterrupt`, `KeDisconnectInterrupt`, `KeInitializeDpc`,
  `KeInsertQueueDpc`, `KeRemoveQueueDpc` e `HalRegisterShutdownNotification`, com
  `KINTERRUPT`/`KDPC` na memória do guest. Quatro registradores do NV2A (`PMC_INTR_0`,
  `PMC_INTR_EN_0`, `PCRTC_INTR`, `PCRTC_INTR_EN`) ganham modelo no `Bus`; um vblank a cada
  59,94 Hz do Clock único trava a pendência. `Session::conduzir` entrega o ISR e depois os
  DPCs como chamadas guest aninhadas, atômicas para o host, e uma espera por evento que só o
  DPC pulsa bloqueia, deixa o tempo correr até o vblank e repete a chamada. O pulso libera
  quem espera. Snapshots: `BUS0` v2, `KRNL` v12, `KEVT` v2, `KINT` v2, `AOTS` v3 (migrações
  testadas). 37/69 serviços do hello. Um erro de memória agora diz o EIP da instrução.
  Declarados P10–P12. O `triangle` instala o ISR e o DPC e para em `pb_init`, que lê zero do
  PLL da GPU e falha com -5: o começo da Fase 5, não uma falha da interrupção.
- Fase 4/alpha: **eventos do kernel** (`NtCreateEvent`, `NtSetEvent`, `NtClearEvent`,
  `NtPulseEvent`; ADR-0054). Os handles compartilham o contador de mutants/threads/links;
  esperar um evento sinalizado funciona (o de sincronização é limpo), esperar um não
  sinalizado é erro identificável porque não há scheduler nem interrupção para sinalizá-lo.
  `NtClose` passa a fechar eventos. Snapshot `KRNL` v11 (v1–v10 migram para tabela vazia).
  32/69 serviços do hello. O `triangle` passa a programar a GPU (MMIO do NV2A) e para em
  `HalGetInterruptVector`: o caminho da interrupção da GPU, que pede ADR próprio.
- Fase 4/alpha: **SSE escalar, x87 de carga/armazenamento, BT/BTS/BTR/BTC e WBINVD** no AOT,
  guiados pelo `triangle` do nxdk (ADR-0053). `State` ganha XMM0..7, MXCSR e a pilha x87 de
  80 bits exatos; o snapshot `AOTC` passa à v8 e v1–v7 migram para o estado de RESET. A
  conversão float→inteiro devolve o "indefinido" `0x80000000` em vez de saturar. Provisórios
  declarados no ROADMAP §16.1: P8 (MXCSR dinâmico) e P9 (aritmética x87). Com `MmAllocate-
  ContiguousMemoryEx` aceitando alinhamento 0 e `--fatias`, o `triangle` passa pela
  inicialização de vídeo e para em `NtCreateEvent`. 198 casos gerados (eram 181).
- Fase 4/alpha: `KeTickCount` (ordinal 156), primeiro bloqueio do **segundo sample** (`triangle` do nxdk, compilado no WSL com o mesmo SDK fixado). Dado exportado em página somente-leitura para o guest; o kernel o escreve pelo endereço físico e o publica a cada espera atendida e a cada fim de fatia. Imagem sem dados importados não é afetada. Provisório declarado como P7 no ROADMAP §16.1. Com isso o triangle traduz com 8 raízes e executa até a primeira instrução de ponto flutuante (`cvtsi2ss`, SSE), que o AOT ainda não implementa.
- Fase 4/alpha: o laço de execução do `main` gerado saiu do template para
  `Session::conduzir` (`xbox-recomp`), com o código nativo e a apresentação injetados como
  fechamentos. Fecha a lacuna do ADR-0052 "usuário fechando a janela durante a execução":
  6 testes cobrem retorno, parada pelo host (sem novas fatias, estado retomável),
  orçamento esgotado, #BP, HLT e propagação de erros, sem janela nem projeto gerado.
  `#BP`, `HLT` e orçamento viraram variantes de `Error` com o mesmo texto de antes. O `main`
  gerado aceita `--fatias N` (padrão 64) e recusa valor ausente ou inválido.
- Fase 4/alpha: **o hello do nxdk executa o seu `main` e escreve "Hello nxdk!" no
  framebuffer**. Duas peças fecharam os dois últimos bloqueios (ADR-0051):
  - janela MMIO do NV2A (`0xFD000000..0xFE000000`) mapeada como identidade, RW e nunca
    executável — o que a revisão da Fase 2 já havia decidido e nenhum código montava. Só o
    mapeamento: o dispositivo segue sem modelo, escritas são descartadas e leituras
    devolvem zero como placeholder declarado. O `main.rs` gerado imprime a telemetria em
    todo caminho de saída: a rampa de gamma aparece como 1 escrita de índice e 768 de dados;
  - `KeDelayExecutionThread` (99) para espera relativa não alertável. O serviço não avança
    relógio nenhum: deixa um pedido que `Session::atender_kernel` aplica ao Clock único,
    **antes** do retorno e com `TimeExhausted` deixando a CPU intacta. Nenhum `sleep` do
    host. `Clock::hundred_ns_to_cycles` é nova e verificada (arredonda para cima e devolve
    `None` em vez de truncar). Alertável, absoluto e intervalo zero recusam como variante.
- Evidência no hello real: 64 M de blocos, 17.452.569 instruções, **relógio guest 8023 ms**
  (4 × 2000 ms + ~23 ms de custo de instruções), 47 chamadas atendidas e 0 recusadas;
  framebuffer com 1208 pixels em 48 linhas, renderizado em ASCII a partir de
  `FramebufferView`. A parada é o orçamento do `main` gerado, não um serviço faltante.
  **28/69 serviços** — não é percentual de compatibilidade, e **não há apresentação em
  janela**: o framebuffer existe em RAM guest e é inspecionado.
- O template do `main.rs` gerado saiu de uma string raw dentro de `write_project` para
  `tools/xberecomp/src/template_main.rs.txt` (`include_str!`). O `main.rs` emitido é
  byte a byte idêntico ao anterior; a extração só tirou `write_project` do limite de
  linhas do clippy.
- 19 testes de kernel (11 de espera, 8 de janela MMIO), 2 de conversão de tempo e 4 no
  Rust gerado, onde o guest mede o próprio `Sleep` com `RDTSC`. **420 testes
  workspace/doctests** e **181 casos gerados** em debug/release (165 nos demais hosts).

- Fase 4/alpha: pedidos de filtros no encoder HLE, opções 11/flicker (0..7) e
  14/soften (0/1), com Result nulo e validação antes dos efeitos. Valores
  independentes preservam modo/blanking/RAM/Clock; cold boot distingue ausência
  de pedido de zero explícito. AVST v3 persiste pedidos e migra v1/v2 sem herdar
  configuração; KRNL continua v10, registro permanece 27/69 serviços. Não aplica
  filtros visuais ao framebuffer. O hello passa pelos pedidos 5/false e para
  na escrita do DAC gamma em 0xFD6813C8, PC=0x12EEB, ainda sem mapeamento/modelo.
  398 testes workspace/doctests e 177 nativos debug/release (ADR-0050).

- Fase 4/alpha: AvSetDisplayMode (3), perfil NTSC-M 640×480/60 Hz, A8R8G8B8,
  com pitch/PA validados contra o recurso físico contíguo e a EEPROM (27/69
  serviços). Visão emprestada das linhas BGRA por DMA, sem compartilhar a arena
  ou produzir pixels artificiais. AVST v2 persiste o modo, migra v1 sem modo e
  cruza EEPROM/CMEM; KRNL permanece v10. O hello configura o modo e para no filtro
  de cintilação, AvSendTVEncoderOption (2), opção 11, retorno 0x12DFA. 394 testes
  workspace/doctests e 174 nativos debug/release. Saída ainda desabilitada;
  ajustes/MMIO e apresentação da imagem pendentes (ADR-0049).

- Fase 4/alpha: MmAllocateContiguousMemoryEx (166), com frames consecutivos reais,
  limites físicos inclusivos, alinhamento e janela guest restrita a páginas RW
  possuídas (26/69 serviços). RAM virtual/física/DMA coerente; KRNL v10/CMEM v1
  guarda recursos e confere bitmap, traduções, permissões e aliases. O hello
  aloca/zera 300 páginas para 640×480×32 e chega a AvSetDisplayMode (3), retorno
  0x12CA4. 388 testes workspace/doctests e 171 nativos debug/release;
  programação do modo e apresentação pendentes (ADR-0048).

- Fase 4/alpha: encoder de vídeo com GET_SETTINGS derivado da EEPROM e perfil de
  cabo padrão, VIDEOENABLE/blanking com estado explícito e AvGetSavedDataAddress
  no cold boot sem framebuffer herdado (25/69 serviços). KRNL v9/AVST v1 guarda
  o estado e migra v1–v8. O hello pede 1.228.800 bytes para 640×480×32 e para em
  MmAllocateContiguousMemoryEx (166), retorno 0x12C01; alocação/apresentação ainda
  pendentes. 380 testes workspace/doctests e 168 nativos debug/release; perfil
  e evidência real no ADR-0047.

- Fase 4/alpha: namespace guest mínimo de D: com NtOpenSymbolicLinkObject,
  IoCreateSymbolicLink e NtClose limitado a links (23/69 serviços). Consulta de
  ausência, criação/cópia, handles compartilhados, fechamento sem desmontagem e
  erros explícitos; KRNL v8/SYML v1 com migração. A raiz adicional 0x14F14 leva
  o hello a 10676 blocos e à chamada de vídeo AvSendTVEncoderOption (2), retorno
  0x12769. 372 testes workspace/doctests e 165 casos nativos debug/release; perfil e
  evidência real no ADR-0046. Montagem de mídia na VFS/framebuffer pendentes.

- Fase 4/alpha: bootstrap cooperativo de uma filha guest, com PsCreateSystemThreadEx,
  KeGetCurrentThread e PsTerminateSystemThread (20/69 serviços). Pilha/TLS/TIB
  próprios, prefixo KTHREAD limitado, continuação do pai, Clock único e propriedade
  de mutants/seções críticas por identidade ativa. Espera em filha terminada e RET
  local; variantes não modeladas recusam. AOTS v2/KRNL v7/KMUT v3 com THRD/TCXT v1
  e migração. 365 testes workspace/doctests e 162 nativos debug/release; hello
  executa o bootstrap, oito raízes/10444 blocos, para no JMP 0x14E86 → 0x14F14.
  Escalonamento geral e framebuffer pendentes (ADR-0045).

- Fase 3-AOT/alpha: NOP multibyte r/m16/r/m32 baixa para Nop sem calcular/acessar
  seu operando; flags, orçamento, tempo e snapshot preservados. As entradas
  SystemRoutine/StartRoutine acrescentam duas raízes: 9454 blocos traduzem,
  compilam e executam até PsCreateSystemThreadEx, ainda sem implementação.
  357 testes workspace/doctests e 156 casos nativos debug/release; catálogo
  permanece em 17/69 serviços, sem thread criada por essa entrega (ADR-0044).

- Fase 4/alpha: NtAllocateVirtualMemory (184), stdcall/20, reserva+commit privado
  com base escolhida/ZeroBits=0/PAGE_READWRITE em Passive. First-fit após a imagem
  inteira, alinhamento, frames próprios zerados e saídas validadas sem efeitos
  parciais. KRNL v6/VMEM v1 salva regiões e confere Bus/MMU/frames no snapshot;
  FRM0 v1 valida bitmap/contagem transacionalmente. 354 testes workspace/doctests
  e 153 nativos debug/release. O hello usa 64 KiB em 0x30000 e chega a
  PsCreateSystemThreadEx (255), retorno 0x13BF1; 17/69 serviços (ADR-0043).

- Fase 4/alpha: KeGetCurrentIrql (103), stdcall/0, consulta o IRQL explícito da
  única CPU guest, independente de CLI/IF. Padrão Passive; Apc/Dispatch são
  contextos representáveis, sem transições ou entrega de IRQ/APC/DPC. KRNL v5/
  IRQL v1 preserva o nível e migra v1–v4. 346 testes workspace/doctests e 148
  nativos debug/release. O hello consulta Passive com IF=0 e chega a
  NtAllocateVirtualMemory (184), retorno 0x20E68, pedido de 64 KiB;
  16/69 serviços (ADR-0042).

- Fase 4/alpha: KeQuerySystemTime (128), stdcall/4 VOID, escreve o tempo de sistema
  guest em oito bytes após validação integral. Referência UTC sintética explícita
  (2000-01-01), configurável e derivada do Clock único; sem hora host ou retired.
  KRNL v4/STIM v1 preserva a referência e migra v1–v3. 342 testes workspace/doctests
  e 145 nativos debug/release; teste real confere a saída e avança até
  KeGetCurrentIrql (103), retorno 0x20E09. 15/69 serviços (ADR-0041).

- Fase 4/alpha: InterlockedExchange (54), FASTCALL/8, troca LONG em RAM guest e
  retorna os bits anteriores; valida leitura/escrita antes do efeito e preserva
  a atomicidade do Bus exclusivo. Sem estado host novo ou mudança de snapshot.
  336 testes workspace/doctests e 140 casos nativos debug/release. O XBE real
  passou pela troca e chegou a KeQuerySystemTime (128), retorno 0x20D8B;
  14/69 serviços, com variantes limitadas (ADR-0040).

- Fase 4/alpha: HalReadWritePCISpace (46) lê o PLL do controlador DRAM e suas
  subfaixas em RAM guest, com validação integral. CPU/MSRs e PCI compartilham
  configuração nominal; escritas e áreas desconhecidas recusam explicitamente.
  KRNL v3/PCIC v1 preserva o perfil e migra v1/v2. 331 testes workspace/doctests
  e 135 casos nativos debug/release. O hello calcula 733333333 Hz e avança até
  InterlockedExchange (54), retorno 0x19DC7; 13/69 serviços (ADR-0039).

- Fase 3-AOT/alpha: RDMSR com perfil guest explícito de CPU, leitura de TSC pelo
  mesmo Clock de RDTSC e configuração nominal EBL_CR_POWERON em 0x2A. Outros MSRs
  recusam com PC/índice, sem efeitos; sem instrução privilegiada host ou novo unsafe.
  AOTC v7 salva o perfil e migra v1–v6. 322 testes workspace/doctests e 131 casos
  nativos debug/release. Com quatro raízes explícitas, o hello traduz 9027 blocos
  e chega a HalReadWritePCISpace (46), leitura em offset 0x6C (ADR-0038).

- Fase 4/alpha: família de seções críticas (291/277/294/306), com estrutura Xbox
  de 28 bytes na RAM guest, aquisição recursiva, liberação e TryEnter com BOOLEAN.
  Contenção bloqueante/esperadores/exceções recusam explicitamente; validação integral
  evita stores parciais. Sem estado host novo ou mudança de snapshot. 317 testes
  workspace/doctests e 125 casos nativos debug/release; 12/69 serviços. O hello
  reconstruído no Linux mantém o hash da referência, passa por Init e encontra o
  construtor `0x205E0`; incluir essa raiz revela RDMSR em `0x207C6` (ADR-0037).
  Na retomada Linux, a arena compartilhável também passou a verificar a conversão
  `usize` → `i64` antes de criar/dimensionar o memfd, mantendo Clippy limpo no
  Rust 1.98.1 fixado; sem mudança no caminho Windows ou novo unsafe.

- Fase 4/alpha: espera e liberação de mutant (`NtWaitForSingleObject` 233 e
  `NtReleaseMutant` 221) sobre o objeto existente, **sem bloquear a thread do host**:
  com uma thread guest cada caso tem resposta definida, e `Alertable`, `Timeout`,
  "mutant de outra thread" e `PreviousCount` não nulo são recusados como variantes em
  vez de receberem valor suposto. `KMUT` foi para a v2, porque a v1 só representava os
  estados de criação e a espera produz recursão acima de 1 (ADR-0035).
- `CLC`, `STC` e `CMC` traduzidos. As três entraram no mesmo braço de
  `analysis::targets` que `Nop`/`Direction` para não invalidar o rastreamento de
  registradores — sem isso um `CLC` antes de uma transferência indireta perderia um
  destino descoberto (ADR-0036).
- Raiz explícita `--entry 0x145E0`: construtor global alcançado pelo laço que percorre
  a tabela de ponteiros de inicialização do CRT em `.rdata`. A análise não infere porque
  o ponteiro passa por slot de pilha. A tradução foi de 8769 para 8791 blocos.
- O homebrew real passa por mutex, SHA, EEPROM e construtores globais e para em
  `RtlInitializeCriticalSection` (291), com o objeto em `0x2810C`. 308 testes
  workspace/doctests e 122 casos gerados em debug/release. **8/69 serviços** — não é
  percentual de compatibilidade.

- Fase 4/alpha: consulta integral `ExQueryNonVolatileSetting` usa EEPROM explícita
  da sessão, com origem sintética/fornecida, bytes preservados e saídas validadas
  antes de escrever. KRNL v2/EEPR v1 preserva a imagem; migração v1 usa o perfil
  sintético padrão. 301 testes workspace/doctests e 121 casos gerados em debug/
  release; hello passa pela consulta e para em NtWaitForSingleObject (233),
  retorno 0x16B99, handle 4. Registro atual 6/69 serviços; índices individuais,
  escrita/persistência, espera e threads pendentes (ADR-0034).

- Fase 4/alpha: SHA-1 `XcSHAInit`/`Update`/`Final` em Rust, com contexto opaco
  de 116 bytes na RAM guest, cópia/snapshot parcial e validação de buffers sem
  stores parciais. Formato interno HLE v1, Final preserva origem por política
  local; sem novo unsafe ou dependência. 288 testes workspace/doctests e 118
  casos gerados em debug/release. O hello passa por SHAInit e para na consulta
  de EEPROM `ExQueryNonVolatileSetting` (24), retorno 0x19176. Update/Final
  foram exercidos nos testes; registro atual 5/69 serviços (ADR-0033).

- Fase 4/alpha: `NtCreateMutant` cria mutexes sem nome com handles guest,
  propriedade inicial e limite de recursos, sem sucesso genérico. `Session`
  passa a possuir o dispatcher; snapshot integral AOTS v1 inclui CPU/bus e
  objetos KRNL/KMUT. Retorno HLE validado antes dos efeitos; 271 testes de
  workspace/doctests e 115 casos gerados em debug/release. O hello real passa
  por NtCreateMutant e para em XcSHAInit (335), retorno 0x19140. Registro atual:
  2/69 serviços com variantes limitadas; espera/threads/framebuffer pendentes
  (ADR-0032).

- Fase 3-AOT: BSF/BSR de 16/32 bits, com origem registrador/memória e ZF.
  Política determinística: preservar destino em origem zero e zerar as demais
  flags aritméticas indefinidas. Aliases, FS/SIB, SP/ESP, faltas, branches e
  snapshots cobertos; 262144 comparações físicas dos resultados definidos.
  AOTC continua v6; 110 testes nativos em x86_64. A análise do fluxo descoberto
  do hello passa com 8725 instruções/blocos (ADR-0030).

- Fase 3-AOT/alpha: blocos emitidos em funções não inlineadas, dispatcher com
  chamada única e limite de 32 instruções por bloco, corrigindo o estouro de
  pilha observado no hello real. Regressão com pilha host de 128 KiB; 111 testes
  nativos. Com `--entry 0x19110`, 8769 instruções/blocos do hello compilam e
  executam até NtCreateMutant (192), ainda não implementado (ADR-0031).

- Fase 3-AOT: CMOVcc nas 16 condições, com origem registrador/memória de
  16/32 bits, leitura incondicional antes da condição e flags preservadas.
  Faltas, aliases, FS/SIB, SP/ESP, snapshots e tempo cobertos; 32768 comparações
  com CMOV físico no oráculo opcional. AOTC continua v6; 103 testes nativos
  em x86_64. Próximo bloqueio real: BSR em `0x1657C` (ADR-0029).

- Fase 3-AOT: SFENCE tem IR própria e chama `Bus::ordenar_escritas`. No perfil
  síncrono/exclusivo atual, stores já são visíveis em ordem, sem filas a drenar;
  a barreira não implementa MMIO ou conclusão de GPU. Testes cobrem aliases/DMA,
  faltas, snapshots, flags e tempo. AOTC continua v6; 96 casos nativos em x86_64.
  Próximo bloqueio real: CMOVNE em `0x13B88` (ADR-0028).

- Fase 3-AOT: INT3 (`0xCC`) gera uma fronteira #BP guest, vetor 3, com PC de
  origem, EIP seguinte e diagnóstico no main. Polls não executam a continuação
  nem cobram tempo. A descoberta termina esse caminho de exceção, mantendo
  destinos alcançados por branches/entradas explícitas. AOTC v6 guarda o
  breakpoint e migra v1–v5 sem exceção pendente. Projeto gerado com 92 casos em
  x86_64; próximo bloqueio real: SFENCE em `0x12A43` (ADR-0027).

- Fase 3-AOT: CLI limpa IF guest e HLT suspende a CPU com saída explícita ao
  host, PC/IF no diagnóstico e continuação descoberta estaticamente. Polls não
  executam código nem cobram tempo; AOTC v5 preserva a parada e migra v1–v4 com
  CPU ativa. Projeto gerado com 88 testes em x86_64. Próximo bloqueio do hello:
  INT3 em `0x20D6C`, ainda durante a tradução (ADR-0026).

- Fase 3-AOT: RDTSC lê o Clock guest em EDX:EAX, sem tempo host ou alteração
  de flags. Perfil determinístico de custo sintético por instrução/elemento REP;
  falhas e polls HLE não cobram tempo. Snapshot AOTC v4 guarda perfil/Clock e
  migra v1–v3 com tempo zero; esgotamento é erro explícito antes de efeitos.
  Projeto gerado com 84 testes em x86_64. O hello avança para CLI em `0x1409E`,
  ainda em tempo de tradução, sem execução completa (ADR-0025).

- Fase 3-AOT: CMPXCHG de 8/16/32 bits e LOCK em destino RAM, com escrita mesmo
  na comparação desigual e sem commit parcial em faltas. SETcc implementa as
  16 condições em byte baixo/alto/memória, preservando flags. Oráculo físico:
  147456 comparações; projeto gerado com 79 testes em x86_64. A tradução do nxdk
  hello avança para RDTSC em `0x2055B`, ainda sem execução completa (ADR-0024).

- Alpha, item 1 da entrega 3: região de dados exportados do kernel em memória guest,
  em duas páginas porque a permissão da MMU é por página e o título escreve um dos
  dados. `PsThreadObjectType` e `XeImageFileName` em `0x7FFF0000` somente-leitura;
  `LaunchDataPage` em `0x7FFF1000` gravável e nulo. O slot do thunk de um dado recebe o
  endereço do símbolo, não um trampolim, porque o nxdk usa `__declspec(dllimport)`.
  Procedimentos de `OBJECT_TYPE` recebem endereços **envenenados** em página não
  mapeada, para que uso indevido falhe apontando o campo em vez de saltar para zero.
  Dez testes de layout, ponteiros, permissões e conteúdo (ADR-0022).
- `translate` do `hello.xbe` real avançou do bloqueio no ordinal 164 para
  `LOCK CMPXCHG` em `0x209C3`: bloqueio de ISA **em tempo de tradução**. Resolver os
  dados destravou a próxima investigação; nenhum serviço de kernel foi implementado.
- Alpha, item 2 da entrega 3: `xbox_kernel::dispatch::Dispatcher` atende chamadas de
  kernel em `(&mut Bus, pc, regs)`; `Session::atender_kernel` aplica o resultado. Recusa
  carrega nome, ordinal, endereço do chamador e argumentos pela ABI, e **não altera a
  CPU** — o estado pendente continua retomável. Sucesso genérico é proibido por
  construção. Primeiro serviço: `RtlInitAnsiString` (289), função pura sobre a memória
  guest, com destino nulo recusado e string sem NUL tratada como erro. O projeto gerado
  ganhou laço de despacho e duas verificações no Rust emitido (69 → 71 casos);
  `1/69 serviços implementados` no relatório (ADR-0023). `1/69` não aproxima a execução
  do homebrew: o bloqueio segue sendo de ISA.

### Corrigido

- `ordinal-report` imprimia `0 serviços implementados` como texto literal, que não
  media nada. Os contadores passam a derivar do estado real, consultando a mesma fonte
  do planejamento de imports e separando **dado com representação** de **serviço
  implementado** (`3/3` e `0/69` no alvo).

- Alpha: build local do nxdk hello com revisão/submódulos fixados, manifesto e
  hashes; normalização dos timestamps e padding de metadata do cxbe. Novo
  `ordinal-report` inventaria TLS, ABI e pendências: 69 funções e 3 dados no alvo.
- Perfil inicial TLS nxdk, base FS explícita, LEA sem segmento e snapshot AOTC v3
  com migração v1/v2. Imports de funções recebem trampolins guest RX e saem por
  `Exit::KernelCall`; stdcall/cdecl/fastcall e retorno validados em Rust gerado.
  Serviços, dados exportados e threads continuam pendentes; o hello completo
  ainda não executa (ADR-0021).

- Marco 1/6 do alpha: loader compartilhado em `xbox-kernel`, com políticas de
  inspeção/AOT; `obsidian check-memory` valida XBE/pasta/ISO pelo bus sem executar
  código. T14 NFSU2 local confirma 14 seções, 152 ordinais e 4634697 bytes.
  Contratos T1–T10 passaram no Miri; 20000 cenários contra vetor independente,
  benchmark seguro, auditoria de unsafe e job Miri no CI (ADR-0020).
- Corrigidos os 19 avisos de Clippy de memória; preservado formato de snapshot.
  Arena recusa tamanho acima de isize::MAX. Posse exclusiva Send/não Sync documentada;
  proposta de aceitar data races do ADR-0011 substituída. Alvo nxdk hello fixado
  para as próximas entregas; ainda não há homebrew executando.

- Fase 3-AOT: CALL/JMP near de 32 bits por registrador/memória, incluindo SIB e
  aliases de ESP. Descoberta iterativa por constantes locais e ponteiros imutáveis;
  API `analyze_with_entries` e CLI `--entry` para callbacks/tabelas mutáveis.
  Destinos conhecidos são blocos explícitos, sem decodificação em execução.
  CALL/JMP/RET validam alvo e permissão antes de comprometer CPU/pilha; erros
  `UnknownTarget` incluem PC e destino. Orçamento, recursão e snapshots preservados
  (ADR-0019).

- Fase 3-AOT: JCXZ/JECXZ e LOOP/LOOPE/LOOPNE com CX/ECX; CLD/STD e
  MOVS/STOS/LODS/CMPS/SCAS de 8/16/32 bits, com endereços de 16/32 bits.
  REP/REPE/REPNE usam um elemento por passagem de bloco, respeitando orçamento
  e snapshot parcial. Comparações restauram flags originais em faltas; `AOTC` v2
  preserva a continuação e migra v1. 3456 comparações com strings na CPU física,
  mais testes de faults, wrap, DF, sobreposição e snapshots (ADR-0018).
- Fase 2: o bus valida a faixa inteira antes de escrever; uma página inválida,
  read-only ou região física inválida não deixa metade de um store na RAM.

**Fase 3-AOT — MUL/IMUL, DIV/IDIV e diagnóstico de #DE (ADR-0017)**
- Produtos e divisões de 8/16/32 bits com registradores implícitos; IMUL de dois e
  três operandos em 16/32 bits, com memória e imediatos com sinal.
- CBW/CWDE/CWD/CDQ. Flags indefinidas normalizadas para zero e partes superiores
  preservadas. Divisor zero e overflow do quociente retornam `#DE` recuperável com
  PC, sem comprometer CPU/FLAGS; entrega via IDT/kernel continua pendente.
- 483916 comparações físicas adicionais e 1000580 diagnósticos classificados de
  divisão. A integração executa 45 testes nativos, incluindo memória read-only,
  aliases, erros e snapshot entre produto e divisão.

**Fase 3-AOT — rotações e shifts duplos (ADR-0016)**
- ROL/ROR/RCL/RCR de 8/16/32 bits, com anel de carry de até 33 bits, contagens
  mascaradas, voltas completas e preservação de SF/ZF/AF/PF.
- SHLD/SHRD de 16/32 bits com fonte registrada e contagem imediata ou CL; contagens
  efetivas maiores que a largura retornam erro recuperável com PC antes de acesso
  à memória, pois o resultado é indefinido na ISA.
- Mais 791764 comparações físicas: 70488 rotações gerais, 524288 rotações de byte
  e 196988 shifts duplos definidos. A integração agora executa 35 testes nativos,
  incluindo aliases, memória, falhas e snapshot entre RCL/RCR/SHLD.

**Fase 3-AOT — SHL/SHR/SAR e flags indefinidas (ADR-0015)**
- Shifts de 8/16/32 bits com contagem um implícita, imediato ou CL, com contagem
  mascarada a cinco bits, sinal de SAR e preservação das flags em contagem zero.
- Flags indefinidas normalizadas para zero, com máscara de comparação independente
  no oráculo físico; memória mantém leitura/escrita mesmo com contagem efetiva zero.
- 52470 comparações físicas de shifts e 393216 casos exaustivos de byte. Testes
  adicionais de AH/CL/CX/ECX, memória, falhas e snapshot antes de ADC; a integração
  agora executa 24 testes nativos em x86_64.

**Fase 3-AOT — ADC/SBB e carry entre blocos (ADR-0014)**
- ADC/SBB de 8/16/32 bits na análise, IR e Rust gerado, com registradores parciais,
  memória e imediatos; CF de entrada participa do resultado e das flags.
- Oráculo físico ampliado: 9438 casos gerais de ALU e 262144 comparações exaustivas
  de ADC/SBB de byte; as 3872 verificações independentes de Jcc permanecem.
- Testes nativos de AH, imediatos com sinal, leitura/escrita de memória, falha de
  escrita sem comprometer FLAGS e pausa/snapshot entre palavras de uma soma/subtração.

**Repositório e publicação**
- README com estado atual, build, demonstração AOT e documentação.
- Metadados Apache-2.0 conforme a licença já escolhida no repositório do mantenedor;
  ADR-0007 atualizado. `publish = false` permanece para os pacotes Cargo.
- Exclusão explícita de `*.bin`, além de mídias, dados locais e builds já ignorados.

**Fase 3-AOT — ISA inteira ampliada e referência independente (ADR-0014)**
- Operandos de 8/16/32 bits, subregistradores incluindo AH, memória tipada,
  AND/OR, INC/DEC/NEG/NOT, MOVZX/MOVSX, LEA e as 16 condições Jcc curtas/near.
- PUSH/POP de 16/32 bits e LEAVE de 32 bits, com os casos especiais de ESP.
- Oráculo opcional em `xbox-cpu`: instruções fixas na CPU física, isolado dos
  módulos nativos. 7986 comparações ALU/flags e 3872 casos de Jcc no Rust compilado.
- Treze testes nativos na integração x86_64, mantendo as verificações de snapshot,
  memória e erros da primeira prova. Formato de snapshot preservado.
- CI de testes inclui `xbox-cpu/isa-oracle`. Validação local: 173 testes com
  warnings como erros; 13 testes do Rust gerado passaram em debug e release.

**Fase 3-AOT — primeira recompilação nativa em Rust (ADR-0013)**
- `xbox-recomp`: decoder Rust `iced-x86`, descoberta conservadora de fluxo, IR e
  emissão de blocos Rust para um subconjunto inteiro de x86-32. Instruções sem
  suporte e fluxo ambíguo são recusados com endereço.
- Runtime experimental com memória pelo `Bus`, CALL/RET na pilha do guest, orçamento
  por blocos e snapshot explícito de CPU. Não implementa TLS/kernel/GPU/áudio.
- `xberecomp demo` e `translate`: geram projeto Cargo local; `--run` compila e
  executa. Diretório não vazio é recusado.
- Fixture aberta com soma/loop/call/escrita; executável Rust resultante retorna
  EAX=62 após 37 instruções. Teste de integração compila e executa o Rust gerado,
  incluindo snapshot com call pendente, flags, endereçamento e erros de memória.
- Documentação de arquitetura e uso em `docs/arch/recompilacao.md` e
  `docs/dev/recomp.md`. Jogos comerciais ainda não executam.

**Documentação**
- `ROADMAP.md`: arquitetura completa, 12 fases, decisões fundamentais, protocolo de
  documentação.
- ADRs 0001–0008 formalizando as decisões de arquitetura (Fase 0).
- `docs/arch/core.md`, `docs/arch/formats.md`, `docs/dev/build.md`,
  `docs/dev/third-party.md`, `docs/README.md`.
- `docs/compat/formatos.md`: os 11 pontos dos formatos que ainda precisam de
  verificação contra arquivo real — a honestidade de que fixture gerada por código
  prova consistência, não conformidade.

**Fase 0 — fundação**
- Workspace Cargo com 16 pacotes: 11 crates de subsistema, o app `obsidian` e 4
  ferramentas de linha de comando. Cada crate esqueleto declara em `ROADMAP_PHASE` a
  fase que o implementa.
- Perfis de build (`release` com LTO fat e 1 codegen unit, `release-dbg` para
  perfilagem) e lints de workspace com `unsafe_code = "deny"`.
- `xbox-core`:
  - `error`: erros tipados, incluindo `NotImplemented { what, phase }`, que faz um
    caminho não escrito falhar citando a fase responsável em vez de entrar em pânico;
  - `snapshot`: formato binário com seções versionadas e trait `Snapshot` (ADR-0004),
    com testes de determinismo e de rejeição de dado truncado/corrompido;
  - `time`: relógio virtual único em ciclos do guest (733⅓ MHz) com derivação para
    contador de performance, tick de 1 ms, áudio e vblank; agendador de eventos por
    prazo, determinístico e serializável;
  - `config`: configuração TOML com defaults, `deny_unknown_fields` e validação;
  - `log`: `tracing` com filtro por subsistema via `OBSIDIAN_LOG`.
- `obsidian`: CLI com `--version`, `--help`, `ui`, `run`, `info` e `doctor`.
- CI (GitHub Actions): `fmt`, `clippy -D warnings`, testes, doctests, build release e
  compilação dos benchmarks, em Windows e Linux.
- Benchmarks com criterion em `crates/xbox-core/bench` (pacote fora do workspace).

**Fase 1 — formatos**
- `xbox-formats::bytes`: leitura limitada de inteiros e strings. Nenhum acesso direto a
  `&[u8]` no crate, para que arquivo truncado seja `Err` com posição exata, nunca
  pânico.
- `xbox-formats::xbe`: parser completo de executáveis — header, certificado de tamanho
  variável, seções (com validação antecipada de faixa), versões de biblioteca, TLS e
  logo RLE. Entry point e thunk table são desofuscados **por plausibilidade**: as três
  chaves conhecidas são testadas e vence a que aterra dentro da imagem; se nenhuma
  aterrar, o erro lista os candidatos.
- `xbox-formats::xdvdfs`: leitor do filesystem do DVD — descritor de volume, árvore
  binária de diretórios, leitura por caminho sem distinguir caixa, `walk` com ordem
  estável. Detecta a base da partição (imagem cortada vs. dump completo) e tem
  terminação garantida contra ciclos na árvore.
- `xbox-formats::fixtures`: construtores de XBE e de imagem XISO em código, incluindo
  variantes defeituosas (seção fora do arquivo, ciclo na árvore) para os testes
  pedirem exatamente o caso patológico que querem.
- `xbox-formats::identificar`: detecta o tipo de mídia pelos bytes, não pela extensão.
- `tests/robustez.rs`: ~24 mil entradas mutadas de forma determinística mais truncamento
  em todos os pontos, exigindo ausência de pânico e de laço infinito. Semente fixa, para
  que qualquer falha seja reproduzível.
- `fuzz/`: alvos `cargo-fuzz` para XBE e XDVDFS (nightly, fora do CI de todo push).
- `xbetool`: relatório completo de um `.xbe`, com extração de seção e desenho do logo em
  ASCII — que é como se verifica o decodificador de RLE a olho. Aceita `.xbe`, pasta com
  `default.xbe` ou imagem XISO (extrai o executável da imagem).
- `isotool`: `info`, `ls`, `tree` e `extract` sobre imagens XISO, com `--base` para
  forçar a base da partição.
- Exemplo `gerar_fixtures`: escreve as fixtures em arquivos para teste manual das
  ferramentas.
- `xbox-formats::media`: origem de bytes como fatia em memória **ou arquivo lido sob
  demanda**, devolvendo `Cow` para manter zero-copy onde dá. Motivo: imagem de jogo tem
  de 2 a 7 GB, e carregar o arquivo inteiro para ler um setor é inviável — tanto na
  ferramenta quanto, depois, no `NtReadFile` do kernel HLE.
- `Xdvdfs::read_range`: leitura parcial de arquivo, que é a forma como o jogo lê mídia
  durante a gameplay.
- `Xbe::va_to_offset`: traduz endereço virtual em deslocamento no arquivo, olhando a
  região de headers **e** as seções.
- `Xbe::kernel_imports` e `xbetool --thunks`: lista os ordinais do kernel que o título
  importa. É o inventário exato do que a Fase 4 precisa implementar para um jogo rodar
  (o disco verificado importa 152).
- `Header::extended`: guarda as palavras do header além de `0x178`, que existem em XBE
  de SDK novo (`size_of_image_header = 0x184` no disco verificado).
- `docs/compat/formatos.md`: resultado da verificação contra um disco de varejo real —
  7 dos 11 itens de conformidade fechados, 2 itens novos descobertos e fechados, e o
  limite deliberado de não examinar o código de biblioteca da Microsoft dentro do XBE.
- `xbox-formats::fatx`: leitor do filesystem do HD e dos memory units. Superbloco, FAT
  de 16 ou 32 bits, cadeia de clusters com terminação garantida contra ciclo, leitura
  parcial, e a tabela de partições padrão do HD do Xbox.
- `xbox-formats::eeprom`: os 256 bytes de configuração do console — serial, MAC, padrão
  de vídeo, idioma, flags de vídeo e áudio, região de DVD. `Eeprom::sintetica` gera uma
  coerente, para que ninguém precise de dump de console para jogar. O bloco cifrado é
  preservado byte a byte e não é decifrado: precisa de RC4/SHA-1/HMAC, que chegam com
  os `Xc*` da Fase 4.
- `xbox-formats::vfs`: as letras de drive que o jogo vê (`D:`, `T:`, `U:`, `Z:`, `C:`,
  `E:`, `X:`, `Y:`) sobre imagem XDVDFS, partição FATX ou pasta do host, com tradução
  dos caminhos de dispositivo (`\Device\CdRom0\...`, `\??\D:\...`) e **bloqueio de
  travessia de diretório em duas camadas**.
- `obsidian info` e `obsidian run` passaram a funcionar sobre mídia real: abrem a
  imagem, montam os drives do console, leem e parseiam o executável e contam os
  ordinais do kernel necessários. `run` ainda falha ao executar — citando as Fases 3 a
  5 — mas só depois de provar que todo o resto está no lugar.
- Alvos de fuzz para FATX e EEPROM; testes de robustez cobrindo mutação de partição
  FATX, EEPROM de conteúdo arbitrário e caminhos de VFS aleatórios.

### Alterado
- `ROADMAP.md` §4.6.1: esclarece que Vulkan e SPIR-V são a API de render desde a
  **Fase 5**, nos dois SOs — a Fase 10 troca apenas a *apresentação*
  (`winit` → `VK_KHR_display` no KMS), não a API nem os shaders. Decidido fixar o
  backend Vulkan do wgpu também no Windows, com DX12 apenas como reserva.
- `ROADMAP.md`: Fase 0 concluída; Fase 1 parcial (XBE e XDVDFS prontos). A infra de
  fuzz saiu da Fase 0 para a Fase 1, onde existem parsers para fuzzar.
- `clap` e `tracing-subscriber` passam a ser usados sem as features de cor
  (`color`/`ansi`) e os benchmarks saíram do workspace — contorno de toolchain
  documentado no **ADR-0008**, com prazo de validade antes da Fase 5.

### Corrigido
- `Clock::ms_to_cycles`/`us_to_cycles` arredondam para **cima**. Com arredondamento
  para baixo, uma espera de 16 ms vencia em 15,999 ms de tempo emulado — o guest veria
  um timer acordar antes do pedido. Tempo decorrido continua arredondando para baixo.
- `xbetool`: o rótulo de tamanho dizia "tamanho do arquivo" mesmo quando o `.xbe` tinha
  sido extraído de uma imagem, mostrando o tamanho do executável ao lado do caminho da
  imagem.
- **TLS perdido em jogos reais.** `Xbe::parse` só resolvia o diretório de TLS se ele
  estivesse na região de headers, e devolvia `None` caso contrário. Num XBE retail de
  verdade o TLS fica **dentro de `.rdata`** — ou seja, o caso que importa era
  justamente o que falhava, e sem TLS o loader da Fase 4 não inicializa as threads do
  título. Agora a tradução de endereço virtual considera headers e seções.
  Encontrado ao rodar o parser contra um disco de varejo.

### Adicionado (revisão da Fase 2)
- `docs/arch/memoria.md`: revisão arquitetural de memória, MMU, MMIO e fastmem **antes**
  da implementação, com medição no hardware real e 13 seções cobrindo taxonomia de
  endereços, mapa físico vs. janelas virtuais, API única para CPU/GPU/kernel, plano de
  testes, preparação para Vulkan e limitações conhecidas.
- ADR-0009: cinco espaços de endereçamento com newtypes e nenhuma conversão implícita.
- ADR-0010: **revisa o ADR-0005** — caminho seguro na Fase 2, fastmem na Fase 3b,
  medido contra o JIT.
- ADR-0011: arena como objeto de memória compartilhável, e a corrida CPU↔GPU como
  desvio consciente e documentado do modelo de memória do Rust.
- ADR-0012: modelo de sandbox das pastas montadas; o que está fechado e o que fica para
  a Fase 7.
- `tests/sandbox.rs`: cinco testes do sandbox da VFS, incluindo fuga por atalho de
  diretório, que é exercitável neste host mesmo sem privilégio de link simbólico.

### Corrigido (segurança)
- **Fuga da pasta montada por link simbólico.** A VFS recusava `..` e conferia o prefixo
  do caminho, mas um link dentro da pasta apontando para fora passava pelas duas
  barreiras — nenhum componente é `..` e o caminho está sob a raiz. A resolução agora
  canonicaliza caminho e raiz e exige que o resultado continue debaixo da raiz.
  Verificado com *junction* de diretório, que não exige privilégio no Windows.
  A corrida entre canonicalizar e abrir **continua aberta** e está agendada para a
  Fase 7 (ADR-0012).

### Alterado (revisão da Fase 2)
- Fase 2 do ROADMAP reescrita: fastmem fora de escopo, newtypes de endereço, arena
  compartilhável, Miri no caminho completo e um teste de integração (T14) que prova a
  leitura dos 152 ordinais do jogo real **através do bus**.
- ADR-0005 marcado como revisado pelo ADR-0010.
- ADR-0008 atualizado: declarar funções do Win32 à mão linka no toolchain atual sem
  `windows-sys`, então `xbox-mem` e `xbox-platform` não dependem dele. O prazo passa a
  valer só para a Fase 5, por causa do `wgpu`.

### Alterado (continuação)
- `Xdvdfs` passou a receber um `Media` em vez de `&[u8]`, e `read` devolve `Cow`.
  `isotool` e `xbetool` leem sob demanda: `isotool info` numa imagem de 2,87 GB passou
  a rodar em **0,4 s** sem carregar a imagem na memória.

### Performance
- Base de medição criada (criterion: agendador, conversões de relógio, snapshot).
  Ainda não há número a comparar.

### Removido
-
