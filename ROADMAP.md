# ROADMAP — Obsidian: Xbox Clássico com Recompilação Nativa em Rust

> **Nome de trabalho:** `obsidian` (provisório — trocável com um `sed` no workspace)
> **Linguagem:** Rust (100% nativo, sem dependências de runtime externo)
> **Plataformas na v1:** Windows 10/11 (x86_64) e Linux (x86_64, desktop)
> **Plataforma-alvo final:** Linux minimalista rodando direto no KMS/DRM, sem X11/Wayland, como *appliance* (mini console)
> **Última atualização:** 2026-10-21
> **Status global:** Fases 0 e 1 concluídas; código de memória presente, com aceite da Fase 2 ainda pendente. **Fase 3-AOT iniciada:** primeira fixture traduzida para Rust, compilada e executada nativamente (ADR-0013). Jogos comerciais ainda não executam.

---

## Sumário

1. [Visão, objetivos e não-objetivos](#1-visão-objetivos-e-não-objetivos)
2. [Estratégia de plataformas (desktop agora, appliance depois)](#2-estratégia-de-plataformas)
3. [O hardware que vamos emular](#3-o-hardware-que-vamos-emular)
4. [Decisões arquiteturais fundamentais](#4-decisões-arquiteturais-fundamentais)
5. [Arquitetura de software](#5-arquitetura-de-software)
6. [Estrutura do workspace](#6-estrutura-do-workspace)
7. [Subsistemas em detalhe](#7-subsistemas-em-detalhe)
8. [Roadmap por fases](#8-roadmap-por-fases)
9. [Estratégia de testes e qualidade](#9-estratégia-de-testes-e-qualidade)
10. [Orçamento de performance e metas](#10-orçamento-de-performance-e-metas)
11. [Riscos e mitigações](#11-riscos-e-mitigações)
12. [Protocolo de documentação](#12-protocolo-de-documentação)
13. [Convenções de código, build e features](#13-convenções-de-código-build-e-features)
14. [Legal e ética](#14-legal-e-ética)
15. [Referências de estudo](#15-referências-de-estudo)
16. [Glossário](#16-glossário)
    - [16.1 Soluções provisórias declaradas](#161-soluções-provisórias-declaradas)
17. [Log de progresso](#17-log-de-progresso)

---

## 1. Visão, objetivos e não-objetivos

### 1.1 Visão

Um frontend nativo em Rust, com módulos de jogo recompilados localmente e uma camada
de compatibilidade Xbox também em Rust (ADR-0013), que:

- ao ser executado mostra uma interface estilo *Big Picture* navegável **100% por controle**;
- lista os jogos de Xbox clássico que o usuário possui (ISO/XISO ou pasta extraída com `.xbe`);
- prepara a recompilação no primeiro uso e mantém o módulo nativo em cache nas utilizações seguintes (ainda planejado);
- ao selecionar um jogo já preparado, inicia a execução nativa **no mesmo processo e no mesmo contexto gráfico** (sem reabrir janela, sem recriar device Vulkan; integração ainda planejada);
- com o botão *Home* do controle, suspende a execução e volta ao menu **instantaneamente**;
- e que, na sua forma final, **é o próprio ambiente gráfico** de uma distro Linux minimalista — boot do kernel direto no emulador, sem servidor gráfico, sem compositor, sem gerenciador de sessão.

### 1.2 Objetivos (em ordem de prioridade)

| # | Objetivo | Métrica de sucesso |
|---|---|---|
| O1 | **Correção antes de velocidade** | Homebrew do nxdk roda pixel-correto; jogos comerciais bootam sem hacks por título |
| O2 | **Performance em hardware fraco** | 60fps em CPU dual-core ~1.6GHz com iGPU Vulkan 1.1 |
| O3 | **Latência baixíssima de input** | < 1 frame de latência adicionada pelo emulador (input→render) |
| O4 | **Zero alocação no hot path** | 0 `malloc`/`free` por frame durante gameplay (verificado por instrumentação) |
| O5 | **UX de console, não de PC** | Nenhuma tarefa essencial exige teclado ou mouse |
| O6 | **Arquitetura portável sem IFDEF espalhado** | Código específico de OS confinado a crates de *backend* |
| O7 | **Estado serializável** | Save state / quick resume funcionando como consequência do design, não como remendo |

### 1.3 Não-objetivos (explícitos — evitar escopo infinito)

- ❌ **Xbox 360 / Xbox One.** Só o Xbox original (NV2A).
- ❌ **Xbox Live.** Os servidores não existem mais. *System Link* (LAN) é opcional e fica na Fase 11.
- ❌ **Emulação LLE do BIOS/MCPX.** Não vamos bootar a ROM original (ver [4.1](#41-decisão-1-kernel-hle-não-boot-do-bios)).
- ❌ **Distribuir qualquer bit da Microsoft.** Nem BIOS, nem kernel, nem bibliotecas, nem jogos.
- ❌ **Suporte a macOS** na v1 (a arquitetura não impede, mas não é alvo).
- ❌ **GUI de desktop tradicional** (menus, barras, janelas múltiplas).
- ❌ **Hacks por título** como estratégia principal de compatibilidade.

---

## 2. Estratégia de plataformas

A proposta original começava pelo *appliance* Linux (KMS/DRM, init próprio, musl). **Invertemos a ordem de propósito**, porque depurar um emulador novo sem console, sem janela e sem debugger gráfico é masoquismo. O trabalho de *appliance* não é descartado: é a Fase 10, e a arquitetura é desenhada desde a Fase 0 para recebê-la sem reescrita.

```
ETAPA A — DESKTOP (Fases 0 → 9)          ETAPA B — APPLIANCE (Fase 10)
┌──────────────────────────────┐         ┌──────────────────────────────┐
│ Windows 10/11  │  Linux      │         │ Linux minimalista            │
│ (dev principal)│  (Wayland/  │   ──►   │ kernel + init + 1 binário    │
│                │   X11)      │         │ sem X11, sem Wayland         │
├────────────────┴─────────────┤         ├──────────────────────────────┤
│ winit (janela + eventos)     │         │ DRM/KMS atomic modesetting   │
│ wgpu → Vulkan (DX12 reserva) │         │ Vulkan direto (ash) + dmabuf │
│ WASAPI / cpal | ALSA         │         │ ALSA raw, thread SCHED_FIFO  │
│ XInput/RawInput | evdev      │         │ evdev puro                   │
│ MSVC / GNU | glibc           │         │ musl estático, LTO + PGO     │
└──────────────────────────────┘         └──────────────────────────────┘
        mesmo núcleo de emulação, trocando apenas os traits de backend
```

**Regra de ouro de portabilidade:** nenhum `#[cfg(target_os)]` dentro de `xbox-cpu`, `xbox-kernel`, `xbox-gpu`, `xbox-audio` ou `xbox-mem`. Toda diferença de SO vive em `xbox-platform` (display, input, audio, timer, memória virtual) atrás de traits. Os crates de núcleo não sabem em que SO estão.

---

## 3. O hardware que vamos emular

### 3.1 Especificação

| Componente | Especificação | Implicação para o emulador |
|---|---|---|
| **CPU** | Intel Pentium III "Coppermine" (vendido como Mobile Celeron), **733 MHz**, FSB 133 MHz, L1 32 KB, L2 128 KB on-die | x86 **32-bit**, protected mode, paging. ISA: base 686 + CMOV + x87 + MMX + **SSE** (sem SSE2!). Mesma família do host x86_64 → abre caminho para dynarec trivial e virtualização |
| **RAM** | **64 MB** DDR unificada, 128-bit @ 200 MHz (≈6.4 GB/s), compartilhada CPU+GPU | Arena única de 64 MiB. Dev kits tinham 128 MB (feature opcional — alguns homebrews querem) |
| **GPU** | NVIDIA **NV2A** @ 233 MHz — híbrido GeForce3/GeForce4. 4 pixel pipelines × 2 TMUs, 2 unidades de vertex shader | Pipeline **DX8-era**: vertex programs programáveis (VS 1.1-like) + **register combiners** (não há pixel shader HLSL; são combinadores configuráveis) + fixed-function T&L. Comandos chegam por **pushbuffer** (FIFO DMA) |
| **APU** | NVIDIA MCPX "SoundStorm": **Voice Processor** (256 voices PCM, 64 com 3D), **GP** (DSP de processamento/3D), **EP** (DSP de encode Dolby Digital 5.1) | Mixagem em software a 48 kHz. GP/EP são mal documentados → começam como stub |
| **Southbridge** | MCPX: USB (OHCI), IDE/ATA, NIC nForce 100 Mbit, SMBus (SMC + EEPROM), AC97 | Controles são **dispositivos USB** (classe XID). HD e DVD são ATA |
| **Armazenamento** | HD 8 GB (às vezes 10) formatado **FATX**, DVD-ROM com filesystem **XDVDFS** | Precisamos de parser XDVDFS (jogos) e FATX (saves/cache/HD virtual) |
| **Executável** | `.XBE` — PE modificado, base `0x00010000`, imports do kernel **por ordinal** via *thunk table* | Loader próprio. Entry point e thunk address são XOR-ados com chaves distintas para retail/debug/Chihiro |
| **Kernel** | `xboxkrnl.exe` — ~380 ordinais exportados, API tipo Windows NT enxuta | **Vamos reimplementar isto** (ver 4.1) |
| **Vídeo** | 480i/480p NTSC (59.94 Hz), 576i PAL (50 Hz), 720p/1080i em alguns títulos | Relógio de frame deriva do vblank do encoder de TV, não do monitor do host |
| **Timer de referência** | `KeQueryPerformanceFrequency` = **3.375 MHz**; RDTSC = 733.33 MHz | Base para todo o modelo de tempo do guest |

### 3.2 Mapa de endereços (esqueleto)

> ⚠️ **Validar cada faixa contra a documentação na implementação** (XboxDevWiki / fontes do xemu). A tabela abaixo é o esqueleto de trabalho, não verdade final.

| Faixa | Conteúdo | Tratamento |
|---|---|---|
| `0x00000000`–`0x03FFFFFF` | RAM física (64 MB) | Arena, acesso direto |
| `0x04000000`–`0x07FFFFFF` | RAM extra em dev kit (128 MB) | Arena, se feature ativa |
| `0x80000000`–`0x83FFFFFF` | Mapeamento contíguo do kernel sobre a RAM | Mesma arena, outro alias |
| `0xF0000000`–`0xF3FFFFFF` | Memória *tiled* (acesso com swizzle da GPU) | Alias com tradução de endereço |
| `0xFD000000`–`0xFDFFFFFF` | MMIO do NV2A (PMC, PBUS, PFIFO, PGRAPH, PCRTC, PRAMDAC, PTIMER, PRAMIN…) | Dispatch de MMIO → `xbox-gpu` |
| `0xFE800000`–` ` | MMIO do APU (VP/GP/EP) | Dispatch → `xbox-audio` |
| `0xFEC00000`–` ` | AC97 | Dispatch → `xbox-audio` |
| `0xFED00000`–` ` | USB OHCI | Dispatch → `xbox-input` (ou HLE, ver 7.7) |
| `0xFEF00000`–` ` | NIC nForce | Stub (Fase 11) |
| `0xFF000000`–`0xFFFFFFFF` | Flash ROM (espelhada) | Stub — não bootamos BIOS |

### 3.3 O que um jogo de Xbox realmente é

Isto determina **toda** a arquitetura, então vale explicitar:

1. O `.xbe` é um PE com base `0x10000`. Ele **contém, estaticamente linkadas, as bibliotecas da Microsoft**: `D3D8`, `XGRAPHC`, `DSOUND`, `XAPILIB`, `LIBCMT`. O certificado do XBE declara as versões dessas libs.
2. Logo, **o "driver gráfico" roda dentro do jogo**. O D3D8 do Xbox não faz chamadas de sistema para desenhar: ele **escreve comandos direto no pushbuffer** e toca registradores MMIO do NV2A em user mode.
3. O jogo só chama o kernel para coisas de sistema: alocar memória contígua, descobrir endereços físicos, I/O de arquivo, threads, sincronização, vídeo (`AvSetDisplayMode`), EEPROM, USB.

**Consequência:** se emularmos (a) o conjunto de ordinais do kernel e (b) o NV2A no nível de pushbuffer/MMIO, **o D3D8 original do jogo roda sem modificação e sem patch por título**. Esse é o pilar da nossa compatibilidade.

---

## 4. Decisões arquiteturais fundamentais

Cada decisão abaixo deve virar um ADR formal em `docs/adr/` na Fase 0.

### 4.1 Decisão 1: Kernel HLE, não boot do BIOS

**Escolhido:** reimplementar `xboxkrnl` em Rust (HLE) e carregar o `.xbe` diretamente.

**Rejeitado:** LLE — bootar a MCPX + BIOS retail e deixar o kernel real rodar (caminho do xemu/XQEMU).

| | HLE (escolhido) | LLE (rejeitado) |
|---|---|---|
| Usuário precisa de dump de BIOS? | **Não** | Sim (barreira legal e prática) |
| Performance | Muito melhor (I/O, threads, heap em código nativo) | Pior (emula ATA, SMC, MMU completa, criptografia de boot) |
| Compatibilidade | Depende da nossa cobertura de ordinais | Altíssima "de graça" |
| Precisão de edge cases | Nós mesmos temos que acertar | Correta por construção |
| Esforço inicial | Alto (≈380 ordinais) | Alto (hardware de boot + ATA + segurança) |
| Save state | Projetável do zero | Mais simples (só estado de hardware) |

A decisão sacrifica precisão por performance e por não exigir um dump de BIOS do usuário — coerente com O2 e com a meta de *appliance*. O custo é cobertura de ordinais, que é trabalho **incremental e mensurável** (telemetria de ordinal faltando).

### 4.2 Decisão 2: GPU em nível de pushbuffer, não HLE de D3D8

**Escolhido:** interpretar o command stream do NV2A (PFIFO → métodos da classe 3D) e traduzir para Vulkan.

**Rejeitado:** detectar as funções do D3D8 dentro do XBE por assinatura e substituí-las por implementação nativa (caminho do Cxbx-Reloaded).

Motivo: HLE de D3D8 exige um banco de assinaturas por versão de biblioteca e quebra a cada título que linka uma build diferente. Pushbuffer é **uma interface só, estável, documentada pelo hardware**. É mais trabalho no começo e muito menos manutenção depois — e é a única rota viável para um projeto que não quer hacks por jogo.

### 4.3 Decisão 3: recompilação estática primeiro (ADR-0013)

A direção principal passa a ser `XBE → análise x86 em Rust → IR → Rust gerado →
rustc → módulo nativo`. `xbox-recomp` e `xberecomp` entregam a primeira prova. O
interpretador permanece planejado como oráculo; JIT e hypervisor ficam adiados.
Não há fallback automático implementado. A escada abaixo é o plano anterior do
ADR-0003, preservado como referência dos backends possíveis:

```
                 precisão ──────────────────────────► velocidade
   ┌──────────────┐     ┌──────────────────┐     ┌────────────────────┐
   │ Interpreter  │     │ JIT (dynarec)    │     │ Hypervisor         │
   │ Fase 3a      │ ──► │ Fase 3b          │ ──► │ Fase 3c            │
   │ ~50-150 cy/i │     │ ~1.5-3× nativo   │     │ ~1× nativo         │
   │ oráculo/debug│     │ caminho padrão   │     │ caminho ultra      │
   └──────────────┘     └──────────────────┘     └────────────────────┘
```

- **AOT:** prioridade atual. Estado de registradores e pilha do guest explícitos;
  acessos pelo bus e pausa em fronteiras de blocos. Cobertura comercial ainda por validar.
- **Interpretador:** infraestrutura de teste futura, não implementada.
- **JIT e hypervisor:** alternativas futuras; não condicionam a primeira entrega AOT.

O primeiro protótipo exige Cargo/rustc no desenvolvimento e gera executável separado
para teste. Cache de jogos, módulos carregados no frontend e distribuição sem
toolchain de desenvolvimento ainda precisam de projeto e implementação.

### 4.4 Decisão 4: serialização desde o dia zero

Save state, *quick resume* e replay determinístico são impossíveis de adicionar depois num emulador com estado espalhado em ponteiros e handles do host. Então, desde a Fase 0:

- todo estado emulado implementa um trait `Snapshot` (serialize/deserialize determinístico);
- **nenhum** ponteiro do host é guardado dentro de estruturas do guest — só índices/handles em arenas;
- o grafo de objetos do kernel HLE é uma arena indexada, não `Arc<dyn Object>`;
- fontes de não-determinismo (tempo real, RNG, ordem de thread) passam por um único módulo `xbox-core::timing`/`entropy` que é serializável e substituível.

### 4.5 Decisão 5: *fastmem* (memória virtual do host como MMU)

A RAM do guest é mapeada no espaço de endereços do host (`mmap` / `VirtualAlloc2` + `MapViewOfFile3`) de forma que um acesso do guest vire **um acesso de memória direto**, sem checagem de faixa. MMIO e páginas protegidas são páginas sem permissão; o fault é capturado por `SIGSEGV` (Linux) ou **VEH** (Windows) e roteado para o bus. É a diferença entre ~3 e ~15 instruções por acesso à memória do guest.

**Nota de projeto:** isso implica `panic = "unwind"` ou cuidado extra no handler, e implica que o crate `xbox-mem` é um dos poucos com `unsafe` autorizado.

### 4.6 Decisão 6: wgpu agora, `ash` provavelmente depois

`wgpu` dá Vulkan + DX12 + validação + portabilidade com pouco código — ideal para as Fases 5–9. Mas:

- o NV2A tem estados que o wgpu não expõe bem (combinadores, formatos exóticos, *point sprites*, modos de blend legados);
- **`wgpu` não apresenta em KMS/DRM direto** (não usa `VK_KHR_display`), o que é exatamente o que a Fase 10 precisa.

Então o renderer é desenhado atrás de um trait `GpuBackend` com implementação `wgpu` na Etapa A, e a Fase 10 avalia um backend `ash` (Vulkan puro) com `VK_KHR_display`. Decidir isso cedo evita acoplar o tradutor NV2A às estruturas do wgpu.

#### 4.6.1 Vulkan é desde a Fase 5, não a partir da Fase 10

Para não haver dúvida: **Vulkan não é "a API de quando migrarmos para Linux"** — é a API de render do projeto desde o primeiro triângulo desenhado, nos dois sistemas operacionais.

| | Etapa A (Windows + Linux desktop) | Etapa B (Linux full, Fase 10) |
|---|---|---|
| API gráfica | **Vulkan** (via `wgpu`) | **Vulkan** (via `ash`) |
| Shaders | SPIR-V | SPIR-V — idêntico |
| Tradutor NV2A, caches, texturas | o mesmo código | o mesmo código |
| Origem da surface | `winit` (janela do SO) | `VK_KHR_display` sobre KMS/DRM |
| Compositor | DWM / X11 / Wayland | **nenhum** |

Decisões que decorrem disso:

- **No Windows, fixamos o backend Vulkan do wgpu** (`WGPU_BACKEND=vulkan` / `Backends::VULKAN` no código), deixando DX12 apenas como reserva para driver quebrado. Motivo: um só caminho de código gráfico. Se desenvolvermos em DX12 e rodarmos o produto final em Vulkan, bugs de driver e diferenças de semântica aparecem **só na máquina-alvo**, que é o pior lugar possível para descobri-los.
- O que a Fase 10 troca é a **apresentação** (de onde vem a `VkSurface` e quem faz o *page flip*), não a API, não os shaders, não o tradutor.
- Logo, o risco R3 é de *apresentação e cobertura de estado*, não de "precisar aprender Vulkan depois".

---

## 5. Arquitetura de software

### 5.1 Visão em camadas (Etapa A — desktop)

```
┌───────────────────────────────────────────────────────────────────────────┐
│                        FRONTEND  (xbox-ui)                                │
│      Big Picture: Home · Biblioteca · Detalhe · Overlay in-game           │
│                        egui sobre o mesmo device gráfico                  │
└───────────────┬───────────────────────────────────────┬───────────────────┘
                │ comandos (Launch/Suspend/Resume/Exit) │ estado p/ render
┌───────────────▼───────────────────────────────────────▼───────────────────┐
│                      ORQUESTRADOR  (apps/obsidian)                        │
│   máquina de estados da sessão · scheduler de threads · hot-swap de modo   │
└───┬──────────────┬──────────────┬──────────────┬──────────────┬───────────┘
    │              │              │              │              │
┌───▼────┐  ┌──────▼─────┐  ┌─────▼──────┐  ┌────▼─────┐  ┌─────▼────────┐
│xbox-cpu│  │xbox-kernel │  │ xbox-gpu   │  │xbox-audio│  │ xbox-input   │
│interp/ │  │HLE ~380    │  │NV2A: PFIFO │  │APU: VP + │  │XID/USBD HLE  │
│jit/hv  │◄─┤ordinais    │  │→ IR → SPIR │  │GP/EP stub│  │4 portas      │
└───┬────┘  └──────┬─────┘  └─────┬──────┘  └────┬─────┘  └─────┬────────┘
    │              │              │              │              │
┌───▼──────────────▼──────────────▼──────────────▼──────────────▼──────────┐
│                  xbox-mem — arena 64 MiB + MMU + bus MMIO                 │
└───────────────────────────────┬───────────────────────────────────────────┘
┌───────────────────────────────▼───────────────────────────────────────────┐
│       xbox-platform — traits de backend (display/input/audio/timer/vm)    │
│   win: Vulkan/DX12 · WASAPI · XInput/RawInput · QPC · VirtualAlloc2       │
│   lin: Vulkan · ALSA/cpal · evdev · clock_gettime · mmap                  │
│   kms (Fase 10): DRM/KMS atomic · ALSA raw · evdev · mmap+hugepages       │
└───────────────────────────────────────────────────────────────────────────┘
```

### 5.2 Modelo de threads

O Xbox é mono-core, mas o emulador **não** precisa ser.

| Thread | Prioridade | Responsabilidade | Comunicação |
|---|---|---|---|
| **Main / UI** | normal | eventos de janela, egui, orquestração | canais |
| **Guest CPU** | alta | executa código do guest (interp/JIT/HV) + kernel HLE + scheduler de threads do guest | escreve pushbuffer na arena |
| **GPU (PFIFO)** | alta | consome pushbuffer, traduz métodos, emite comandos Vulkan | lê arena; semáforos/notifiers para sincronizar com o guest |
| **Render / Present** | alta | submete, apresenta, compõe overlay da UI | fila de command buffers |
| **Audio** | **tempo real** (SCHED_FIFO / `AvSetMmThreadCharacteristics`) | mixa voices do APU, alimenta ring buffer do SO | ring lock-free |
| **I/O** | baixa | leitura de XISO/FATX, cache de shaders, scraping local | completions |

Desacoplar CPU-guest de GPU é o que dá os 60fps em hardware fraco; o preço é implementar corretamente os **semáforos e notifiers** do NV2A que o D3D8 usa para esperar a GPU.

### 5.3 Fluxo de boot de um jogo

```
UI: usuário escolhe jogo
  │
  ├─ 1. monta mídia:  XISO/pasta  → vfs `D:\`
  ├─ 2. EEPROM: região/idioma/flags de vídeo (sintética, editável nas settings)
  ├─ 3. zera arena de 64 MiB, cria page tables do guest
  ├─ 4. carrega default.xbe: valida header, desXOR entry+thunk, mapeia seções,
  │     inicializa TLS, cria heap do guest
  ├─ 5. resolve a thunk table: cada ordinal → endereço de trampolim HLE
  ├─ 6. inicializa dispositivos: NV2A reset, APU reset, XID nas portas, timers
  ├─ 7. cria thread principal do guest em XbeHeader.entry_point
  └─ 8. UI hiberna seu estado; orquestrador dá o contexto gráfico ao emulador
          │
          ▼
    loop de emulação ──(botão Home)──► suspende threads, UI reassume, estado intacto
```

### 5.4 Fluxo de um frame

```
 guest (D3D8 dentro do jogo)            emulador
 ───────────────────────────            ────────────────────────────────────────
 escreve métodos no pushbuffer  ──►  PFIFO parser lê o ring (GET/PUT)
 toca registrador PUT           ──►  decodifica classe/método/args
                                     atualiza state tracker (dirty flags)
 SET_BEGIN_END(begin)           ──►  resolve pipeline: VP→SPIR-V (cache),
                                     combiners→SPIR-V (cache), textures (cache)
 vertex data / índices          ──►  copia p/ ring buffer de VB/IB na VRAM
 SET_BEGIN_END(end)             ──►  draw call Vulkan
 semáforo / notifier            ──►  sinaliza quando a GPU do host terminou
 flip de surface                ──►  resolve/blit p/ framebuffer, present,
                                     dispara IRQ de vblank no guest
```

---

## 6. Estrutura do workspace

```
emulador-xbox/
├── Cargo.toml                    # workspace, profiles, lints compartilhados
├── rust-toolchain.toml           # toolchain pinada (1.98.1)
├── rustfmt.toml                  # max_width = 100
├── .cargo/config.toml            # aliases: cargo lint, cargo ci
├── .github/workflows/ci.yml      # fmt · clippy · test · release · bench, Win + Linux
├── ROADMAP.md                    # este arquivo
├── CHANGELOG.md                  # Keep a Changelog
├── docs/
│   ├── adr/                      # Architecture Decision Records (0000 = template)
│   ├── arch/                     # um documento por subsistema
│   ├── compat/                   # matriz de compatibilidade
│   └── dev/                      # build.md, third-party.md
├── crates/
│   ├── xbox-core/                # tipos base, tempo, erro, trait Snapshot, config
│   │   └── bench/                #   pacote de benchmarks, FORA do workspace (ADR-0008)
│   ├── xbox-formats/             # xbe, xdvdfs(xiso), fatx, eeprom, savemeta
│   ├── xbox-mem/                 # arena, MMU, fastmem, bus MMIO            [unsafe]
│   ├── xbox-cpu/                 # trait Cpu + decoder x86-32
│   │   ├── src/interp/           #   backend interpretador
│   │   ├── src/jit/              #   backend dynarec (cranelift)           [unsafe]
│   │   └── src/hv/               #   backend KVM / WHPX                    [unsafe]
│   ├── xbox-kernel/              # HLE do xboxkrnl: objetos, threads, Mm, Io, Rtl…
│   ├── xbox-gpu/                 # NV2A: MMIO, PFIFO, state tracker, shaders, texturas
│   │   ├── src/pfifo/
│   │   ├── src/state/
│   │   ├── src/shader/           #   VP + combiners → IR → SPIR-V
│   │   └── src/backend/          #   trait GpuBackend; impl wgpu (e ash na Fase 10)
│   ├── xbox-audio/               # APU: voice processor, mixbins, GP/EP
│   ├── xbox-input/               # XID/USBD HLE + mapeamento + perfis
│   ├── xbox-platform/            # traits de SO + impls: windows/ linux/ kms/
│   ├── xbox-ui/                  # Big Picture (egui), navegação por gamepad
│   └── xbox-dbg/                 # GDB stub, tracer de pushbuffer, dumps, profiler
├── apps/
│   └── obsidian/                 # binário final (orquestrador + CLI)
├── tools/
│   ├── xbetool/                  # inspeciona/extrai .xbe
│   ├── isotool/                  # lista/extrai XISO, converte
│   ├── pbreplay/                 # grava e reproduz traces de pushbuffer
│   └── ordinal-report/           # relatório de cobertura de ordinais do kernel
├── tests/                        # integração; fixtures geradas por nós (nxdk)
└── benches/                      # criterion: cpu, mem, pfifo, mixer
```

---

## 7. Subsistemas em detalhe

### 7.1 `xbox-formats` — formatos de dados

- **XBE:** header (`XBEH`), certificate (title ID, nome, região, media flags, versões de biblioteca), tabela de seções (+flags de preload/head-page), TLS directory, kernel thunk table, logo bitmap (RLE), libraries. Tratar as **três famílias de chave XOR** (retail / debug / Chihiro) para entry point e thunk address — detectar por tentativa e validação de faixa.
- **XDVDFS (XISO):** árvore de diretórios em árvore binária, leitura por setor de 2048 bytes, suporte às variantes de offset (imagem "redump" com partição de vídeo vs. imagem cortada). Read-only. Também aceitar **pasta extraída** com `default.xbe`.
- **FATX:** parser read-only na v1 (ler HD virtual/memory unit), escrita na Fase 9 (saves de verdade). Nomes de 42 chars, clusters, duas variantes de tamanho de entrada.
- **EEPROM:** 256 bytes — região, idioma, flags de vídeo (480p/720p/widescreen), configuração de áudio, serial, chave de HD. **Geramos uma sintética** e deixamos editável pelas settings; aceitamos import de dump do usuário.
- **Metadados:** extração do nome e do logo do XBE para a biblioteca da UI.

### 7.2 `xbox-mem` — memória e barramento

- Arena contígua de 64 MiB (feature `ram128` → 128 MiB), alinhada e, no Linux, com *huge pages* (`MADV_HUGEPAGE` / `MAP_HUGETLB` quando disponível).
- **Fastmem:** reserva de espaço virtual com os aliases do guest (físico, contíguo `0x80000000`, tiled `0xF0000000`) apontando para o **mesmo backing store** (memfd/`CreateFileMapping` + múltiplas views).
- **MMU:** page tables do guest (4 KiB e 4 MiB), cache de tradução, bits de dirty/accessed, exceções `#PF`. Mesmo com fastmem, o modelo de paging existe porque o kernel HLE e o JIT dependem dele.
- **Bus MMIO:** tabela de regiões → handlers. Acesso a página sem permissão → `SIGSEGV`/VEH → decodifica a instrução faltante → despacha para o device → retoma.
- **Tracking de dirty** por página: essencial para invalidação de cache de textura e de blocos do JIT (código automodificável).

### 7.3 `xbox-cpu` — CPU

**Decoder** compartilhado pelos três backends: x86-32, prefixos, ModRM/SIB, 686 base, `CMOV`, `BSWAP`, `CMPXCHG`, `XADD`, x87 completo (incluindo controle de precisão — jogos dependem disso), MMX e **SSE** (sem SSE2). Instruções não usadas pelo Xbox podem levantar `#UD` explicitamente em vez de serem implementadas.

- **`interp/`:** laço de despacho, FLAGS avaliadas *lazily* (guardar operandos + operação, computar só quando lido), `fpu` com 80-bit real (via `softfloat` ou x87 do host com cuidado).
- **`jit/`:** blocos básicos → Cranelift IR → código nativo; *block linking* direto, cache indexado por (endereço físico, flags de modo), invalidação por página suja, *guest register allocation* em registradores do host, fastmem sem bounds check, saída rápida para trampolins do kernel.
- **`hv/`:** VM 32-bit flat; GDT/IDT mínimas montadas por nós; thunks do kernel em uma página armadilha; MMIO do NV2A/APU mapeado como região que causa exit; `kvm-ioctls` no Linux, `WHvRunVirtualProcessor` no Windows.

**Oráculo de teste:** no Linux, usar **KVM como referência** — executar a mesma instrução + estado inicial na CPU real e comparar o estado final com o do interpretador. Isso dá cobertura de ISA que nenhuma suíte escrita à mão alcança.

### 7.4 `xbox-kernel` — kernel HLE

Agrupamento dos ordinais por família, com ordem de implementação:

| Grupo | Exemplos | Prioridade |
|---|---|---|
| **Loader/Imagem** | `XeImageHeader`, `XeLoadSection`, `XeUnloadSection` | P0 |
| **Memória** | `MmAllocateContiguousMemory(Ex)`, `MmFreeContiguousMemory`, `MmGetPhysicalAddress`, `MmPersistContiguousMemory`, `MmQueryAllocationSize`, `MmMapIoSpace`, `MmAllocateSystemMemory` | P0 (D3D8 depende) |
| **RTL** | `RtlCreateHeap`/`RtlAllocateHeap`/`RtlFreeHeap`, strings ANSI/Unicode, `RtlEnterCriticalSection`, `RtlTime*`, `RtlAssert` | P0 (heap real na memória do guest) |
| **Threads/Sync** | `PsCreateSystemThread(Ex)`, `KeWaitForSingleObject/MultipleObject`, `KeSetEvent`, `KeInitializeSemaphore`, `KeDelayExecutionThread`, `KeSetTimerEx`, `KeInsertQueueApc/Dpc`, `KeRaiseIrql` | P0 |
| **Objetos/Handles** | `NtCreateEvent`, `NtWaitForSingleObject`, `NtClose`, `ObReferenceObjectByHandle` | P0 |
| **I/O** | `NtCreateFile`, `NtReadFile`, `NtWriteFile`, `NtQueryDirectoryFile`, `NtQueryInformationFile`, `IoCreateDevice`, `NtQueryVolumeInformationFile` | P0 |
| **Vídeo** | `AvSetDisplayMode`, `AvSendTVEncoderOption`, `AvGetSavedDataAddress` | P0 |
| **Sistema** | `ExQueryNonVolatileSetting` (↔ EEPROM), `HalReturnToFirmware` (↔ sair do jogo), `HalReadSMCTrayState`, `KeTickCount`, `KeQueryPerformanceCounter/Frequency` | P1 |
| **USB/Input** | `XapiInitProcess`, camada USBD/XID (ver 7.7) | P1 |
| **Cripto** | `XcSHA1*`, `XcHMAC`, `XcRC4*`, `XcDESKey*` | P1 (saves e títulos que verificam) |
| **Rede/Online** | `XNet*`, `XOnline*` | P3 (stub que falha graciosamente) |
| **Debug** | `DbgPrint`, `KdDebuggerEnabled`, `OutputDebugStringA` | P1 (ouro para depurar jogos) |

**Scheduler do guest:** nós implementamos o scheduler — N threads do guest multiplexadas sobre o(s) backend(s) de CPU, com prioridades do Xbox, quantum, APC/DPC e IRQL. Precisa ser determinístico (ver 4.4): troca de contexto em pontos de contagem de ciclos, não em tempo de parede.

**Telemetria obrigatória:** cada chamada a ordinal não implementado é contada e logada com o PC do chamador. `tools/ordinal-report` gera a lista "o que falta para o jogo X". Isso transforma compatibilidade em uma fila de trabalho concreta.

### 7.5 `xbox-gpu` — NV2A

O subsistema mais trabalhoso do projeto. Divisão:

**a) MMIO e engines.** PMC (controle/IRQ), PBUS, PFIFO (ring, GET/PUT, DMA objects, *puller*), PGRAPH (engine 3D), PCRTC (scanout, flip), PRAMDAC (encoder/modos), PTIMER, PVIDEO (overlay de vídeo — alguns jogos usam para FMV), PRAMIN (instance memory).

**b) Parser de pushbuffer.** Decodificar os pacotes de método: incrementing/non-incrementing, jumps, calls/returns, `NV_SET_OBJECT`, e os métodos da classe 3D (Kelvin/NV20). Cada método atualiza o **state tracker** com dirty flags por grupo de estado.

**c) State tracker.** Viewport/scissor, matrizes e estado de T&L fixo, formatos de atributo de vértice (array, inline, inline-array, indexado), 4 unidades de textura (wrap, filtros, LOD bias, bump), register combiners (até 8 estágios + final combiner), alpha/depth/stencil/blend, surface setup (color + zeta, pitch, formato, tiling), modos de anti-alias, *clip planes*.

**d) Tradução de shaders.** Pipeline: `bytecode NV2A → IR própria → otimização → SPIR-V`.
- **Vertex programs** (xvs/VS1.1-like): 128 instruções, swizzles, registradores constantes, `R12`/posição.
- **Fixed-function T&L:** gerar um vertex shader equivalente a partir do estado (luzes, materiais, fog, texgen, skinning).
- **Register combiners:** traduzir os estágios RGB/alpha + final combiner para um fragment shader. É a parte mais sutil da emulação gráfica do Xbox (mapeamentos de sinal, escala/bias, `AB+CD`, dot products).
- Cache em disco de SPIR-V compilado, indexado por hash de estado, para eliminar *stutter* de compilação.

**e) Texturas.** Deswizzle (Morton order), formatos: `A8R8G8B8`, `X8R8G8B8`, `R5G6B5`, `A1R5G5B5`, `A4R4G4B4`, `A8`, `L8`, `AL8`, `DXT1/DXT3/DXT5`, paletizados `P8`, formatos *signed*, cubemaps, mipmaps, texturas 3D, texturas lineares não-swizzled. Cache indexado por (endereço, formato, dimensões) + invalidação por página suja.

**f) Surfaces e apresentação.** Gerenciar color/zeta surfaces na RAM unificada, detectar *render-to-texture*, tiling, resolve de MSAA, e o **flip** → present. Suporte a **escala de resolução interna** (render em 2×/3×/4× e manter a UI nítida) como melhoria da Fase 11 — mas a arquitetura de surfaces precisa prever isso desde já.

**g) Timing.** vblank a 59.94 Hz (NTSC) / 50 Hz (PAL) gerando IRQ no guest, independente da taxa do monitor do host; *frame pacing* com fila de apresentação e, opcionalmente, VRR.

### 7.6 `xbox-audio` — APU

- **Voice Processor:** até 256 voices. Por voice: endereço de buffer na RAM do guest, formato (PCM8/PCM16/**ADPCM do Xbox**), *pitch* (resample), envelope/volume, loop points, roteamento para até 32 *mixbins*. Mixagem SIMD (SSE2 no host, sem problema — o host é moderno).
- **GP (DSP de processamento):** começa como stub. Jogos que usam DirectSound3D com processamento no DSP terão áudio simplificado (downmix direto) até a implementação real. Documentar explicitamente quais títulos dependem.
- **EP (encode Dolby Digital):** **não** vamos gerar AC3 — pegamos o PCM pré-encode e fazemos downmix para estéreo/5.1 no host.
- **Saída:** ring buffer lock-free; backend WASAPI (Windows) e ALSA direto (Linux); `cpal` como fallback de desktop. Thread de tempo real. Latência alvo < 30 ms.
- **Sincronização:** opção de "áudio como relógio mestre" com *dynamic rate control* (resample fino ±0.5%) para eliminar crackle sem travar o vídeo.

### 7.7 `xbox-input` — controles

O controle do Xbox é USB (classe **XID**). Duas rotas:

| Rota | Como | Veredito |
|---|---|---|
| **LLE OHCI** | emular o controlador USB + descritores XID; o `XAPILIB` do jogo enumera de verdade | Preciso, mais custoso; **plano B** |
| **HLE USBD/XID** | implementar a camada USBD do kernel e entregar pacotes de estado XID direto | Mais simples e rápido; **plano A** |

Começamos no HLE e mantemos a possibilidade de OHCI atrás do mesmo trait, para títulos que façam enumeração exótica.

- 4 portas, hotplug, *memory units* nos slots do controle (mapeadas para pastas do host).
- Tipos: Duke, Controller S, e mapeamento de qualquer gamepad moderno para eles (incluindo os dois botões analógicos black/white).
- Rumble (`XInputSetState` → feedback do host).
- Backends: Windows (XInput + RawInput para exóticos), Linux desktop (`gilrs`/evdev), Linux appliance (**evdev puro**, sem SDL).
- Perfis de mapeamento por jogo, deadzones, curvas de resposta, **combinação Home** configurável (padrão: botão Guide, ou Back+Start).

### 7.8 `xbox-platform` — abstração de SO

Traits mínimos e estáveis:

```rust
trait DisplayBackend  { /* criar surface, modos, present, vsync, hotplug */ }
trait InputBackend    { /* enumerar, poll de eventos, rumble */ }
trait AudioBackend    { /* abrir stream, escrever frames, latência real */ }
trait HostTimer       { /* monotônico de alta resolução, sleep preciso */ }
trait VirtualMemory   { /* reservar, mapear alias, proteger, handler de fault */ }
trait Hypervisor      { /* criar VM, vcpu, run, exits */ }
```

Implementações: `windows/`, `linux/`, `kms/` (Fase 10). Nenhum outro crate conhece a existência de Windows ou Linux.

### 7.9 Tempo e sincronização

Um único relógio virtual em `xbox-core::time`, em unidades de ciclo do guest (733.33 MHz), do qual derivam: RDTSC, `KeQueryPerformanceCounter` (3.375 MHz), `KeTickCount` (1 ms), vblank, sample clock do APU (48 kHz), e os timers do kernel. Modos de sincronização:

- **Vsync do host** (padrão no desktop);
- **Áudio como mestre** (padrão no appliance — elimina crackle, que é mais perceptível que judder);
- **Livre/turbo** (para benchmarks e testes).

Todo evento agendado vive numa fila de eventos por *deadline em ciclos*, não em `Instant` do host — pré-requisito do determinismo (4.4).

Estado AOT atual (ADR-0025–0028): `Clock` integrado à sessão e RDTSC, com custo
sintético de um ciclo virtual por instrução/elemento REP concluído. Snapshot
AOTC v6 preserva tempo, perfil, parada por HLT e breakpoint. CLI limpa IF guest;
HLT e INT3 entregam controle ao host com diagnóstico. Polls preservam estado/tempo,
sem entrega de IRQ/IDT ou retomada automática.
SFENCE usa a barreira do bus síncrono exclusivo, sem estado/fila novo ou conclusão
de GPU. Escrita diferida e observadores concorrentes exigem ampliar esse contrato.
Não representa latência física; o Scheduler
existente e os domínios de vídeo/áudio/kernel ainda precisam ser conectados.

### 7.10 `xbox-ui` — Big Picture

- Renderiza com `egui` **sobre o mesmo device/queue** do emulador, para que entrar e sair do jogo não recrie nada.
- Navegação por D-pad/analógico com foco visível, som de navegação, animações de 60fps e **orçamento de < 5% de CPU** quando ocioso (redesenho só sob evento + animação).
- Telas: **Home** (continuar, recentes) → **Biblioteca** (grid com capas/logos do XBE, filtro, ordenação) → **Detalhe** (metadados, tempo jogado, opções por jogo) → **Em jogo** → **Overlay** (resume, save/load state, screenshot, configurações rápidas, sair).
- **Configurações:** vídeo (resolução interna, filtro, aspect, vsync), áudio (backend, latência, downmix), input (mapeamento, deadzone, perfis), sistema (RAM 64/128, região, idioma, EEPROM), avançado (backend de CPU, logs, debug).
- Biblioteca: scanner de pastas, cache em SQLite ou arquivo binário próprio, capas locais (`<jogo>.png`) — **sem scraping online na v1**.

### 7.11 `xbox-dbg` — ferramentas de desenvolvimento

Investimento que se paga: é o que diferencia "meses" de "anos" no tempo de depuração.

- **GDB stub** para o guest (breakpoints, watchpoints, registradores, memória, step).
- **Tracer de pushbuffer:** gravar o stream, reproduzir offline (`pbreplay`), inspecionar métodos frame a frame.
- **Visualizador de estado da GPU:** surfaces, texturas, shaders traduzidos (NV2A + SPIR-V lado a lado).
- **Dumps:** textura, framebuffer, shader, região de RAM.
- **Profiler interno:** tempo por subsistema, instruções/s, draws/frame, voices ativas, com overlay no jogo.
- **Modo diferencial:** interpretador e JIT executando a mesma trace e comparando estado a cada bloco.

---

## 8. Roadmap por fases

Marcação: `[ ]` pendente · `[~]` em andamento · `[x]` concluído. **Cada item concluído é marcado com data e referenciado no CHANGELOG** (ver [§12](#12-protocolo-de-documentação)).

---

### Fase 0 — Fundação do projeto *(sem nenhuma emulação)*

**Objetivo:** ter um esqueleto onde cada fase seguinte apenas preenche lacunas.

- [x] *(2026-10-19)* Workspace Cargo com todos os crates de [§6](#6-estrutura-do-workspace) — 11 crates, 1 app, 4 ferramentas
- [x] *(2026-10-19)* `rust-toolchain.toml` com toolchain pinada; `.cargo/config.toml` com aliases `lint` e `ci`
- [x] *(2026-10-19)* Lints de workspace: `clippy` pedante, `unsafe_code = "deny"` com `#![allow(unsafe_code)]` explícito apenas em `xbox-mem`, `xbox-cpu`, `xbox-gpu` e `xbox-platform`
- [x] *(2026-10-19)* `xbox-core`: erros tipados, trait `Snapshot`, relógio virtual + agendador, config TOML — 26 testes
- [x] *(2026-10-19)* Logging/tracing com filtro por subsistema e `OBSIDIAN_LOG`; feature `trace-verbose`
- [x] *(2026-10-19)* CI (GitHub Actions): `fmt` · `clippy -D warnings` · `test` · `doctest` · build release · bench compila — **Windows + Linux**
- [x] *(2026-10-19)* Infra de bench (`criterion`, em pacote separado — ADR-0008)
- [ ] Infra de fuzz (`cargo-fuzz`) — movida para a Fase 1, onde existirão parsers para fuzzar
- [x] *(2026-10-19)* `docs/` com ADRs 0001–0008, `docs/arch/core.md`, `docs/dev/build.md`, `docs/dev/third-party.md`
- [x] *(2026-10-19)* `CHANGELOG.md` + template de ADR
- [x] *(2026-10-19)* Binário `obsidian` com `--version`, `--help`, `doctor`, e stubs que citam a fase responsável

**Aceite:** ✅ `cargo build --workspace --release` limpo · `cargo clippy -D warnings` limpo · `cargo fmt --check` limpo · 30 testes + 1 doctest verdes · `obsidian doctor` funcional.
CI escrito; a publicação no GitHub foi autorizada em 2026-10-19. Execuções remotas
devem ser conferidas no GitHub Actions; testes locais estão registrados em `docs/dev/recomp.md`.

---

### Fase 1 — Formatos e ferramentas de dados

**Objetivo:** entender os arquivos do usuário antes de executar qualquer coisa.

- [x] *(2026-10-19)* Parser XBE completo (header, certificate, seções, libs, TLS, thunk table, logo RLE)
- [x] *(2026-10-19)* Desofuscação de entry point e thunk address **por plausibilidade** (retail / debug / Chihiro) — chave errada vira diagnóstico, não corrupção
- [x] *(2026-10-19)* Parser XDVDFS: árvore binária de diretórios, leitura de arquivo, detecção de base da partição, terminação garantida contra ciclos
- [x] *(2026-10-19)* Suporte a jogo em **pasta extraída** (nas ferramentas; a montagem como `D:` é o item da VFS)
- [x] *(2026-10-19)* `tools/xbetool` e `tools/isotool` funcionais (`info`, `ls`, `tree`, `extract`, `--logo` em ASCII)
- [x] *(2026-10-19)* Fixtures **geradas por código** (`xbox-formats::fixtures`) + exemplo `gerar_fixtures` — nada proprietário no repositório
- [x] *(2026-10-19)* Robustez: `tests/robustez.rs` com ~24 mil mutações determinísticas + alvos `cargo-fuzz` em `fuzz/`
- [x] *(2026-10-19)* Parser FATX read-only: superbloco, FAT de 16/32 bits, cadeia de clusters com terminação garantida, leitura parcial
- [x] *(2026-10-19)* EEPROM: parse, geração sintética por região/idioma, checksums, bloco cifrado preservado (decifrar fica para a Fase 4, com os `Xc*`)
- [x] *(2026-10-19)* VFS unificada (`D:` mídia, `T:`/`U:`/`Z:` do título, `C:`/`E:` do sistema) sobre XISO/FATX/pasta do host, com caminhos de dispositivo e **bloqueio de travessia de diretório**
- [x] *(2026-10-19)* `obsidian info` e `obsidian run` operando sobre mídia real: montam a VFS, leem o executável e relatam o que falta
- [x] *(2026-10-19)* Leitura **sob demanda** (`xbox-formats::media`): as ferramentas operam numa imagem de 2,87 GB em 0,4 s, sem carregar a mídia na RAM
- [x] *(2026-10-19)* `Xbe::kernel_imports` + `xbetool --thunks`: inventário dos ordinais do kernel que um título importa — a fila de trabalho da Fase 4
- [~] **Conformidade** de [`docs/compat/formatos.md`](docs/compat/formatos.md): **9 itens fechados** contra um disco de varejo real (2026-10-19); faltam 4 — chaves de debug/Chihiro, dump de base `0x18300000`, nomes não-ASCII e o significado das palavras extras do header

**Aceite:** ✅ `xbetool` e `isotool` descrevem corretamente fixtures **e um disco de varejo real**; `obsidian info` e `obsidian run` abrem a mídia, montam os drives do console e leem o executável; 105 testes (incluindo ~27 mil entradas mutadas) verdes. Pendente: 10 itens de conformidade que exigem arquivos que não temos (dev kit, dump completo, HD de Xbox, dump de EEPROM) — listados em [`docs/compat/formatos.md`](docs/compat/formatos.md).

---

### Fase 2 — Memória e barramento

> **Revisão arquitetural concluída antes da implementação:**
> [`docs/arch/memoria.md`](docs/arch/memoria.md). Ela mediu as premissas no hardware real
> e **mudou duas decisões**: fastmem sai da Fase 2 para a 3b (ADR-0010) e MMIO não será
> despachado por falha de página (custa 11,5 µs no Windows). Os ADRs 0009 a 0011
> formalizam o resultado.

- [x] Newtypes `GuestVirt`, `GuestPhys`, `ArenaOff` sem conversão implícita (ADR-0009)
- [x] Arena compartilhável de 64 MiB (+ feature `ram128`); posse exclusiva, **Send e não Sync**, sem Clone. Integração Vulkan/concorrência ficam para a Fase 5 (ADR-0020 revisa ADR-0011)
- [x] MMU do guest: páginas de 4 KiB e 4 MiB, permissões, dirty/accessed, `Falta` com endereço e código de erro; cache TLB adiado até medição
- [x] Janelas por tabela de páginas; alias contíguo `0x80000000 + PA` explícito na inspeção. O AOT não oferece alias RW para código
- [x] Classificação física + stubs e telemetria de acesso desconhecido
- [x] Caminho seguro; 16 blocos unsafe confinados à arena, auditados (em vez da previsão de dois)
- [x] Snapshot determinístico de arena, MMU e frames
- [x] `docs/arch/memoria-unsafe.md` com inventário, invariantes e evidência
- [x] T1–T10 sob Miri com arena pequena; T11/T12 com vetor independente e 20000 cenários nativos. Diferencial com fastmem aguarda Fase 3b
- [x] **T14 local:** 14 seções e 152 ordinais do NFSU2 pelo bus iguais ao arquivo; fixture no CI e mídia real atrás de `OBSIDIAN_MIDIA_REAL`
- [x] Benchmark seguro reproduzível em `xbox-mem/examples/safe_memory.rs`; resultados locais em `docs/dev/memoria.md`

**Fora de escopo por decisão:** fastmem (Fase 3b, ADR-0010), janela *tiled* (Fase 5 — é transformação de endereço, não espelho), dispositivos além de stub (Fases 5 e 6).

**Aceite local concluído em 2026-10-20:** contrato T1–T10 sob Miri sem diagnóstico
de UB, traço nativo T11/T12 e T14 com fixture/mídia real. FFI Linux e CI remoto
não foram executados neste host; GPU, fastmem e concorrência não estão cobertos.

---

### Fase 3 — CPU

#### 3-AOT — Recompilação estática em Rust *(prioridade atual, ADR-0013)*

- [x] *(2026-10-19)* Decoder x86-32 em Rust (`iced-x86`) e IR para MOV/ADD/SUB/XOR/CMP/TEST/NOP, JMP/JZ/JNZ, CALL/RET, com operandos de 32 bits
- [x] *(2026-10-19)* Descoberta por fluxo alcançável, rejeição de instruções sobrepostas, alvos inválidos e instruções sem suporte
- [x] *(2026-10-19)* Emissão de blocos Rust compiláveis, memória pelo bus e pilha explícita do guest
- [x] *(2026-10-19)* `xberecomp demo`/`translate`: geração de projeto Cargo e compilação/execução nativa opcional
- [x] *(2026-10-19)* Prova aberta: soma/loop/call/escrita retorna 62 após 37 instruções; teste compila e executa o Rust emitido
- [x] *(2026-10-19)* Snapshot CPU+bus retoma em fronteira de bloco, inclusive com CALL pendente
- [x] *(2026-10-19)* Operações inteiras de 8/16/32 bits e subregistradores; AND/OR, INC/DEC/NEG/NOT, MOVZX/MOVSX e LEA
- [x] *(2026-10-19)* PUSH/POP de 16/32 bits, LEAVE de 32 bits e as 16 condições Jcc curtas/near
- [x] *(2026-10-19)* Oráculo de ALU na CPU física: 7986 comparações no Rust traduzido; mais 3872 casos de Jcc (ADR-0014)
- [x] *(2026-10-19)* ADC/SBB de 8/16/32 bits, com carry/borrow entre blocos e snapshot; 9438 casos gerais de ALU + 262144 comparações exaustivas de ADC/SBB de byte contra a CPU física
- [x] *(2026-10-19)* SHL/SHR/SAR de 8/16/32 bits por CL/imediato/um implícito; flags definidas verificadas em 445686 comparações físicas e política determinística para flags indefinidas (ADR-0015)
- [x] *(2026-10-19)* ROL/ROR/RCL/RCR de 8/16/32 bits e SHLD/SHRD de 16/32 bits; 791764 comparações físicas; contagens de resultado indefinido recusadas com PC (ADR-0016)
- [x] *(2026-10-19)* MUL/IMUL e DIV/IDIV de 8/16/32 bits, IMUL truncado 16/32, CBW/CWDE/CWD/CDQ; 483916 comparações físicas e 1000580 diagnósticos de #DE classificados, sem entrega por IDT/kernel (ADR-0017)
- [x] *(2026-10-20)* JCXZ/JECXZ, LOOP/LOOPE/LOOPNE e CLD/STD; strings de 8/16/32 bits com endereços de 16/32 bits, REP limitado por elemento e snapshot parcial (ADR-0018); 3456 comparações físicas
- [x] *(2026-10-20)* CALL/JMP near de 32 bits por registrador/memória, descoberta conservadora e entradas adicionais `--entry`; validação de destinos antes de alterar CPU/pilha e RET validado (ADR-0019). Descoberta completa de tabelas arbitrárias continua pendente
- [ ] x87/MMX/SSE e validação diferencial independente
- [ ] Loader real: TLS, ordinais e ambiente XAPI (Fase 4)
- [ ] Perfis por revisão de XBE, identificação por hash e cache versionado
- [ ] Carregar módulo no frontend com erros recuperáveis e encerramento completo
- [ ] Validar homebrew nxdk e depois compatibilidade comercial, começando pelo candidato NFSU2

**Aceite inicial:** fixture criada pelo projeto traduzida, compilada e executada;
não equivale ao aceite completo da Fase 3 ou a um jogo funcionando. A estratégia
gráfica continua pendente de integração; a prova usa somente CPU e memória.

#### 3a — Interpretador *(marco crítico)*
- [ ] Decoder x86-32: prefixos, ModRM/SIB, grupo 686 completo
- [ ] ALU/branch/string ops com FLAGS *lazy*
- [ ] x87 completo com controle de precisão
- [ ] MMX
- [ ] SSE (sem SSE2) incluindo `MXCSR`
- [ ] Exceções: `#UD`, `#GP`, `#PF`, `#DE`, `#DB`; `int`/`iret` (mínimo necessário)
- [ ] **Oráculo KVM:** harness que compara estado final interpretador vs CPU real
- [ ] Testes de ISA (suítes públicas de teste de 386/686 + nossos XBEs de teste)

#### 3b — JIT dynarec *(marco crítico)*
- [ ] Descoberta de bloco básico e cache indexado por página física
- [ ] Tradução para Cranelift IR; FLAGS lazy preservadas na IR
- [ ] Alocação de registradores do guest nos do host
- [ ] *Block linking* / chaining direto, com *trampoline* de saída
- [ ] Fastmem sem bounds check + recuperação de fault dentro de código JIT
- [ ] Detecção de **código automodificável** por página suja + invalidação
- [ ] Modo diferencial interp↔JIT
- [ ] Bench: ≥ 10× o interpretador

#### 3c — Hypervisor *(experimental)*
- [ ] Trait `Hypervisor`; impl KVM (`kvm-ioctls`) no Linux
- [ ] Impl WHPX no Windows
- [ ] VM 32-bit flat: GDT/IDT/CR0/CR4/page tables montadas por nós
- [ ] Thunks do kernel em página armadilha → VM exit → HLE
- [ ] MMIO do NV2A/APU por exit
- [ ] Detecção de capacidade em runtime com fallback automático para JIT

**Aceite da fase:** um XBE homebrew que escreve padrões no framebuffer roda nos três backends com resultado idêntico.

---

### Fase 4 — Kernel HLE *(a fase mais longa)*

- [x] Carregamento de headers/seções/BSS e permissões pelo loader compartilhado; inspeção e política AOT (ADR-0020)
- [ ] TLS e resolução de thunks para trampolins; imports ainda não executam
- [x] Perfil AOT inicial: TLS nxdk, FS, trampolins guest RX e fronteira stdcall/cdecl/fastcall; serviços/dados/threads pendentes (ADR-0021)
- [ ] Mecanismo de trampolim (JIT/HV/interp) com ABI stdcall e logging de ordinal
- [ ] Object manager: arena de objetos indexada, handles, `OBJECT_ATTRIBUTES`, parsing de caminhos `\Device\...`
- [ ] **Scheduler de threads do guest** determinístico (prioridades, quantum, IRQL, APC, DPC)
- [ ] Primitivas de sincronização (event, semaphore, mutant, timer, critical section)
- [ ] Memory manager (grupo `Mm*`) + **heap real na memória do guest** (`RtlHeap`)
- [ ] I/O: `Nt*File` sobre a VFS; async/completions
- [ ] Grupo `Rtl*` (strings, tempo, bits, assert)
- [ ] `Av*` (modos de vídeo) ligado ao `xbox-gpu`
- [ ] `Ex*` ↔ EEPROM; `Hal*` (incluindo `HalReturnToFirmware` → voltar à UI)
- [ ] `Xc*` (SHA1, HMAC, RC4, DES)
- [ ] `Dbg*` → nosso log (visibilidade enorme para depurar jogos)
- [ ] `XNet*`/`XOnline*` como stubs que falham graciosamente
- [ ] **Telemetria de ordinal** + `tools/ordinal-report`

**Aceite:** os *samples* do nxdk (hello, input, audio, texture, 3D) rodam; relatório de ordinais mostra cobertura ≥ 90% dos ordinais efetivamente chamados pelos samples.

---

### Fase 5 — GPU NV2A *(a fase mais difícil)*

> **Pré-requisito de ambiente:** esta fase traz `wgpu`, que depende de `windows-sys`.
> O host de desenvolvimento atual não consegue linká-lo (ADR-0008). Antes de começar a
> Fase 5, instalar **MSVC Build Tools** ou **MinGW-w64 completo** — ver
> [`docs/dev/build.md`](docs/dev/build.md).

#### 5a — Infra
- [ ] Registradores MMIO: PMC, PBUS, PFIFO, PGRAPH, PCRTC, PRAMDAC, PTIMER, PRAMIN
- [x] *(2026-10-22)* Armazenamento PRAMIN, perfil independente provisório de 1 MiB, DWORD alinhado e snapshot BUS0 v5/PRAM v1; sem execução PFIFO/PGRAPH ou vínculo físico com RAM (ADR-0060/P14)
- [x] *(2026-10-22)* Página D3D de boot reservada antes da imagem, com posse física, BUS0 v6/BTPG v1 e replay de layouts antigos sem reserva (ADR-0061)
- [x] *(2026-10-23)* MmFreeContiguousMemory remove recursos CMEM comuns e devolve frames, preservando boot e backings referenciados (ADR-0062)
- [ ] IRQs da GPU para o guest (vblank, PGRAPH)
- [ ] DMA objects e tradução de contexto
- [ ] Parser de pushbuffer (GET/PUT, jump, call/ret, métodos inc/non-inc)
- [ ] Semáforos e notifiers (sincronização guest↔GPU) — **sem isso, nada sincroniza**
- [ ] Thread de GPU desacoplada da thread de CPU

#### 5b — State tracker (classe 3D)
- [ ] Viewport, scissor, clip planes
- [ ] Formatos e fontes de vértice: array, inline, inline-array, indexado
- [ ] Primitivas (incluindo quads e polygon, que o Vulkan não tem → conversão de índices)
- [ ] 4 unidades de textura com todo o estado de amostragem
- [ ] Depth/stencil/alpha/blend/logic ops; formatos de surface e tiling
- [ ] Register combiners (8 estágios + final combiner) no modelo de estado
- [ ] Modos de anti-alias e resolve

#### 5c — Shaders
- [ ] IR própria de shader + validador
- [ ] Vertex program NV2A → IR → SPIR-V
- [ ] Fixed-function T&L → IR → SPIR-V (luzes, materiais, fog, texgen, skinning)
- [ ] Register combiners → IR → SPIR-V
- [ ] Cache de shader em disco (hash de estado → SPIR-V)

#### 5d — Texturas
- [ ] Deswizzle (Morton) e caminho linear
- [ ] Formatos: ARGB/XRGB/RGB565/A1R5G5B5/A4R4G4B4/A8/L8/AL8, DXT1/3/5, paletizados, signed
- [ ] Cubemaps, mipmaps, texturas 3D
- [ ] Cache com invalidação por página suja

#### 5e — Surfaces e present
- [ ] Gerência de color/zeta na RAM unificada; render-to-texture
- [ ] Flip/scanout, modos 480i/480p/576i/720p/1080i
- [ ] Composição do overlay da UI sobre o frame do jogo
- [ ] *Frame pacing* e política de vsync

#### 5f — Backend
- [ ] Trait `GpuBackend` + implementação `wgpu`
- [ ] Ring buffers persistentes de VB/IB/uniform (**zero alocação por frame**)
- [ ] Pipeline cache; nenhuma compilação de shader durante gameplay (pré-aquecimento + cache)

**Aceite:** samples 3D do nxdk pixel-corretos (comparação por hash de frame); um jogo comercial simples chega ao menu principal a 60fps no hardware de referência.

---

### Fase 6 — Áudio (APU)

- [ ] Registradores do APU e DMA de buffers da RAM do guest
- [ ] Voice processor: 256 voices, PCM8/PCM16/ADPCM-Xbox, pitch/resample, envelope, loop
- [ ] 32 mixbins e submixagem; mixagem SIMD
- [ ] IRQs/notificações de buffer para o guest
- [ ] GP/EP: stub documentado + caminho de downmix PCM
- [ ] Backends: WASAPI (Windows), ALSA direto (Linux), `cpal` fallback
- [ ] Thread de tempo real + ring lock-free
- [ ] Modo "áudio como relógio mestre" com *dynamic rate control*

**Aceite:** sample de áudio do nxdk correto; um jogo com música+SFX sem crackle por 10 minutos; latência medida < 30 ms.

---

### Fase 7 — Input

- [ ] Modelo de dispositivo XID (Duke, Controller S, memory unit)
- [ ] HLE da camada USBD/XID + `XInput*` do XAPI
- [ ] 4 portas + hotplug
- [ ] Rumble
- [ ] Backend Windows (XInput + RawInput) e Linux (evdev/`gilrs`)
- [ ] Mapeamento configurável, deadzones, curvas, perfis por jogo
- [ ] Combinação **Home** → suspende emulação e abre a UI
- [ ] Memory units mapeadas para pastas do host
- [ ] Medição de latência input→display instrumentada

**Aceite:** 4 controles simultâneos, hotplug sem travar, rumble funcional, latência adicionada < 1 frame.

---

### Fase 8 — Frontend Big Picture

- [ ] Integração `egui` no device gráfico compartilhado
- [ ] Máquina de estados: Home / Biblioteca / Detalhe / Em jogo / Overlay
- [ ] Navegação 100% por gamepad com foco, som e animação
- [ ] Scanner de biblioteca + cache de metadados (nome e logo do XBE)
- [ ] Capas locais; placeholders gerados a partir do logo do XBE
- [ ] Suspender/retomar emulação em < 1 frame, sem recriar contexto gráfico
- [ ] Telas de configuração completas ([§7.10](#710-xbox-ui--big-picture))
- [ ] Primeira execução: wizard de pastas de jogos, região/idioma, teste de controle
- [ ] Modo quiosque (`--kiosk`): sem decoração, sem mouse, sem sair sem combinação

**Aceite:** instalar o binário numa máquina limpa, plugar um controle e jogar **sem tocar em teclado ou mouse**; UI a 60fps com < 5% de CPU no hardware de referência.

---

### Fase 9 — Precisão, estados e ferramentas

- [ ] **Save state** completo (memória + CPU + grafo de objetos do kernel + GPU + APU + input)
- [ ] *Quick resume* ao fechar/reabrir o emulador
- [ ] Rewind opcional (buffer circular de snapshots delta)
- [ ] GDB stub do guest
- [ ] Tracer/replay de pushbuffer (`pbreplay`) + visualizador de estado da GPU
- [ ] Profiler com overlay
- [ ] Replay determinístico de input (regressão estilo TAS): 60s reproduzem o mesmo hash de frame
- [ ] Matriz de compatibilidade automatizada em `docs/compat/`
- [ ] FATX com escrita (saves reais no HD virtual)
- [ ] Hardening: nenhum input malformado (XBE/ISO corrompido) causa UB ou crash não tratado — fuzz nos parsers

**Aceite:** save/load em 3 jogos diferentes com retomada perfeita; suíte de replay determinístico verde no CI.

---

### Fase 10 — Appliance Linux *(a meta final do projeto)*

**Objetivo:** o notebook antigo liga e **é** um console.

- [ ] Backend **DRM/KMS** com atomic modesetting: enumerar conectores, EDID, modos, page flip
- [ ] Apresentação Vulkan sem compositor — avaliar `VK_KHR_display` com **`ash`** (ver [4.6](#46-decisão-6-wgpu-agora-ash-provavelmente-depois)); se necessário, segundo backend de GPU
- [ ] *Seatless*: DRM master direto, sem logind; tratamento de VT e troca de terminal
- [ ] evdev puro (sem SDL), hotplug via netlink/udev opcional
- [ ] ALSA direto com thread `SCHED_FIFO` e `mlockall`
- [ ] Build `x86_64-unknown-linux-musl` **estático**; LTO `fat`, `codegen-units=1`, `panic` avaliado, strip
- [ ] PGO (e BOLT, se valer) com perfil coletado de gameplay real
- [ ] Tuning de sistema: huge pages, governor `performance`, `isolcpus` para a thread de CPU-guest, `nohz_full`
- [ ] Imagem bootável: kernel enxuto + initramfs + init mínimo que executa o binário
- [ ] Boot do power-on à Big Picture em **< 3 s** (meta); medir com `bootchart`
- [ ] Modo de recuperação e atualização (imagem A/B ou update de binário único verificado)
- [ ] Documentar o perfil de kernel (configs necessárias: DRM, evdev, ALSA, KVM, hugetlb)

**Aceite:** imagem que liga direto na UI, sem X11/Wayland, consumo de RAM do sistema + emulador medido e documentado, jogo rodando a 60fps.

---

### Fase 11 — Além

- [ ] Resolução interna escalável (2×/3×/4×) + filtro anisotrópico + widescreen hacks
- [ ] Shader pre-cache distribuível por título
- [ ] **System Link** (LAN) emulando o NIC nForce com túnel entre instâncias
- [ ] Suporte a Chihiro (arcade) — opcional
- [ ] Backend de CPU próprio (substituir Cranelift) se o perfil justificar
- [ ] Portabilidade ARM64 (SBC/handheld) — só faz sentido com JIT maduro
- [ ] Modo de captura: screenshot, vídeo, GIF

---

## 9. Estratégia de testes e qualidade

| Camada | Técnica | Onde |
|---|---|---|
| Parsers (XBE/XISO/FATX/EEPROM) | unitários + **fuzz** (`cargo-fuzz`) com corpus de fixtures geradas | `xbox-formats` |
| MMU / bus | unitários de tradução, acessos cruzando página, permissões | `xbox-mem` |
| CPU | **diferencial contra KVM** (CPU real como oráculo) + suítes públicas de teste de ISA + diff interp↔JIT | `xbox-cpu` |
| Kernel HLE | XBEs de teste escritos por nós com nxdk, um por família de ordinal, com asserts no guest | `tests/` |
| GPU | **hash de frame** contra referência; replay de traces de pushbuffer; comparação visual assistida | `tests/` + `pbreplay` |
| Áudio | comparação de buffer PCM gerado vs. referência; detector de descontinuidade | `xbox-audio` |
| Integração | replay determinístico de input: input gravado reproduz hash de frames | `tests/` |
| Performance | `criterion` em micro (decoder, mixer, deswizzle) + harness de FPS por cena | `crates/*/bench/` |
| Compatibilidade | execução manual registrada em `docs/compat/` com log de ordinais faltantes | docs |

**Regras de CI:** `fmt` e `clippy -D warnings` bloqueiam merge; testes unitários e de integração rápidos rodam em todo push; fuzz e suíte de GPU rodam em agenda noturna.

---

## 10. Orçamento de performance e metas

### 10.1 Perfis de hardware

| Tier | Descrição | Expectativa |
|---|---|---|
| **Dev** | x86_64 4+ núcleos ≥ 2.5 GHz, GPU Vulkan 1.2 | Tudo a 60fps com folga; é onde se desenvolve |
| **Alvo** | x86_64 2 núcleos ≥ 1.6 GHz **com VT-x/AMD-V**, iGPU Vulkan 1.1 | 60fps na maioria dos títulos 2D/3D leves; é o alvo do *appliance* |
| **Mínimo** | x86_64 2 núcleos sem virtualização | Depende do JIT; alguns títulos a 60fps |

### 10.2 Orçamento de um frame a 60 Hz (16.67 ms) no Tier Alvo

| Consumidor | Orçamento | Observação |
|---|---|---|
| CPU do guest (JIT) | ≤ 8.0 ms | Com HV, cai para ≤ 3 ms |
| Tradução de pushbuffer + state | ≤ 3.0 ms | Thread separada; o custo é paralelo, não serial |
| Submissão/driver do host | ≤ 2.0 ms | |
| Mixagem de áudio | ≤ 1.0 ms | Thread RT |
| UI/overlay | ≤ 0.5 ms | Quase nada durante o jogo |
| Folga | ≥ 2.0 ms | Absorver picos e *frame pacing* |

### 10.3 Metas numéricas rastreadas

- Alocações de heap por frame em gameplay: **0**
- Compilações de shader durante gameplay: **0** (após pré-aquecimento)
- Latência input→display adicionada: **< 1 frame**
- Latência de áudio: **< 30 ms**
- RAM residente do emulador (sem contar a arena de 64 MiB): **< 150 MB**
- Tempo UI → primeiro frame do jogo: **< 2 s**
- Boot do appliance (power-on → UI): **< 3 s**
- Tamanho do binário estático (musl, stripped): **< 25 MB**

Cada meta ganha um teste ou um medidor instrumentado — meta sem medição é desejo.

---

## 11. Riscos e mitigações

| # | Risco | Impacto | Mitigação |
|---|---|---|---|
| R1 | **Register combiners e vertex programs** traduzidos errado → gráficos sutilmente quebrados em muitos jogos | Alto | Validar contra samples do nxdk escritos para exercitar cada estágio; `pbreplay` para iteração rápida; comparação de frame por hash |
| R2 | Cobertura de ordinais do kernel vira poço sem fundo | Alto | Telemetria de ordinal → fila de trabalho priorizada por jogo real; stubs que falham graciosamente em vez de travar |
| R3 | **`wgpu` não cobre** estados do NV2A nem apresentação em KMS | Alto | Trait `GpuBackend` desde a Fase 5; backend `ash` planejado na Fase 10; nunca acoplar o tradutor ao wgpu |
| R4 | Backend de hypervisor com semântica de exceção difícil (SMC, page fault preciso) | Médio | HV é **experimental**; JIT é o caminho suportado; fallback automático em runtime |
| R5 | GP/EP (DSPs de áudio) mal documentados | Médio | Stub + downmix; documentar títulos afetados; implementar só se a lista crescer |
| R6 | Determinismo quebrado por multithread → save state e replay inviáveis | Alto | Decisão 4.4 aplicada desde a Fase 0; troca de contexto por contagem de ciclos; suíte de replay no CI |
| R7 | Escopo de um projeto solo | Alto | Homebrew antes de comercial; fases com aceite objetivo; nada de "melhorias" antes do aceite da fase |
| R8 | *Fastmem* + handler de sinal/VEH gerando bugs difíceis | Médio | `xbox-mem` isolado, com modo "slowmem" sempre disponível para bissecar bug |
| R9 | Drivers Vulkan antigos em iGPU velha (o hardware-alvo!) | Médio | Validar cedo no Tier Alvo real, não só no Dev; caminho de fallback de features |
| R10 | Perda de interesse / pausa longa no projeto | Médio | Documentação ([§12](#12-protocolo-de-documentação)) é o que permite retomar em 6 meses sem reaprender tudo |

---

## 12. Protocolo de documentação

> Combinado com o objetivo declarado: **a cada melhoria, documentamos.** Isto é o processo.

### 12.1 Onde cada coisa é documentada

| Tipo de mudança | Documento |
|---|---|
| Qualquer mudança visível | `CHANGELOG.md` |
| Item de fase concluído | checkbox marcado neste `ROADMAP.md` + linha no [§17](#17-log-de-progresso) |
| Decisão arquitetural | novo ADR em `docs/adr/NNNN-titulo.md` |
| Como um subsistema funciona | `docs/arch/<subsistema>.md` |
| Jogo testado | `docs/compat/<title-id>.md` |
| Processo de build/debug | `docs/dev/*.md` |

### 12.2 Checklist obrigatório por melhoria

1. Código + **teste** que prova que funciona.
2. Entrada no `CHANGELOG.md` (seção `[Não publicado]`).
3. Se mudou arquitetura ou houve escolha entre alternativas → **ADR**.
4. Se mudou o comportamento de um subsistema → atualizar `docs/arch/<subsistema>.md`.
5. Marcar o checkbox correspondente no ROADMAP com a data.
6. Se afetou performance → rodar o bench e anotar o número **antes e depois**.
7. Commit convencional.

### 12.3 Formato de commit

```
<tipo>(<escopo>): <resumo imperativo>

<corpo: o que, por quê, e o que ficou de fora>

Refs: ROADMAP Fase 5c · ADR-0012
```

`tipo` ∈ `feat` `fix` `perf` `refactor` `docs` `test` `build` `chore`
`escopo` ∈ `core` `formats` `mem` `cpu` `jit` `hv` `kernel` `gpu` `shader` `audio` `input` `platform` `ui` `dbg` `tools` `ci`

### 12.4 Template de ADR

```markdown
# ADR-NNNN: <título>

- **Status:** proposto | aceito | substituído por ADR-MMMM
- **Data:** AAAA-MM-DD
- **Fase:** <fase do roadmap>

## Contexto
Que problema real forçou essa decisão.

## Alternativas consideradas
1. <A> — prós / contras / custo
2. <B> — prós / contras / custo

## Decisão
O que foi escolhido, em uma frase.

## Consequências
- Positivas:
- Negativas / dívida aceita:
- O que isso impede no futuro:

## Como reverter
Qual seria o custo de voltar atrás.
```

### 12.5 Regra de ouro da documentação

**Se a informação só existe na sua cabeça, ela não existe.** Decisão tomada em 10 minutos de raciocínio que não virou ADR custa 3 horas para reconstruir em seis meses.

---

## 13. Convenções de código, build e features

### 13.1 Perfis de build

```toml
[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
strip = true
debug = false

[profile.release-dbg]      # perfilar com símbolos
inherits = "release"
debug = true
strip = false
lto = "thin"               # build de perfilagem não precisa esperar LTO fat

[profile.dev]
opt-level = 1              # emulador em debug puro é inusável
[profile.dev.package."*"]
opt-level = 3              # dependências sempre otimizadas
```

> `panic`: manter `unwind` enquanto o fastmem depende de recuperação de fault; reavaliar `abort` na Fase 10 com medição de tamanho/velocidade (**ADR obrigatório**).

### 13.2 Features do workspace

| Feature | Efeito | Default |
|---|---|---|
| `jit` | backend dynarec | ✅ |
| `hv` | backend KVM/WHPX | ❌ (experimental) |
| `ram128` | 128 MB de RAM (dev kit) | ❌ |
| `kms` | backend DRM/KMS (Linux appliance) | ❌ |
| `vk-direct` | backend `ash` em vez de `wgpu` | ❌ |
| `trace-verbose` | tracing de alta granularidade | ❌ (custo em release) |
| `dbg-gdb` | GDB stub | ❌ |

### 13.3 Regras de código

- `unsafe` **só** nos crates listados na Fase 0, sempre com comentário `// SAFETY:` explicando a invariante.
- Nenhum `unwrap()`/`expect()` em caminho que processa dado do usuário (XBE, ISO, config) — erro tipado, sempre.
- Hot path: sem alocação, sem `dyn` em laço interno (despacho estático ou tabela de função), sem formatação de string.
- Nomes de registradores/métodos de hardware seguem a nomenclatura pública da documentação do NV2A (`NV097_SET_*`), para que o código seja pesquisável contra as fontes.
- Toda constante de hardware vive em um módulo `regs`/`consts` com comentário citando a fonte.

---

## 14. Legal e ética

- O emulador é **reimplementação limpa**: escrito a partir de documentação pública de hardware, engenharia reversa de comportamento observável e código aberto licenciado compativelmente. **Nenhum código ou binário da Microsoft é copiado, embutido ou distribuído.**
- **Nada proprietário no repositório:** sem BIOS, sem kernel, sem bibliotecas, sem jogos, sem fixtures extraídas de mídia comercial. As fixtures de teste são compiladas por nós com o **nxdk** (SDK aberto).
- O usuário usa **dumps das mídias que ele possui**. O projeto não fornece, não indexa e não facilita obtenção de conteúdo protegido.
- Antes de usar qualquer código de terceiros, verificar licença e compatibilidade, e registrar em `docs/dev/third-party.md` (crate, versão, licença, para quê).
- Documentação consultada é **referência de fato técnico**, não fonte de código: não copiar trechos de outros emuladores sem respeitar a licença deles — e registrar em ADR quando um algoritmo for inspirado em projeto específico.

---

## 15. Referências de estudo

Fontes técnicas públicas que sustentam a implementação (consultar, citar nos comentários de código, não copiar código sem checar licença):

- **XboxDevWiki** — mapa de memória, registradores do NV2A, ordinais do kernel, formatos XBE/XDVDFS/FATX, APU.
- **nxdk** (Xbox dev kit aberto) — como um XBE real é construído e linkado; base das nossas fixtures e testes.
- **xemu / XQEMU** — referência de comportamento do NV2A e do APU em nível de registrador (LLE).
- **Cxbx-Reloaded** — referência de semântica dos ordinais do kernel (HLE).
- Documentação pública do NV20/NV2A: *register combiners* e *vertex programs* (extensões `NV_register_combiners`, `NV_vertex_program`).
- Manuais da Intel (SDM) — semântica exata de x86-32, x87, MMX, SSE.
- Documentação do Vulkan e do SPIR-V; `wgpu`/`ash`.
- Documentação do KVM API e do Windows Hypervisor Platform.
- `drm`/KMS: documentação do kernel Linux sobre atomic modesetting.

---

## 16. Glossário

| Termo | Significado |
|---|---|
| **HLE** | *High-Level Emulation* — reimplementar uma API/biblioteca em código nativo em vez de emular o hardware por baixo |
| **LLE** | *Low-Level Emulation* — emular o hardware e deixar o software original rodar sem modificação |
| **XBE** | *Xbox Executable* — formato de executável do Xbox (PE modificado) |
| **XDVDFS / XISO** | Filesystem do DVD do Xbox / imagem desse disco |
| **FATX** | Filesystem do HD e dos memory units |
| **NV2A** | GPU do Xbox (NVIDIA, híbrido GeForce3/4) |
| **Pushbuffer** | Fila DMA de comandos que a CPU escreve e a GPU consome |
| **PFIFO / PGRAPH / PCRTC** | Engines internas do NV2A: fila de comandos / engine 3D / scanout |
| **Register combiners** | Unidade de combinação de cores programável por configuração (o "pixel shader" da era DX8) |
| **Vertex program** | Shader de vértice da era DX8 (VS 1.1-like) |
| **APU / VP / GP / EP** | Áudio: unidade de processamento / processador de voz / DSP geral / DSP de encode |
| **XID** | Classe USB dos controles do Xbox |
| **Ordinal** | Índice numérico com que um XBE importa funções do kernel |
| **Thunk table** | Tabela no XBE que o loader preenche com endereços das funções do kernel |
| **Dynarec / JIT** | Recompilação dinâmica do código do guest para código nativo do host |
| **Fastmem** | Usar a MMU do host para que acessos do guest sejam acessos diretos de memória |
| **KMS/DRM** | Interface do kernel Linux para modo de vídeo e GPU, sem servidor gráfico |
| **Appliance** | Modo final: o emulador como único software gráfico do sistema |

---

## 16.1 Soluções provisórias declaradas

> **Para que serve esta tabela.** Uma solução provisória só é honesta enquanto estiver
> marcada como provisória. O risco não é a solução existir — é ela **virar definitiva por
> omissão**, porque funciona para o alvo atual e ninguém lembra por que foi aceita. Esta
> lista contém apenas o que foi aceito *conscientemente*, com a **condição de saída**
> verificável. Não é o backlog das fases (isso é a §8) e não deve receber pendências
> comuns. Ao fechar um item, mova-o para o log de progresso com o commit.

| # | Provisório | Por que é aceitável hoje | O que quebra se virar definitivo | Condição de saída |
|---|---|---|---|---|
| P1 | **Janela MMIO do NV2A sem modelo** (ADR-0051): escritas descartadas, leituras devolvem zero declarado | a telemetria registra todo acesso; o hello só *escreve* a rampa de gamma e nunca lê | título que leia status de registrador (`PCIO_CRTC_STATUS`, vblank) gira para sempre; título que dependa do efeito de uma escrita vê comportamento errado **sem erro** | cada registrador que a telemetria listar como **lido**, ou cujo efeito afete saída, ganha modelo antes de a Fase 5 fechar; o relatório vira teste de regressão |
| P2 | **Gamma e filtros do encoder não aplicados na apresentação** (ADR-0052, ADR-0050) | o hello configura gamma com identidade e nunca a lê; flicker/soften são só pedidos lógicos | fade por gamma (comum) aparece sem fade; a janela passa a ser tomada por "a saída do console" | modelar o DAC (índice com autoincremento, 3 componentes, snapshot) e aplicá-lo na apresentação; decidir sobre os filtros |
| P3 | **`KeDelayExecutionThread` avança o relógio** (ADR-0051) em vez de bloquear a thread | há uma única thread executável; esperar `T` e avançar `T` são a mesma coisa observável | com mais de uma thread executável, o tempo avança sem que as outras corram | escalonador de threads: a espera passa a **bloquear** a thread, e o avanço de tempo vira decisão do escalonador |
| P4 | **Nome do arquivo e caminho de `XeImageFileName` declarados**, não derivados de mídia (ADR-0022) | o hello só chama `strncpy` do caminho para um buffer | título que abre arquivo relativo ao próprio XBE lê o caminho errado | ligar a VFS ao kernel e derivar o caminho da mídia carregada |
| P5 | **Raízes de tradução declaradas à mão** (`--entry`, nove hoje) | análise conservadora (ADR-0019) e o hello tem poucos destinos indiretos | cada título novo exige caçar raízes por tentativa e erro; sem cobertura, o erro só aparece **em execução** | resolver jump table e tabelas de ponteiros na análise, provando que o limite protege o salto |
| P6 | **Apresentação só no Windows**, e só `--janela` opt-in | o alvo de desenvolvimento é Windows e o nxdk hello é o único alvo | o alvo final (appliance Linux, Fase 10) não tem janela alguma | backend Linux desktop (Fase 8) e DRM/KMS (Fase 10) atrás do mesmo `Apresentador` |
| P7 | **`KeTickCount` só atualiza em fronteiras do host** (serviço de espera e fim de fatia), não a cada instrução | nenhum alvo testado lê o contador sem chamar o kernel; o triangle do nxdk o importa | um laço do guest que faça polling de `KeTickCount` sem nenhuma chamada de kernel o vê parado até o fim da fatia (~1,4 ms guest) e só então avança | atualização acoplada ao avanço do relógio (ou interrupção de timer, junto do vblank) |
| P8 | **SSE sem MXCSR dinâmico**: flags de exceção nunca atualizadas, todas as exceções mascaradas, arredondamento sempre ao mais próximo, sem FTZ/DAZ; `LDMXCSR`/`STMXCSR` recusados (ADR-0053) | nenhum alvo testado alterou o MXCSR | um título que mude o modo de arredondamento ou leia as flags obtém resultado errado sem erro | modelar o MXCSR por completo quando um alvo real o usar |
| P9 | **x87 só carga e armazenamento**: sem aritmética, sem palavras de controle/estado nem exceções (ADR-0053) | o `triangle` usa `fild`/`fstp`/`fldz`; aritmética ainda não foi alcançada | instruções x87 além das listadas são recusadas na análise, com o nome | implementar a aritmética quando um caminho executado a alcançar, com a precisão de 80 bits já exata na pilha |
| P10 | **Interrupção com convenções declaradas**: vetor `0x30 + nível`, `KIRQL = 27 − nível`, ISR com IRQL inalterado (ADR-0055) | nenhum alvo lê o número; o perfil só tem PASSIVE/APC/DISPATCH | um título que leia o vetor ou o IRQL dentro do ISR vê valores convencionais | DIRQL no modelo de IRQL |
| P11 | **Entrega de interrupção só nas fronteiras de `conduzir`**, sem preempção e sem consultar IF (ADR-0055) | o `pbkit` espera por evento e tolera ~1 fatia de latência | código que dependa de interrupção no meio de um bloco ou de `CLI`/`STI` como máscara | modelar IF, `STI` e entrega com preempção |
| P12 | **NV2A parcial:** vblank, NVPLL, PFB_CFG0/CFG1 e PRAMIN com perfis explícitos (ADRs 0055/0057/0058/0060); demais offsets sem modelo | recortes guiados pelo guest, fontes públicas fixadas; perfil PFB distingue 64/128 MiB | readback não implementa geometria elétrica ou tiling; faltam PFIFO e PGRAPH; vínculo físico PRAMIN/RAM limitado pelo P14; consulta de instância limitada pelo P13; PTIMER só registra escritas | modelo do NV2A (Fase 5), com evidência de cada valor |
| P13 | **Bootstrap de instância GPU na primeira consulta** (ADR-0059): reserva CMEM dos 128 KiB finais, instância + padding, somente retail 64 MiB | pbkit consulta 0xFFFFFFFF; retorno tem backing real e posse de frames; repetição preserva bytes | boot não reserva previamente; colisão retorna NULL; não simula PFN database, resize ou devkit; não define backing de PRAMIN; MmFreeContiguousMemory recusa o pool provisionado (ADR-0062) | integrar reserva ao boot/firmware e estabelecer posse/backing PRAMIN, teardown e perfis maiores com evidência |
| P14 | **PRAMIN independente** (ADR-0060): armazenamento de 1 MiB no dispositivo, sem alias RAM/CMEM | perfil público xemu e uso real de RAMHT/contextos pelo pbkit; preserva bytes e snapshot | não representa a relação física com a RAM UMA ou reserva de instância do boot; ainda sem consumo PFIFO/PGRAPH | evidência do vínculo físico PRAMIN/RAM e integração ao boot/reserva, com migração explícita do perfil |

Investigação de evidência e ferramentas em 2026-10-22:
[ADR-0056](docs/adr/0056-evidencia-nv2a-e-reproducao.md). O PLL lembrado foi
localizado em revisão pública fixada do xemu e confrontado com o consumidor nxdk;
isso documenta um perfil de emulador, não medição de console. Nenhum valor novo
foi adotado no runtime; P1/P5/P12 permanecem abertos. Scripts de build/raízes
preservam manifesto, tentativas e logs para reproduzir a próxima entrega.

## 17. Log de progresso

Uma linha por marco concluído. Append-only.

| Data | Fase | Marco | Commit/PR | Notas |
|---|---|---|---|---|
| 2026-10-19 | — | Roadmap inicial escrito | — | Arquitetura definida; etapa A (desktop) antes da etapa B (appliance) |
| 2026-10-19 | — | §4.6.1 esclarece o papel do Vulkan | — | Vulkan é a API desde a Fase 5 nos dois SOs; Fase 10 troca só a apresentação |
| 2026-10-19 | 0 | **Fase 0 concluída** | — | Workspace (16 pacotes), `xbox-core` com snapshot/relógio/config, CI, ADRs 0001–0008. 30 testes + 1 doctest |
| 2026-10-19 | 0 | ADR-0008: contorno de toolchain no Windows | — | `windows-sys` não linka no host; features de cor desligadas e bench isolado. Tem prazo: resolver antes da Fase 5 |
| 2026-10-19 | 0 | Arredondamento assimétrico no relógio | — | Duração pedida arredonda para cima (timer nunca vence cedo); tempo decorrido arredonda para baixo |
| 2026-10-19 | 1 | Parser de XBE + `xbetool` | — | Desofuscação por plausibilidade; logo em ASCII para verificação visual |
| 2026-10-19 | 1 | Leitor de XDVDFS + `isotool` | — | Detecção de base da partição; caminhamento com terminação garantida |
| 2026-10-19 | 1 | Fixtures por código + testes de robustez | — | ~24 mil mutações determinísticas sem pânico; `docs/compat/formatos.md` lista o que ainda precisa de arquivo real |
| 2026-10-19 | 1 | **Validação contra disco de varejo real** | — | 9 itens de conformidade fechados; chaves XOR retail confirmadas (thunk caiu no início de `.rdata`); logo decodificado legível |
| 2026-10-19 | 1 | Bug: TLS perdido em XBE real | — | O TLS fica dentro de `.rdata`, não nos headers; a tradução de VA agora olha as seções |
| 2026-10-19 | 1 | Leitura sob demanda (`media.rs`) | — | `fs::read` de 2,87 GB era inviável; `isotool info` passou a rodar em 0,4 s |
| 2026-10-19 | 1 | 152 ordinais do kernel inventariados | — | Lista real de funções que um jogo comercial exige — entrada direta para a Fase 4 |
| 2026-10-19 | 1 | FATX, EEPROM e VFS | — | **Fase 1 concluída.** Travessia de diretório bloqueada em duas camadas; partições graváveis são pasta do host por decisão de projeto |
| 2026-10-19 | 1 | `obsidian info` e `run` funcionais | — | O binário principal abre a mídia real, monta D:/T:/U:/Z: e lê o executável; execução falha citando as Fases 3 a 5 |
| 2026-10-19 | 2 | **Revisão arquitetural de memória** | — | `docs/arch/memoria.md`: mediu as premissas no host real e mudou duas decisões (ADR-0010) |
| 2026-10-19 | 2 | Fastmem rende 3,4x em cache e 1,04x em RAM | — | Medido; a conclusão do ADR-0005 não se sustentava como escrita |
| 2026-10-19 | 2 | Falha de pagina custa 11,5 us no Windows | — | Mata MMIO por falha de pagina; proteção de página fica para eventos raros |
| 2026-10-19 | 1 | Sandbox da VFS endurecido | — | Fuga por link simbólico/junction fechada e **verificada** com junction neste host (ADR-0012) |
| 2026-10-19 | 0 | Win32 sem `windows-sys` | — | Declaração à mão linka no toolchain atual; a dívida do ADR-0008 só vale para a Fase 5 |
| 2026-10-19 | 3-AOT | Primeira recompilação nativa em Rust (ADR-0013) | — | `xberecomp`: XBE de teste → IR → Rust → executável; EAX=62 em 37 instruções; estado e call pendente preserváveis por snapshot |
| 2026-10-19 | 3-AOT | Subconjunto inteiro ampliado e oráculo físico (ADR-0014) | — | Operandos de 8/16/32 bits, subregistradores, pilha e Jcc; 7986 comparações de ALU + 3872 de branches no código gerado |
| 2026-10-19 | 0 | Preparação do repositório GitHub e licença Apache-2.0 | — | Histórico inicial de LICENSE preservado; README e metadados Cargo; mídias, BIOS e artefatos locais ignorados |
| 2026-10-19 | 3-AOT | ADC/SBB e carry preservado entre blocos | — | 8/16/32 bits, AH, memória, imediatos e snapshot; 9438 casos gerais de ALU + 262144 exaustivos de ADC/SBB de byte contra a CPU física; 18 testes nativos |
| 2026-10-19 | 3-AOT | SHL/SHR/SAR e política de flags (ADR-0015) | — | Contagens mascaradas, CL capturado antes da escrita, flags indefinidas normalizadas; 445686 comparações físicas de shifts; 177 testes de workspace e 24 nativos |
| 2026-10-19 | 3-AOT | Rotações e shifts duplos (ADR-0016) | — | Anel com carry, aliases de CL/fonte, memória e contagens indefinidas com diagnóstico; 791764 comparações físicas adicionais; 179 testes de workspace e 35 nativos |
| 2026-10-19 | 3-AOT | MUL/IMUL e DIV/IDIV com diagnóstico de #DE (ADR-0017) | — | 483916 comparações físicas e 1000580 diagnósticos classificados; extensões de sinal, memória, aliases e snapshot; 180 testes de workspace e 45 nativos |
| 2026-10-20 | 3-AOT | Loops por contador e strings REP retomáveis (ADR-0018) | — | 3456 comparações físicas, address16, DF, faltas e snapshots parciais; bus evita escrita parcial; 183 testes de workspace e 55 nativos em debug/release |
| 2026-10-20 | 3-AOT | Controle indireto e entradas adicionais (ADR-0019) | — | CALL/JMP r/m32, inferência local/RO e `--entry`; destinos validados antes de commit, RET, callbacks e snapshot; 189 testes de workspace e 64 nativos em debug/release |
| 2026-10-20 | 2 / 4 | Aceite local de memória e loader compartilhado (ADR-0020) | — | T14 NFSU2: 14 seções, 152 ordinais; Miri T1–T10, 20000 cenários, auditoria unsafe e Clippy global limpo; marco 1/6 do alpha |
| 2026-10-20 | 3-AOT / 4 | Build nxdk real e infraestrutura TLS/ABI (ADR-0021) | — | Hello: 4 seções, 72 imports; TLS nxdk, FS e trampolins guest RX; 69 testes nativos; serviços/dados/threads e execução do homebrew pendentes |
| 2026-10-20 | 4 | Dados exportados do kernel (ADR-0022) | — | Três dados em duas páginas com permissões distintas; slot de dado recebe endereço do símbolo; procedimentos envenenados; 10 testes de layout/permissão/conteúdo; `translate` do hello avança do ordinal 164 para `LOCK CMPXCHG` em 0x209C3 |
| 2026-10-20 | 4 | Dispatcher de serviços de kernel (ADR-0023) | — | Recusa com nome/ordinal/chamador/argumentos e CPU intacta; `RtlInitAnsiString` como primeiro serviço; laço de despacho no projeto gerado; 71 casos nativos; 1/69 serviços — bloqueio do alvo segue sendo ISA |
| 2026-10-20 | 3-AOT | CMPXCHG/LOCK em RAM e SETcc (ADR-0024) | — | 147456 comparações físicas; 79 casos nativos; aliases, FS/SIB, proteção e snapshot; próximo bloqueio real: RDTSC em 0x2055B |
| 2026-10-20 | 3-AOT | Clock determinístico e RDTSC (ADR-0025) | — | Reutiliza Clock do core; custo sintético explícito, REP por elemento e AOTC v4; 84 casos nativos; próximo bloqueio real: CLI em 0x1409E |
| 2026-10-20 | 3-AOT | CLI guest e parada explícita por HLT (ADR-0026) | — | IF, continuação, polls sem efeitos e diagnóstico no main; AOTC v5 preserva parada; 88 casos nativos; próximo bloqueio real: INT3 em 0x20D6C, preenchimento após CALL |
| 2026-10-20 | 3-AOT | INT3 e fronteira #BP guest (ADR-0027) | — | Exceção persistente, EIP seguinte, descoberta terminal e diagnóstico; AOTC v6 e 92 casos nativos; próximo bloqueio real: SFENCE em 0x12A43 |
| 2026-10-20 | 3-AOT | SFENCE e ordenação do bus (ADR-0028) | — | Perfil síncrono exclusivo sem filas a drenar; 96 casos nativos e contrato de alias/físico/DMA sob Miri; AOTC continua v6; próximo bloqueio real: CMOVNE em 0x13B88 |
| 2026-10-20 | 3-AOT | CMOVcc de 16/32 bits (ADR-0029) | — | 16 condições, origem lida mesmo se falsa; aliases/FS/SIB/SP/ESP, faltas e snapshots; 32768 comparações físicas e 103 testes nativos; próximo bloqueio real: BSR em 0x1657C |
| 2026-10-20 | 3-AOT | BSF/BSR de 16/32 bits (ADR-0030) | — | ZF e política de indefinidos; 262144 comparações físicas; fluxo descoberto do hello traduz com 8725 instruções/blocos |
| 2026-10-20 | 3-AOT / 4 | Pilha host limitada e primeira execução nativa do hello (ADR-0031) | — | Funções por bloco, limite de 32 instruções, regressão em 128 KiB; 111 casos nativos; com --entry 0x19110, 8769 blocos executam até NtCreateMutant (192), retorno 0x18CFA |
| 2026-10-20 | 4 | NtCreateMutant e snapshot integral da sessão (ADR-0032) | — | Mutexes sem nome com handles/propriedade guest, NTSTATUS e limites; Session possui kernel, snapshot AOTS/KRNL/KMUT; 271 testes workspace/doctests e 115 nativos debug/release; hello passa por criação e para em XcSHAInit (335), retorno 0x19140; 2/69 serviços com variantes limitadas |
| 2026-10-20 | 4 | SHA-1 em contexto opaco guest (ADR-0033) | — | Init/Update/Final em Rust; estado HLE v1 de 116 bytes na RAM, cópia/snapshot e validação sem stores parciais; 288 testes workspace/doctests e 118 nativos debug/release; hello passa por SHAInit e para em ExQueryNonVolatileSetting (24), retorno 0x19176; 5/69 serviços |
| 2026-10-20 | 4 | Consulta integral da EEPROM da sessão (ADR-0034) | — | Bytes/origem explícitos, perfil sintético existente, validação de todas as saídas; KRNL v2/EEPR v1 com migração v1; 301 testes workspace/doctests e 121 nativos debug/release; hello passa pela consulta e para em NtWaitForSingleObject (233), retorno 0x16B99, handle 4; 6/69 serviços |
| 2026-10-20 | 4 | Espera/liberação de mutant sem scheduler (ADR-0035) | — | Aquisição, recursão e liberação reais; `Alertable`, `Timeout`, outra thread e `PreviousCount` recusados como variantes; KMUT v2 com migração da v1; 8/69 serviços |
| 2026-10-20 | 3-AOT | CLC/STC/CMC e raiz do construtor global (ADR-0036) | — | Três flag ops com teste que compara EFLAGS inteiro; entram no braço que não invalida o rastreamento de destinos; `--entry 0x145E0` cobre a tabela de inicializadores do CRT; 8769 → 8791 blocos |
| 2026-10-20 | 4 / 5 | Hello avança até seções críticas | — | 308 testes workspace/doctests e 122 nativos debug/release; o binário nativo passa por mutex, SHA, EEPROM e construtores globais e para em `RtlInitializeCriticalSection` (291), objeto `0x2810C` |
| 2026-10-20 | 4 / 3-AOT | Seções críticas guest e retomada Linux (ADR-0037) | — | Init/Enter/Leave/TryEnter, 28 bytes em RAM, recursão e snapshot sem estado host; 12/69 serviços, 317 testes workspace/doctests e 125 nativos debug/release; hello passa por Init, chega ao construtor 0x205E0; essa raiz revela RDMSR em 0x207C6 |
| 2026-10-20 | 3-AOT / 5 | RDMSR e perfil de CPU guest (ADR-0038) | — | TSC compartilhado com RDTSC, EBL_CR_POWERON nominal, erros sem efeitos e AOTC v7 com migração; 322 testes workspace/doctests e 131 nativos debug/release; quatro raízes traduzem 9027 blocos e o hello chega a HalReadWritePCISpace (46) |
| 2026-10-20 | 4 / 5 | Leitura PCI do clock guest (ADR-0039) | — | PLL 0:0:3 e subleituras, configuração compartilhada com CPU/MSRs, KRNL v3/PCIC v1 com migração; 331 testes workspace/doctests e 135 nativos debug/release; hello calcula 733333333 Hz e chega a InterlockedExchange (54); 13/69 serviços |
| 2026-10-20 | 4 / 5 | InterlockedExchange guest (ADR-0040) | — | FASTCALL/8, troca LONG/retorno anterior sob Bus exclusivo, sem formato novo; 336 testes workspace/doctests e 140 nativos debug/release; efeito conferido no XBE real, que avança até KeQuerySystemTime (128); 14/69 serviços |

| 2026-10-20 | 4 / 5 | Tempo de sistema guest (ADR-0041) | — | KeQuerySystemTime deriva 100 ns do Clock único e referência UTC explícita; KRNL v4/STIM v1 com migração; 342 testes workspace/doctests e 145 nativos debug/release; saída real conferida aos 704 ciclos, hello chega a KeGetCurrentIrql (103); 15/69 serviços |

| 2026-10-20 | 4 / 5 | Consulta de IRQL guest (ADR-0042) | — | KeGetCurrentIrql lê prioridade explícita da única CPU, independente de IF; KRNL v5/IRQL v1 com migração; 346 testes workspace/doctests e 148 nativos debug/release; hello consulta Passive com IF=0 e chega a NtAllocateVirtualMemory (184), pedido de 64 KiB; 16/69 serviços |

| 2026-10-20 | 4 / 5 | Reserva+commit privado guest (ADR-0043) | — | NtAllocateVirtualMemory, frames próprios zerados, first-fit após imagem completa e publicação planejada; KRNL v6/VMEM v1 com validação de Bus/MMU/bitmap; 354 testes workspace/doctests e 153 nativos debug/release; hello usa 64 KiB em 0x30000 e chega a PsCreateSystemThreadEx (255); 17/69 serviços |
| 2026-10-21 | 3-AOT / 4 | NOP multibyte e raízes de startup da thread (ADR-0044) | — | NOP r/m16/r/m32 sem acesso ao operando; seis raízes/9454 blocos compilam e executam até PsCreateSystemThreadEx, ainda sem modelo; 357 testes workspace/doctests e 156 nativos debug/release; permanece 17/69 serviços |
| 2026-10-21 | 4 / alpha | Bootstrap cooperativo de filha guest (ADR-0045) | — | Criação/identificação/término, pilha/TLS/TIB e prefixo KTHREAD próprios, continuação do pai e Clock único; propriedade por thread e espera em filha terminada; AOTS v2/KRNL v7/KMUT v3/THRD+TCXT v1; 365 testes workspace/doctests e 162 nativos debug/release; oito raízes/10444 blocos, próximo JMP 0x14E86 → 0x14F14; 20/69 serviços |
| 2026-10-21 | 4 / alpha | Namespace guest D: e chegada ao vídeo (ADR-0046) | — | Open/create/close de links, consulta de ausência, handles compartilhados, alvo copiado; KRNL v8/SYML v1; 372 testes workspace/doctests e 165 nativos debug/release; nove raízes/10676 blocos, D: confirmado no hello, próxima chamada AvSendTVEncoderOption (2), retorno 0x12769; 23/69 serviços |
| 2026-10-21 | 4 / alpha | Encoder e perfil de vídeo cold boot (ADR-0047) | — | GET_SETTINGS derivado da EEPROM/cabo padrão, VIDEOENABLE com estado de blanking e ausência explícita de framebuffer herdado; KRNL v9/AVST v1; hello pede 1.228.800 bytes para 640×480×32 em MmAllocateContiguousMemoryEx (166), retorno 0x12C01; 380 testes workspace/doctests e 168 nativos debug/release; 25/69 serviços |
| 2026-10-21 | 4 / alpha | Framebuffer em memória física contígua (ADR-0048) | — | MmAllocateContiguousMemoryEx, frames vizinhos reais, janela guest restrita RW e coerência DMA; KRNL v10/CMEM v1; hello aloca/zera 300 páginas e chega a AvSetDisplayMode (3), retorno 0x12CA4; 388 testes workspace/doctests e 171 nativos debug/release; 26/69 serviços |
| 2026-10-21 | 4 / alpha | Modo de vídeo e leitura do framebuffer (ADR-0049) | — | AvSetDisplayMode configura NTSC-M 640×480/60 A8R8G8B8 sobre CMEM; visão BGRA por DMA sem compartilhar arena; AVST v2 migra v1; hello para na opção 11 do encoder, retorno 0x12DFA, ainda sem apresentação; 394 testes workspace/doctests e 174 nativos debug/release; 27/69 serviços |
| 2026-10-21 | 4 / alpha | Pedidos de filtros do encoder (ADR-0050) | — | Flicker 0..7/soften 0..1, estado opcional e recusa explícita; AVST v3 migra v1/v2 sem pedidos; hello passa por 5/false e para na escrita do DAC gamma em 0xFD6813C8, PC 0x12EEB, ainda sem mapeamento/modelo; 398 testes workspace/doctests e 177 nativos debug/release; mantém 27/69 serviços |
| 2026-10-21 | 4 / alpha | **Hello executa o `main` e desenha "Hello nxdk!"** (ADR-0051) | — | Janela MMIO do NV2A como identidade (só mapeamento; telemetria vê a rampa de gamma: 1 escrita de índice + 768 de dados) e `KeDelayExecutionThread` com `Clock::hundred_ns_to_cycles` verificada; a sessão aplica o avanço antes do retorno, `TimeExhausted` deixa a CPU intacta; relógio guest 8023 ms = 4×2000 ms + custo de instruções; 1208 pixels legíveis em ASCII; 420 testes workspace/doctests e 181 nativos; **28/69 serviços**; apresentação em janela pendente |
| 2026-10-21 | 4 / alpha | **Framebuffer apresentado em janela Win32** (ADR-0052) | — | `Apresentador` em `xbox-platform` (Win32 declarado à mão, sem dependência nova); `Quadro` possui os bytes, sem referência à RAM guest; apresenta quando o guest cede a vez (relógio avançou) — a premissa "fim de fatia" foi refutada: o modo de vídeo só existe a 98,6% da execução; **capturado de fora pelo SO: 640×480 1:1, 1208 pixels em x=26..119 y=26..88, idêntico à RAM, "Hello nxdk!" × 4**; WM_CLOSE encerra em <5 s; 437 testes estáveis em 3 rodadas; clippy limpo em Windows **e** Linux (bug de `Debug` só do Linux achado instalando o target); **seção 16.1 criada**: dívidas provisórias com condição de saída (P1–P6) |
| 2026-10-22 | 5 / alpha | Perfil de memória PFB (ADR-0058) | — | CFG0/CFG1 com procedência Cromwell e distinção 64/128 MiB; BUS0 v4/PFBM v1 migra v1–v3 sem ativar PFB; triangle chega a MmClaimGpuInstanceMemory (168), retorno 0x182AB; 499 testes workspace/doctests e 212 casos gerados debug/release; mantém 37/69 serviços |
| 2026-10-22 | 5 / alpha | Consulta de instância GPU (ADR-0059/P13) | — | Ordinal 168 query-only retail 64 MiB com 32 frames reais; KRNL v13/GINS v1 e migração; triangle alcança PRAMIN e falha depois em VA=0x80000000/EIP=0x18F19; 500 testes Linux e 215 nativos debug/release; hello 37/69, triangle 41/75 |
| 2026-10-22 | 5 / alpha | Armazenamento PRAMIN (ADR-0060/P14) | — | Tabelas/contextos persistentes, BUS0 v5/PRAM v1 e migração v1–v4 sem modelo; página zero do loader ainda bloqueia DMA; 506 testes Linux e 218 nativos debug/release |
| 2026-10-22 | 5 / alpha | Página D3D no boot (ADR-0061) | — | PA zero reservada antes dos frames XBE, headers em PA=0x1000/RO, BUS0 v6/BTPG v1 e migração sem reserva; triangle com 12 raízes passa escrita/WBINVD e ISR/DPC; timeout DMA leva à limpeza em ordinal 171; 513 testes Linux e 221 nativos debug/release |
| 2026-10-23 | 5 / alpha | Liberação contígua (ADR-0062) | — | Ordinal 171 devolve frames e remove mapas; protege backings referenciados; triangle libera 133 frames e continua no esvaziamento PFIFO; sonda 320 fatias com 93 chamadas/zero recusadas; GATE OK 519 testes e 224 nativos debug/release; hello 38/69, triangle 42/75 |

---

### Próxima ação

Inventário concluído em 2026-10-23: [ADR-0063](docs/adr/0063-inventario-pfifo-triangle.md),
37 controles PFIFO + USER PUT/GET e toda PRAMIN tocada até o primeiro polling.
Próxima tarefa do PLANO: readback/aliases com snapshot, mantendo estados de fila
sem modelo até consumo real. Baseline da sonda: 209 MMIO sem modelo. Gate: 519 testes.

Marco mais recente, 2026-10-23: [ADR-0062](docs/adr/0062-liberacao-memoria-contigua.md).
MmFreeContiguousMemory (171) libera quatro alocações reais do triangle, 133 frames,
preservando boot, framebuffer e pool GPU. Snapshot/replay antes/depois da liberação
são estáveis; formatos não mudam. Sonda de 320 fatias: 93 chamadas/zero recusadas,
EIP=0x14D36 por orçamento; 209 endereços MMIO sem modelo e framebuffer diagnóstico
1580 pixels/29 linhas. O próximo bloqueio é o esvaziamento PFIFO em pb_kill/
pb_fifo_handler (CACHE1_STATUS em 0xFD003214 sem EMPTY); DMA_GET continua sem
processamento real. Inventariar fontes/consumidores antes de modelar registradores,
consumir o pushbuffer e então implementar PGRAPH. P13/P14 seguem abertos.
Gate fixado Rust 1.98.1: GATE OK, 519 testes workspace e 224 casos gerados debug/
release; hello 38/69 e triangle 42/75. Win32 não revalidado. PLANO.md permanece
pausado; a tarefa 0.0 de compatibilidade com Clippy 1.99 não foi executada aqui.

Marco anterior, 2026-10-22: [ADR-0061](docs/adr/0061-pagina-boot-dma.md).
AOT reserva a página D3D em PA zero antes do XBE, VA=0x80000000/RW sem execução;
headers passam a PA=0x1000/RO. BUS0 v6/BTPG v1 valida a posse; v1–v5 migram sem
reserva e sem mover headers, preservando replay. Triangle passa escrita/WBINVD e
chega ao polling NV_USER DMA_GET sem modelo. Com 12 raízes (17984 blocos), ISR/DPC
executam; timeout leva à limpeza em MmFreeContiguousMemory (171), retorno 0x17520,
argumento 0x80189000. Esse bloqueio de liberação foi resolvido no ADR-0062;
a causa DMA/PFIFO e P13/P14 permanecem abertos.
Gate Linux: 513 testes workspace/doctests e 221 casos gerados debug/release;
hello 37/69 e triangle 41/75. Win32 não revalidado.
A lista abaixo preserva marcos anteriores; o estado vigente está na passagem de bastão.

Em ordem:

1. **Alpha, integração dos marcos 2 e 3/6:** os dados exportados têm representação
   (ADR-0022) e o dispatcher existe com `27/69` serviços com variantes limitadas.
   NtCreateMutant já cria objeto/handle guest; Session guarda kernel e seu
   snapshot integral (ADR-0032). Faltam os serviços de inicialização e o modelo
   de escalonamento geral do nxdk hello. O bootstrap de uma filha existe (ADR-0045).
   SHA-1 Init/Update/Final tem contexto opaco na RAM
   (ADR-0033), e a consulta integral usa a EEPROM explícita da sessão (ADR-0034).
   Espera e liberação de mutant estão implementadas sem bloquear o host (ADR-0035).
   Seções críticas (`291`, `277`, `294`, `306`) agora têm estrutura de 28 bytes
   na RAM guest, recursão, liberação e TryEnter; contenção bloqueante recusa
   (ADR-0037). RDMSR e o perfil nominal de CPU/MSRs já estão implementados:
   0x10 compartilha o Clock com RDTSC, 0x2A informa bus/razão nominais,
   índices sem modelo recusam sem efeitos. AOTC v7 salva o perfil (ADR-0038).
   Com raízes `0x19110`, `0x145E0`, `0x205E0` e `0x20889`, **9027 blocos**
   passam por HalReadWritePCISpace (46); o PLL usa configuração pública no perfil
   de clock compartilhado e o hello calcula 733333333 Hz. KRNL v3 salva PCIC v1;
   leituras desconhecidas e escritas recusam (ADR-0039).
   InterlockedExchange (54) troca LONG em RAM e retorna o anterior, com FASTCALL/8,
   atomicidade sob Bus exclusivo e sem formato novo de snapshot (ADR-0040).
   KeQuerySystemTime (128) usa o Clock único passado pela sessão e referência UTC
   explícita no power-on, padrão sintético 2000-01-01. KRNL v4/STIM v1 preserva a
   referência e migra KRNL v1–v3. Aos 704 ciclos, o hello recebeu
   125911584000000009 em RAM, sem avanço temporal no atendimento (ADR-0041).
   KeGetCurrentIrql (103) consulta a prioridade explícita da única CPU guest,
   independente de IF. KRNL v5/IRQL v1 salva o nível e migra v1–v4 para Passive;
   Raise/Lower e entrega de IRQ/APC/DPC continuam ausentes (ADR-0042).
   NtAllocateVirtualMemory (184) atende reserva+commit privado/RW com base escolhida
   e ZeroBits=0, frames reais zerados e saídas validadas. KRNL v6/VMEM v1 salva
   regiões e confere Bus/MMU/bitmap; FRM0 v1 valida contagem/padding (ADR-0043).
   O hello usa 64 KiB em 0x30000 e passa heap+8 como contexto da thread.
   PsCreateSystemThreadEx (255), KeGetCurrentThread (104) e término (258) agora
   atendem o bootstrap de uma filha: pilha/TLS/TIB e prefixo KTHREAD próprios,
   continuação do pai e Clock único, sem threads host. O hello passa por TLS/FLS
   e StartRoutine; main_wrapper=0x18C90 e inicializador=0x139B0 acrescentam duas
   raízes às seis anteriores: **10444 blocos**. A raiz 0x14F14 destrava o próximo
   case do CRT: **10676 blocos**. O automount agora consulta/cria D: no namespace
   guest; open/close de links usam handles reais e destino copiado, sem montar
   mídia na VFS. KRNL v8/SYML v1 preserva esse estado (ADR-0046). O próximo
   bloqueio passou pelo encoder: GET_SETTINGS responde padrão/flags da EEPROM e
   cabo padrão; VIDEOENABLE guarda blanking, AvGetSavedDataAddress usa cold boot
   sem framebuffer herdado. KRNL v9/AVST v1 preserva perfil/estado (ADR-0047).
   MmAllocateContiguousMemoryEx (166) já aloca 300 frames consecutivos reais,
   VA=0x80050000/PA=0x50000, com mapa restrito RW e coerência DMA. O hello zera
   1.228.800 bytes para 640×480×32; KRNL v10/CMEM v1 confere posse e aliases.
   AvSetDisplayMode já configura o modo 0x04010101 (NTSC-M 640×480/60), formato
   0x12/pitch 2560 e PA real; o runtime lê linhas BGRA por DMA. AVST v2 persiste
   o modo e migra v1 sem modo, mantendo KRNL v10 (ADR-0049). Os pedidos de filtros
   11/flicker=5 e 14/soften=false já ficam no estado lógico; AVST v3 migra v1/v2
   sem pedidos, sem aplicar filtros analógicos aos pixels (ADR-0050). O bloqueio
   seguinte, a escrita no **DAC gamma, VA=0xFD6813C8, PC=0x12EEB**, foi resolvido pelo
   **ADR-0051**: janela MMIO do NV2A mapeada como identidade e `KeDelayExecutionThread`
   para o `Sleep`. **O hello agora executa o seu `main`** e escreve "Hello nxdk!" no
   framebuffer (relógio guest 8023 ms, 47 chamadas, 0 recusadas); a parada é o orçamento
   do `main` gerado, não um serviço faltante. **28/69 serviços.**
   *Decisão a revisar:* a recomendação anterior era modelar o DAC (índice/dados RGB e
   snapshot) e **não** abrir MMIO sobre o destino genérico que descarta escritas. O
   ADR-0051 fez o contrário, com razão registrada: o destino genérico não é silencioso,
   porque a telemetria do bus registra cada acesso e o executável gerado a imprime; e o
   modelo do DAC não tem consumidor enquanto não existir apresentação. Se o mantenedor
   preferir o modelo antes, a janela e o modelo coexistem sem conflito.
   **O framebuffer agora é apresentado em janela Win32** (ADR-0052, `--janela`): a
   imagem capturada de fora pelo sistema é idêntica à da RAM guest (1208 pixels,
   "Hello nxdk!" × 4), e fechar a janela encerra a execução — o que entrega também a
   interrupção controlada **pelo host**. A apresentação é de bytes crus: gamma e filtros do
   encoder **não** são aplicados, e isso é dívida provisória declarada (§16.1, P1 e P2).
   **Falta:** vblank por interrupção, apresentação em Linux (Fases 8 e 10), e, para além do
   hello, escalonamento de threads (a espera atual só é correta com uma thread
   executável; ADR-0045: espera viva, preempção, múltiplas filhas, NtClose de
   mutant/thread e liberação ainda faltam). O orçamento do `main` gerado continua fixo no
   template. BSF/BSR destravaram a tradução anterior; separar blocos corrigiu a pilha host
   (ADRs 0030/0031). Novas raízes de callbacks/threads podem revelar mais ISA.
2. **Fase 4:** continuar os serviços mínimos efetivamente usados pelo homebrew,
   sem respostas genéricas de sucesso para ordinais pendentes.
3. **Fase 3-AOT:** ampliar a ISA restante e a descoberta de alvos, com perfis de
   imagem/cache versionado; validar o homebrew com instruções reais emitidas pelo
   compilador. Guia: `docs/dev/recomp.md`.
Git inicializado em 2026-10-19 para enviar o projeto a
`https://github.com/rodolfoaltenrath/xbox-classic-rust`, conforme solicitação do
mantenedor. A licença Apache-2.0 já selecionada no remoto é mantida (ADR-0007).
