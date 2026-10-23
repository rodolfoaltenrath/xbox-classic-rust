# Continuidade do alpha — guia para outra LLM

Atualizado em **2026-10-23**, com base no commit **`3e4b36c`** e alterações locais
do ADR **0064** (controles PFIFO persistentes e aliases USER).
Este documento registra o contexto necessário para continuar o projeto sem o histórico
da conversa. Antes de trabalhar, confira `git status`, os commits posteriores e as
instruções locais: o código pode ter evoluído depois desta referência.

> **Como ler este arquivo.** O estado vigente está na "**Passagem de bastão**" abaixo e
> na tabela de contratos ("Fim da execução atual" e "Comando completo"). As seções
> por ADR mais adiante são **registros datados**: frases como "Próximo bloqueio" ou "o
> bloqueio atual" dentro delas descrevem o momento em que cada ADR foi escrito e estão
> preservadas como histórico, não como estado vigente. Na dúvida, reproduza com o
> comando completo da tabela de contratos antes de investigar qualquer bloqueio.

## Passagem de bastão — estado em 2026-10-23

*Escrito para uma LLM (ou pessoa) que assume o projeto sem o histórico da conversa.* Esta
seção é o ponto de partida; o resto do arquivo são registros datados por ADR. Antes de
tudo: `git log --oneline -15`, `git status`, e rode o gate abaixo.

### Regras do projeto que mudam o que você faz

1. **Nada de hardware inventado.** Um registrador ou serviço sem evidência de que o caminho
   executado o usa fica como está: o `Bus` lê zero declarado e registra na telemetria
   (`MMIO sem modelo tocado pelo guest`). Modele só o que um alvo real tocou.
2. **Todo provisório é declarado** em `ROADMAP.md §16.1` (P1–P15), com condição de saída.
   Não promova um provisório a definitivo por omissão.
3. **Cada decisão estrutural tem um ADR numerado** (`docs/adr/`, o último é o 0064) e cada
   entrega entra no `CHANGELOG.md`. Erros de método também são registrados no ADR.
   O ADR-0056 registra a investigação de fontes e as ferramentas de reprodução;
   não implementa hardware novo.
4. **Recusa é melhor que suposição.** Variante não implementada devolve
   `VarianteNaoImplementada` com nome, ordinal, chamador e argumentos; nunca sucesso genérico.
5. **Snapshots são versionados, com migração testada** (`BUS0` v7/`PFFO` v1/`BTPG` v1/`PRAM` v1/`NPLL` v1/`PFBM` v1, `KRNL` v13/`GINS` v1, `KEVT` v2,
   `KINT` v2, `AOTC` v8, `AOTS` v3...). Mudou o formato: suba a versão, migre as antigas e
   escreva o teste da migração.
6. **Commits:** mensagem em inglês no estilo do repositório, **sem Co-Authored-By** nem
   outro trailer de autoria (pedido explícito do mantenedor). Desde 2026-10-23 o agente
   comita sozinho ao concluir cada tarefa com o gate verde (ciclo em `AGENTS.md`).
7. **Gate antes de entregar** (tudo precisa estar limpo):
   `cargo fmt --all --check`,
   `cargo clippy --workspace --all-targets --offline -- -D warnings`,
   `cargo test --workspace --offline --no-fail-fast` (526 testes no Linux nesta revisão; o teste
   `tools/xberecomp/tests/native.rs` compila e executa o Rust gerado e leva ~80 s; ele
   confere o **contador de casos gerados**, hoje `227 passed` em x86-64 e `211` fora dele —
   some os testes novos ali).

### O que foi feito (resumo por etapa)

| Etapa | Entrega | ADR |
|---|---|---|
| Base | arquitetura (kernel HLE, GPU por pushbuffer, backends de CPU), endereços, arena, sandbox de arquivos, loader de XBE/XISO, MMU, bus, telemetria de MMIO | 0001–0012, 0020 |
| AOT | recompilação estática: XBE → análise x86 (iced-x86) → IR → Rust gerado → binário nativo; flags, shifts, MUL/DIV, strings REP, controle indireto, relógio em ciclos, `Session { state, bus, kernel }` | 0013–0019, 0021, 0024–0031 |
| Kernel mínimo | dados exportados, dispatcher, mutants, seções críticas, EEPROM, SHA-1, PCI, tempo de sistema, IRQL, memória virtual, thread filha, links/D:, vídeo (modo, framebuffer, filtros), memória contígua | 0022–0023, 0032–0050 |
| Hello executa | janela MMIO do NV2A, `KeDelayExecutionThread` avançando o relógio, `main` do hello roda | 0051 |
| Janela | framebuffer apresentado em janela Win32 (`--janela`), só Windows | 0052 |
| 2026-10-22 | laço de execução em `Session::conduzir` + `--fatias N`; `KeTickCount`; **SSE escalar, x87 de carga/armazenamento, BT/BTS/BTR/BTC, WBINVD**; alinhamento 0 em `MmAllocateContiguousMemoryEx`; **eventos do kernel**; **interrupções, ISR, DPC e vblank** | 0053, 0054, 0055 |
| Fase 5 inicial | evidência pública e scripts; NVPLL programável; CFG0/CFG1 com perfil Cromwell 64/128 MiB; consulta de instância GPU com reserva física retail 64 MiB (P13), KRNL v13/GINS v1; PRAMIN persistente independente (P14), BUS0 v6/BTPG v1: página D3D reservada antes dos frames XBE | 0056–0061 |
| Limpeza pbkit | MmFreeContiguousMemory (171), remoção real de mapeamentos/frames, reuso e preservação dos backings referenciados | 0062 |

Números hoje: **38 de 69** serviços do hello implementados (não é percentual de
compatibilidade); triangle **42/75**; **227** casos no Rust gerado em debug/release;
**526** testes workspace/doctests no gate padrão Linux (519 no baseline + sete).
Gate adicional da tarefa 0.0: Rust/Cargo 1.99.0, GATE OK 519 testes
(target/gate/20261023-001813.log); Clippy all-targets compatível com 1.98.1.
A apresentação Windows não foi revalidada aqui. Os sete testes Python passaram
na entrega anterior; não repetidos nesta, pois scripts não mudaram.

### Estado do hello e do segundo sample

**Atualização ADR-0064:** PFIFO RegistersV1 guarda os 37 controles inventariados;
USER PUT/GET são aliases do canal DMA zero. BUS0 v7/PFFO v1; v1–v6 preservam PFIFO
sem modelo por migração. Não há consumo DMA nem EMPTY inferido. A sonda nova
(target/sonda/triangle-20261023-003811.log) mantém 93 chamadas/zero recusadas e a
tela diagnóstica, mas reduz MMIO sem modelo de 209 a 170; PUT/GET saem da lista.
Gate: 526 testes, 227 nativos debug/release. P15 declara o perfil temporário.
O relato dos ADRs 0061/0062 abaixo preserva números das sondas anteriores.

- **Hello (`target/homebrew/hello.xbe`)**: executa o `main`, desenha "Hello nxdk!" e mostra
  numa janela (1208 pixels idênticos à RAM). Comando completo e nove raízes na tabela de
  contratos mais abaixo. Revalidado em release/headless no Linux após a reserva:
  1208 pixels/48 linhas, 47 chamadas e orçamento de main esgotado normalmente
  (target/boot-hello.log). Win32 não foi revalidado aqui. A primeira compilação leva minutos.
- **`triangle` (segundo sample, pbkit)**: foi compilado com o mesmo SDK e é o guia dos
  próximos bloqueios. Estado: traduz com 12 raízes (17984 blocos, incluindo ISR/DPC), passa pela inicialização de vídeo, cria o
  evento de vblank e **instala o ISR e o DPC**. Em sessão nova, lê NVPLL=0x00011C01
  (perfil PublicReferenceV1), escreve parâmetros PTIMER e passa da verificação
  **PFB_CFG0** (FirmwareMemoryV1: 0x03070003 na arena de 64 MiB). A consulta
  **MmClaimGpuInstanceMemory (168)** agora reserva 128 KiB reais no topo da RAM,
  retorna 0x83FF0000 e escreve padding=0x10000 (ADR-0059/P13). Passa a tocar
  PRAMIN em 0xFD71386C (EIP=0x183AA); agora preserva tabelas/contextos no perfil
  IndependentV1, 1 MiB, ainda sem vínculo físico com RAM (ADR-0060/P14).
  O ADR-0061 reserva PA zero antes da imagem, VA=0x80000000/RW sem execução;
  headers ficam em PA=0x1000/RO. A sonda moderna passou da escrita e WBINVD,
  chegou ao polling de DMA_GET (0xFD800044, sem modelo) e conferiu snapshot integral.
  Valor bootstrap=0x18C001, pb_Head=0x8018C000; EIP=0x18F9D, retired/ciclos=24827445,
  54 chamadas. A execução com as dez raízes antigas passa a parar em 0x1B420,
  o ISR registrado pelo pbkit. Foram acrescentadas as raízes ISR=0x1B420 e
  DPC=0x1B340; a tradução atual usa 12 raízes/17984 blocos e executou ISR/DPC
  (12 ocorrências). DMA_GET segue zero por 12402025 leituras; pb_init/pb_kill
  expiram as esperas. O ADR-0062 atende MmFreeContiguousMemory (171): libera
  PA=0x189000/0x18A000/0x18B000 (4 KiB cada) e PA=0x18C000 (0x82000 bytes),
  total de 133 frames. Permanecem framebuffer PA=0x5D000/0x12C000 bytes,
  pool GPU e boot. A sonda controlada para após a limpeza com 87 chamadas,
  zero recusadas, EIP=0x1506B e 174079287 retired/ciclos; snapshot integral e
  mais 2 M de blocos reproduzem o mesmo estado. Continua no laço de esvaziamento
  PFIFO em pb_kill/pb_fifo_handler: CACHE1_STATUS=0 em 0xFD003214, bit EMPTY
  ausente, primeira leitura PC=0x14D36. Não concluir artificialmente a espera.
  Sonda padrão limitada a 320 fatias: 93 chamadas, zero recusadas, 252079732
  retired/ciclos, EIP=0x14D36, 209 endereços MMIO sem modelo. O framebuffer
  continua diagnóstico (1580 pixels/29 linhas), sem triângulo. Logs:
  target/free-triangle-probe.log e target/sonda/triangle-20261023-001300.log.
  BUS0 v1–v5 migram sem reserva D3D e preservam o layout físico; a sonda v5
  reproduz a falta VA=0x80000000/EIP=0x18F19, headers em PA zero, 24826623 retired.
  BUS0 v1–v4 migram sem modelo PRAMIN; sessão nova adota o perfil. BUS0 v1–v3
  preservam PFB sem modelo: o replay v3 continua rejeitando PFB e falhando em
  pb_start chamado por pb_kill, VA=0x40/PC=0x16EA2. v1/v2 também preservam PLL
  sem modelo. Iniciar sessão nova para adotar ambos os perfis (ADR-0058).

Reproduzir o `triangle`:

```bash
# 1) compilar (no WSL; reaproveita o SDK que build-homebrew.sh já clonou)
wsl -d FedoraLinux-44 -- bash /mnt/c/Users/Altenrath/emulador-xbox/scripts/build-nxdk-sample.sh triangle homebrew2
# 2) traduzir com as 12 raízes conhecidas (incluindo ISR/DPC), compilar e rodar (Git Bash, raiz do repo)
cargo run --offline -p xberecomp -- translate target/homebrew2/triangle-raw.xbe \
  --entry 0x22700 --entry 0x22C10 --entry 0x1E110 --entry 0x2A0E0 --entry 0x2A389 \
  --entry 0x1D7B0 --entry 0x22AC0 --entry 0x22790 --entry 0x14670 --entry 0x1EA44 \
  --entry 0x1B420 --entry 0x1B340 \
  --out target/homebrew2/proj
cargo run --release --offline --manifest-path target/homebrew2/proj/Cargo.toml \
  --target-dir target/homebrew2/tgt -- --fatias 4000
# Para descobrir raízes de um XBE novo, automaticamente:
bash scripts/find-translation-roots.sh <xbe> target/<pasta-nova> [raizes...]
```

(O Git Bash reescreve caminhos que começam com `/mnt/...`; chame o `wsl` pelo PowerShell
ou use o script dentro do WSL.) `llvm-objdump` existe no WSL para desmontar o `.exe`
(`llvm-objdump -d -M intel --start-address=... triangle.exe`); `awk` não existe lá.

### O que tem que ser feito agora, em ordem

As tarefas 0.0, 1.1 e 1.2 foram concluídas. ADR-0063 contém o inventário;
ADR-0064 modela os controles e aliases com snapshot. Próxima tarefa 1.3: consumir
os comandos pela PRAMIN/RAM e avançar GET somente após decodificação real.
Baseline atual: 170 MMIO sem modelo, 93 chamadas/zero recusadas, 1580 pixels/
29 linhas. Fonte, limites P15 e migrações estão no ADR-0064.

1. **PFIFO/DMA_GET com efeitos reais.** O
   [ADR-0061](docs/adr/0061-pagina-boot-dma.md) reserva a página D3D antes do XBE;
   o triangle já passa a escrita/WBINVD e chega ao polling NV_USER DMA_GET.
   Usar a PRAMIN como fonte dos descritores, implementar o canal/contexto e o
   processamento do pushbuffer com limites e snapshot. Não adiantar GET/PUT
   artificialmente para satisfazer o polling. Com 12 raízes ISR/DPC funcionam;
   DMA não avança e o timeout leva a pb_kill. O ADR-0062 libera os buffers CMEM
   comuns, preservando boot e backings referenciados; isso não resolve a causa
   do timeout. Preservar P13/P14 e a posse do boot.
2. **PGRAPH, depois dos contratos PFIFO.** Métodos NV097 mínimos para limpar a
   tela e desenhar o triângulo. Cada recorte exige consumidor real, fonte, ADR e teste.
   PTIMER continua sem modelo apesar das escritas. Preservar os perfis legados:
   BUS0 v1–v5 não recebem a reserva de boot por migração; sessão nova usa o layout novo.
3. **Itens menores já mapeados** (cada um com condição de saída no ROADMAP §16.1):
   escalonador de threads (P3); análise automática de jump tables (P5); apresentação em
   Linux (P6, Fases 8/10); `KeTickCount` em tempo real (P7); MXCSR dinâmico e aritmética
   x87 (P8/P9); DIRQL/IF/`STI` como máscara real (P10/P11); gamma e filtros na imagem
   apresentada (P1/P2).
4. **Para chegar a um jogo comercial** (longe): subir o limite de 65 536 instruções
   alcançáveis da análise (`Discovery` em `analysis.rs`), áudio (APU), USB/controle,
   armazenamento/XISO ligado à VFS, e muito mais do kernel. Não há percentual medido
   de conclusão ou base para estimar prazo pela contagem de serviços/testes.

### Revisão das ferramentas de passagem de bastão (ADR-0056)

`find-translation-roots.sh` exige diretório novo, mantém `attempt-NN/` e logs de
cada execução, mais `roots.txt` e `final-project.txt`. Não apaga projetos. Propaga
falhas e distingue timeout/limite de um retorno normal. Funciona com executável
com/sem `.exe`; o entry point já é incluído pelo analisador. Sete testes no WSL:
`python3 scripts/tests/test_translation_roots.py`. Continua sendo busca por
execução, sem resolver automaticamente jump tables na análise (P5).

`build-nxdk-sample.sh` confere SDK/submódulos fixados e limpos, mantém build sem
rede e grava `build-<sample>-manifest.txt` e `SHA256SUMS-<sample>`. O XBE permanece
raw. Os scripts foram revisados depois da passagem de bastão original; a
validação e os logs da revisão constam no ADR-0056.
O script final de raízes rodou no Git Bash com as dez raízes do triangle,
compilou 17883 blocos em release e preservou a tentativa. Retornou status 1 na
falta conhecida em VA=0x40/PC=0x16EA2, sem acrescentar raiz falsa ou anunciar
sucesso. O build revisado do sample também rodou no WSL. Gate: 486 testes
workspace/doctests, 206 casos gerados e sete testes Python separados; fmt/Clippy
limpos. A compilação inicial do triangle leva vários minutos além do timeout,
que se aplica só à execução do guest.

### Armadilhas que já custaram tempo

- **Debug vs release:** o binário gerado em debug leva ~38 s até o primeiro `Sleep`
  (21× mais lento). CPU a 100% ali é trabalho, não travamento.
- **Cada serviço de kernel consome uma fatia** no `conduzir`. "47 chamadas atendidas" ao
  esgotar o orçamento era o limite de 64 fatias, não um laço; use `--fatias`.
- **Testes de migração de snapshot** constroem o snapshot antigo *cortando* o atual numa
  tag e rebaixando versões (`KRNL`, `AOTS`). Ao acrescentar uma seção no fim, o corte pode
  precisar de ajuste; veja `verification_contiguous.inc`, `verification_links.inc`,
  `verification_video.inc` e `crates/xbox-kernel/tests/events.rs`. Em Session, TCXT vem
  depois do kernel: a migração KRNL v12 remove GINS no próprio intervalo, sem truncar
  o restante da Session (verification_gpu_instance.inc).
- **Falta dentro de `conduzir`:** `run_slice` converte `Error::Memory` em
  `Error::MemoryAt { pc, fault }`. Sondas devem conferir a variante enriquecida;
  `guest::run` direto continua devolvendo `Memory`.
- **`cargo test` para no primeiro crate que falha**; use `--no-fail-fast` para ver todos.
- **Git Bash e `sed`:** o delimitador `#` quebra com texto que contém `#`; use `|` ou o
  editor de arquivos. Arquivos do repositório estão em LF no disco e CRLF no Git (aviso
  esperado).
- **Clippy pedante:** `too_many_lines` (100), `cast_possible_wrap`, `float_cmp`,
  `unused_self`. `dispatch()` tem `#[allow(clippy::too_many_lines)]` justificado por ser a
  tabela plana de ordinais.

### Mapa rápido do código

| Onde | O que há |
|---|---|
| `crates/xbox-recomp/src/analysis.rs`, `ir.rs`, `emit.rs`, `emit/{sse,x87}.rs` | decodificação (iced-x86) → IR → Rust gerado |
| `crates/xbox-recomp/src/runtime.rs`, `runtime/{drive,interrupts,sse,x87,threads}.rs` | `State` (CPU), `Session`, `conduzir`, entrega de interrupções, semântica de SSE/x87 |
| `crates/xbox-kernel/src/dispatch.rs`, `dispatch/*.rs` | dispatcher por ordinal e cada serviço (`events`, `interrupts`, `mutants`, `video`, `contiguous`...) |
| `crates/xbox-kernel/src/dispatch/gpu_instance.rs` | consulta retail 64 MiB com reserva CMEM (P13), GINS v1 |
| `crates/xbox-kernel/src/loader/{data,mmio,startup}.rs` | imagem, dados exportados, janela MMIO, `KeTickCount` |
| `crates/xbox-mem/src/{bus,mmu,nv2a_irq,dispositivos}.rs` | bus, MMU, os 4 registradores de vblank, telemetria de MMIO |
| `crates/xbox-mem/src/nv2a_pramdac.rs` | perfil NVPLL e coeficiente autoritativo, frequência derivada, NPLL v1 |
| `crates/xbox-mem/src/nv2a_pfb.rs` | CFG0/CFG1 com perfil de firmware 64/128 MiB, readback e PFBM v1 |
| `crates/xbox-mem/src/boot_dma.rs` | reserva física da página D3D, único alias RW, BTPG v1 e validação de posse |
| `crates/xbox-mem/src/nv2a_pramin.rs` | PRAMIN de 1 MiB, perfil independente P14, acesso DWORD e PRAM v1 |
| `crates/xbox-platform/src/{display,windows}.rs` | `Apresentador` e a janela Win32 |
| `tools/xberecomp/` | CLI `translate`; `template_main.rs.txt` é o `main` gerado; `tests/support/*.rs` + `tests/verification_*.inc` são os testes que rodam Rust gerado com x86 escrito à mão |
| `scripts/` | `build-homebrew.sh` (hello), `build-nxdk-sample.sh`, `find-translation-roots.sh` |
| `docs/adr/`, `ROADMAP.md`, `CHANGELOG.md` | decisões, plano (com §16.1 de provisórios) e histórico |

## Objetivo do software e do primeiro alpha

O projeto **Obsidian / xbox-classic-rust** pretende executar software do Xbox
clássico por **recompilação estática para Rust**, com uma camada de compatibilidade
Xbox implementada em Rust. Repositório:
<https://github.com/rodolfoaltenrath/xbox-classic-rust>.

O caminho implementado é:

```text
XBE → análise x86 antes da execução → IR → código Rust → Cargo/rustc → programa nativo
                                                          ↓
                                             memória guest e kernel HLE
```

O código Rust gerado representa as operações do executável original; não recupera
seu código-fonte nem converte automaticamente todo o jogo em Rust idiomático.
Executar o código recompilado nativamente ainda exige representar memória,
serviços do Xbox e dispositivos. A meta de Rust se aplica ao recompilador/runtime
do projeto; o SDK externo nxdk usa C/C++ para produzir a entrada de teste.

**Meta do alpha:** executar o `samples/hello` real do nxdk, com saída verificável,
controle de execução pelo host e apresentação do framebuffer produzido pelo
homebrew. Não há compromisso de executar jogos comerciais neste primeiro alpha.
Controle, áudio e uma interface de mini console ficam para etapas posteriores.

**Estado até o ADR-0052 (histórico; o vigente está na "Passagem de bastão" acima):** a demonstração própria já executa Rust recompilado. O fluxo
descoberto do hello traduz e inicia nativamente; passou por NtCreateMutant (192)
e por XcSHAInit (335), consulta da EEPROM e inicialização de seção crítica.
RDMSR e a leitura PCI do PLL já estão implementados. Com as entradas de startup,
main_wrapper, um inicializador e um case do CRT, **nove raízes traduzem 10676 blocos**.
O fluxo executado passa pela troca de LONG e por KeQuerySystemTime (128),
que usa a referência UTC guest e o Clock da sessão. A consulta de IRQL já usa
prioridade explícita da CPU guest, independente de IF. Reserva+commit já entrega
64 KiB zerados em 0x30000, que o CRT usa. Uma filha guest com pilha/TLS próprios
já executa bootstrap e StartRoutine, mantendo o pai estacionado. A escrita do
DAC gamma em `0xFD6813C8` deixou de bloquear: a janela MMIO do NV2A está mapeada
(ADR-0051) e a telemetria registra os 769 acessos da rampa. **O hello agora executa o
seu `main`**: `XVideoSetMode` e depois o laço `debugPrint` + `Sleep(2000)`, com
`KeDelayExecutionThread` avançando o relógio guest. Não há mais parada por serviço
faltante nesse caminho; a execução termina no **orçamento do `main` gerado** (64 M de
blocos, ~4 iterações do laço), porque o alvo não retorna espontaneamente. O framebuffer
guest contém "Hello nxdk!" legível, e **agora uma janela Win32 o mostra**
(`obsidian-native --janela`, ADR-0052): capturada de fora pelo sistema, a janela tem
640×480 1:1 e os mesmos 1208 pixels que a RAM guest. Fechar a janela interrompe a
execução de forma controlada. A imagem é de **bytes crus**: gamma e filtros do encoder
não são aplicados — dívida provisória declarada, com condição de saída em
`ROADMAP.md §16.1` (P1 e P2). **Falta:** vblank por interrupção, apresentação em Linux
(só existe Windows), orçamento do `main` gerado ainda fixo no template, e o
escalonamento de threads para além do hello.

**Armadilha de medição, para quem for reproduzir:** o binário gerado em **debug** leva
~38 s para chegar ao primeiro `Sleep` (21× mais lento que em release, ~1,85 s). Um
processo a 100% de CPU nesse tempo está *trabalhando*, não travado. Para ver a janela,
use `--release`:

```powershell
cargo run --offline -p xberecomp -- translate target/homebrew/hello.xbe `
  --entry 0x19110 --entry 0x145E0 --entry 0x205E0 --entry 0x20889 --entry 0x13C70 `
  --entry 0x18FC0 --entry 0x18C90 --entry 0x139B0 --entry 0x14F14 --out target/alpha-janela
cargo run --release --offline --manifest-path target/alpha-janela/Cargo.toml `
  --target-dir target/recomp-hello-release -- --janela
```

Verificado literalmente neste host: a **primeira** execução leva ~3 a 4 minutos só
compilando (o perfil release usa LTO fat, `codegen-units = 1`) antes de a janela
aparecer; as seguintes reaproveitam o `--target-dir` e abrem em ~2 s. Um `cargo` ativo
sem janela nesse intervalo é compilação, não travamento.

Sem `--janela` o comportamento é o de antes (relatório em ASCII, sem abrir nada).
O hello já alocou e zerou 1.228.800 bytes/300 frames fisicamente contíguos para
o framebuffer de 640×480×32: VA=0x80050000, PA=0x50000. AvSetDisplayMode já
configura NTSC-M 640×480/60, formato A8R8G8B8 e pitch 2560. O runtime lê as
linhas BGRA por DMA e registra flicker=5/soften=false no encoder lógico, com
saída ainda desabilitada. Faltam gamma MMIO e apresentação da imagem; os pedidos
de filtros não simulam o efeito analógico sobre os pixels.
O namespace guest já permite consultar/criar D: para `\Device\CdRom0\`;
isso ainda não conecta mídia à VFS nem produz imagem.
A frequência calculada pelo hello foi conferida em RAM:
**733333333 Hz**, coerente com o Clock do projeto.
A execução integrada completa e a apresentação do framebuffer ainda estão pendentes.
Jogos comerciais ainda não executam.

## As seis entregas e o estado atual

| Passo | Entrega | Situação nesta referência | Critério para concluir |
|---|---|---|---|
| 1 | Memória e loader | **Concluído no aceite local** | Imagem real conferida pelo bus; contratos, permissões e segurança de memória validados |
| 2 | TLS e importações | **Infraestrutura implementada e testada; aceite integrado aberto** | TLS correto e chamadas por ordinal funcionando com o ambiente real requerido pelo homebrew |
| 3 | Kernel mínimo | **Memória contígua, modo, filtros, janela MMIO do NV2A e espera por intervalo; 38/69 serviços com variantes limitadas** | Implementar dados e serviços necessários ao caminho executado pelo alvo, com erro explícito nos demais |
| 4 | ISA necessária | 10676 blocos com nove raízes; execução chega ao vídeo | Traduzir os caminhos necessários do XBE real, validar semântica e descobrir seus destinos |
| 5 | Execução integrada | **Executa o `main` do hello** (`debugPrint` + `Sleep`); `Session::conduzir`, limite `--fatias` e encerramento pela janela; ISR/DPC/vblank executados no triangle, ainda sem processamento DMA/renderização; aceite integrado aberto | Rodar o homebrew com dispatcher, orçamento, logs e interrupção controlada |
| 6 | Framebuffer | **Apresentado em janela Win32** (`--janela`, ADR-0052): capturado de fora pelo SO, 640×480 1:1, 1208 pixels idênticos à RAM guest, "Hello nxdk!" × 4. **Limites:** só Windows; bytes crus, sem gamma nem filtros do encoder (provisório, ROADMAP §16.1 P1/P2); caminho "usuário fecha durante a execução" não exercitado por teste | Apresentar uma imagem produzida pela execução do homebrew |

Há diversas entregas de implementação registradas, mas isso não significa
aceites completos do alpha. O passo 2 depende de partes do passo 3
para fechar sua integração. As seis entregas não são uma estimativa de seis
commits, seis dias ou uma garantia de compatibilidade.

## Passo 1 — memória e loader

Commit: **`42e5c60` — `feat(loader): share XBE loading and validate the memory subsystem`**.

### O que foi feito

- Loader compartilhado `xbox_kernel::loader::Image`: headers, seções, BSS,
  alocação de frames, validação de faixas e permissões por página.
- `Mode::Inspect` permite inspecionar imagens com páginas RWX e oferece alias
  físico em `0x80000000 + PA`, sem executar código.
- `Mode::Aot` recusa páginas simultaneamente graváveis e executáveis, inclusive
  páginas compartilhadas entre código e dados; não oferece o alias físico RW.
- `Image::verify` compara a imagem com o arquivo e a BSS, confere entry point,
  ordinais pelo bus e uma página não mapeada.
- CLI `obsidian check-memory` aceita XBE, pasta com `default.xbe` ou ISO; a leitura
  da ISO ocorre sob demanda e a inspeção não executa o título.
- Escritas do bus validam a faixa inteira antes de alterar RAM/MMIO, evitando
  stores parcialmente aplicados quando uma página posterior falha.
- Contratos de memória, benchmark seguro, auditoria de unsafe e job Miri no CI.
  A arena mantém posse exclusiva: `Send`, sem `Sync` e sem `Clone`.

### Evidência registrada

O teste T14 local com NFS Underground 2 verificou **14 seções, 1132 páginas,
4634697 bytes e 152 ordinais**. Nenhuma instrução comercial foi executada.
Seis contratos passaram sob Miri; o teste nativo de cenários comparou 20000 casos.
Os testes de workspace e do Rust gerado passaram, assim como Clippy e formatação.
Os resultados detalhados são históricos dessa entrega; não são novas execuções
feitas ao escrever este guia.

Arquivos para retomar:

- [Loader](crates/xbox-kernel/src/loader.rs) e [verificação](crates/xbox-kernel/src/loader/verify.rs).
- [Testes do loader](crates/xbox-kernel/tests/loader.rs).
- [Guia de memória e comandos](docs/dev/memoria.md).
- [ADR-0020](docs/adr/0020-loader-memoria-validada.md) e [auditoria de unsafe](docs/arch/memoria-unsafe.md).

**Limite:** conferir os bytes de uma imagem comercial não demonstra que o jogo
execute. A aceitação de RWX na inspeção não autoriza habilitar RWX no AOT sem
resolver a correspondência entre código traduzido e bytes modificáveis.

## Passo 2 — TLS, imports e build real do nxdk

Commit: **`478c76d` — `feat(kernel): add nxdk TLS initialization and AOT import trampolines`**.

### O que foi feito

- Build do `samples/hello` com nxdk fixado na revisão
  `14d5ee97e73347c973f1f57b68b79ec08c9e77f2` e submódulos fixados.
  `scripts/build-homebrew.sh` registra ferramentas, hashes e logs em `target`.
- Build repetida no WSL/Fedora com Clang/LLD 22.1.8. O script preserva o XBE
  original e normaliza timestamps e dois bytes de padding de metadata do cxbe.
  Não altera código, TLS ou imports para contornar incompatibilidades.
- `ordinal-report` deixou de ser esqueleto: informa TLS, ordinais, nomes, ABI,
  distinção entre funções/dados e o primeiro bloqueio da análise AOT.
- Catálogo de 371 exports gerado das declarações públicas CC0 do nxdk. Ter o
  nome e a assinatura no catálogo não significa implementar o serviço.
- `Image::prepare_aot` prepara o perfil inicial de uma thread, com pilha,
  NT_TIB mínimo, TLS nxdk e resolução de imports de funções.
- TLS com template, zero-fill, payload alinhado e índice negativo relativo a
  `FS:[4]`. A área ocupa o topo da pilha; ESP e retorno ficam abaixo dela.
- `State::fs_base` guarda um endereço guest. Operandos de memória usam FS;
  LEA calcula só o offset. Leituras por FS não são inferidas como ponteiros
  absolutos do arquivo.
- Trampolins RX com endereços guest e marcadores por ordinal. Somente destinos
  importados são incluídos no despacho gerado; não há ponteiros host no guest.
- Saída `Exit::KernelCall`, leitura de argumentos por `Session::kernel_call`
  e retorno por `Session::complete_kernel_call`, com convenções
  stdcall/cdecl/fastcall. As respostas controladas dos testes validam a ABI;
  não são implementações de serviços do Xbox.
- Snapshot `AOTC` v3 preserva FS e migra v1/v2 com base zero. Uma chamada pendente
  é reconstruída por EIP, registradores e RAM, sem continuação oculta no host.

### Contratos que a próxima implementação deve preservar

| Elemento | Contrato atual |
|---|---|
| Pilha inicial | 64 KiB a partir de `0x70000000` |
| NT_TIB mínimo | `0x7FFD0000`; não equivale a KPCR/KTHREAD completos |
| Trampolins | Página RX em `0x7FFE0000`; slot em `base + ordinal * 8` |
| TLS nxdk | `align16(raw + zero_fill) + 4`; slot autorreferente e índice negativo |
| Proteções | Patches em dados RO restauram permissões; páginas executáveis não são alteradas |
| ABI | Stdcall limpa argumentos; cdecl deixa a limpeza ao chamador; fastcall usa ECX/EDX e limpa só a parte na pilha |
| Estado de execução | CPU, RAM, contextos e objetos HLE; usar snapshot integral de Session (AOTS v2) |
| Falta de suporte | Erro identificável; não retornar sucesso genérico para avançar |

O perfil limita TLS a meia pilha e recusa callbacks e alinhamentos acima de
16 bytes. Strings com segmento continuam recusadas. Na entrega do ADR-0021,
ainda não havia serviços HLE ou dados exportados; os avanços posteriores estão
no passo 3 abaixo. O scheduler continua pendente.

### O que foi descoberto no XBE real

| Item | Resultado da build de referência |
|---|---|
| Imagem | 4 seções, 27 páginas, 104141 bytes verificados pelo loader |
| Entry point | `0x00018C00` |
| Imports | 72 entradas: **69 funções e 3 dados**, sem ordinal desconhecido |
| Template TLS | `0x2A000..0x2A110`, 272 bytes; zero-fill 0 |
| Índice TLS | Endereço `0x28108` |
| Callbacks/alinhamento | Sem callbacks; characteristics `0x00300000`, alinhamento 4 |
| Análise AOT atual | 9454 instruções/blocos com raízes 0x19110, 0x145E0, 0x205E0, 0x20889, 0x13C70 e 0x18FC0 |
| Fim da execução atual | **Orçamento do `main` gerado esgotado**, não um bloqueio: 64 M de blocos, 17.452.569 instruções, relógio guest 8023 ms, 47 chamadas atendidas e 0 recusadas. Executa `XVideoSetMode` e ~4 iterações de `debugPrint` + `Sleep(2000)` (ADR-0051). *Histórico: a linha anterior desta tabela dizia `PsCreateSystemThreadEx` (255), retorno `0x13BF1`, resolvido pelo ADR-0045.* |
| Comando completo | `translate target/homebrew/hello.xbe --entry 0x19110 --entry 0x145E0 --entry 0x205E0 --entry 0x20889 --entry 0x13C70 --entry 0x18FC0 --entry 0x18C90 --entry 0x139B0 --entry 0x14F14` (nove raízes, 10676 blocos). Com menos raízes a execução para antes, em `UnknownTarget`. |
| ISA antes bloqueante, agora implementada | `LOCK CMPXCHG` em `0x209C3`; `SETE` em `0x209C9` (ADR-0024) |

Os três dados importados são `LaunchDataPage` (164), `PsThreadObjectType` (259)
e `XeImageFileName` (326). O CRT nxdk cria uma thread para executar `main`.
Logo, TLS e transporte de chamadas, isoladamente, não bastam para rodar o alvo.
As 69 funções são **importações estáticas**; só a execução prova quais são chamadas
em um caminho. O fluxo já exerceu NtCreateMutant e XcSHAInit, chegando à EEPROM; isso não
fornece um inventário completo das chamadas futuras.

### Evidência e arquivos

Passaram testes do workspace e doctests, **69 testes do Rust gerado em debug e
release**, Clippy global e formatação. Os testes cobrem ABI, TLS/FS, LEA,
snapshot com chamada pendente, tail call, orçamento e erros. A execução dos
testes gerados usa 69 casos em x86_64; o conjunto varia conforme o host.
**Nessa entrega ainda não havia serviços HLE de produção. O hello completo não rodou.**

- [TLS e startup](crates/xbox-kernel/src/loader/startup.rs).
- [Imports e ABI](crates/xbox-kernel/src/imports.rs) e [catálogo](crates/xbox-kernel/src/imports/catalog.rs).
- [Runtime e snapshots](crates/xbox-recomp/src/runtime.rs).
- [Análise](crates/xbox-recomp/src/analysis.rs), [IR](crates/xbox-recomp/src/ir.rs) e [emissão Rust](crates/xbox-recomp/src/emit.rs).
- [Testes de startup](crates/xbox-kernel/tests/startup.rs), [análise de startup](crates/xbox-recomp/tests/startup.rs) e [testes nativos de ABI](tools/xberecomp/tests/verification_kernel.inc).
- [Build, hashes e fontes públicas](docs/dev/homebrew.md) e [ADR-0021](docs/adr/0021-tls-imports-aot.md).

## Passo 3 — kernel mínimo: próxima entrega

**Objetivo:** dar semântica real aos dados e serviços que a inicialização do alvo
precisa. A infraestrutura do passo 2 deve ser reutilizada.

### Item 1 concluído — dados exportados (ADR-0022)

Os três dados têm representação em memória guest, em duas páginas porque a permissão
da MMU é por página e o título escreve um deles:

| Export | Endereço | Permissão | Forma |
|---|---|---|---|
| `PsThreadObjectType` (259) | `0x7FFF0000` | somente leitura | `OBJECT_TYPE` de 28 bytes, procedimentos **envenenados** |
| `XeImageFileName` (326) | `0x7FFF0100` | somente leitura | `ANSI_STRING` + buffer com caminho NT |
| `LaunchDataPage` (164) | `0x7FFF1000` | leitura e escrita | ponteiro de 4 bytes, nulo |

O slot do thunk de um dado recebe o endereço do símbolo, não um trampolim — o nxdk usa
`__declspec(dllimport)` e o código carrega o slot antes de desreferenciar. Dado que o
catálogo conhece e ninguém modelou continua recusando a imagem, com nome e ordinal.

Dez testes em `crates/xbox-kernel/tests/data.rs` cobrem layout, ponteiros, permissões e
conteúdo, incluindo a recusa de escrita na página somente-leitura e o envenenamento
ficar em página não mapeada.

**Efeito medido:** `translate` do `hello.xbe` real deixou de parar no ordinal 164 e
agora para em `LOCK CMPXCHG` em `0x209C3` — bloqueio de **ISA, em tempo de tradução**.
Isso confirma que, sendo AOT, instrução faltante bloqueia antes de qualquer serviço.
Resolver os dados destravou a próxima investigação; não aproximou a execução por si.

O caminho em `XeImageFileName` é `\Device\CdRom0\default.xbe`: valor **declarado**, não
derivado de mídia. Ligar isso à VFS é pendência registrada.

### Item 2 concluído — dispatcher (ADR-0023)

`xbox_kernel::dispatch::Dispatcher` atende em `(&mut Bus, pc, regs)`, e quem aplica o
resultado na CPU é `Session::atender_kernel`. Fica em `xbox-kernel` porque receber
`Session` fecharia ciclo com `xbox-recomp`.

Contratos que a próxima implementação deve preservar:

| Elemento | Contrato |
|---|---|
| Recusa | erro com nome, ordinal, **endereço do chamador** e argumentos pela ABI |
| Cdecl | mostra as primeiras palavras e avisa que a quantidade não é declarada |
| Falha | CPU **intacta**; o estado pendente continua retomável por EIP/registradores/RAM |
| Fonte única | `dispatch::IMPLEMENTADOS`, consultada pelo registro e pelo relatório |
| Sucesso genérico | proibido: ordinal sem serviço devolve erro, nunca `EAX = 0` |

Primeiro serviço: `RtlInitAnsiString` (289), escolhido por ser função pura sobre a
memória guest — mede a origem e preenche os três campos do destino, sem estado de
kernel e sem nada inventado. Destino nulo é recusado; string sem NUL dentro do limite
de `ANSI_STRING` é erro, não truncamento.

O projeto gerado passou a ter laço de despacho (executa, atende, retoma). Duas
verificações no Rust **gerado** cobrem o caminho completo; o contador de testes gerados
foi de 69 para 71. O relatório mostra `1/69 serviços implementados`.

**`1/69` não mede proximidade de executar o homebrew.** Após o dispatcher, o alvo
parava em `LOCK CMPXCHG` em `0x209C3`. O ADR-0024 implementou essa operação e SETcc;
o alvo passou a parar em RDTSC (`0x2055B`). O ADR-0025 implementou RDTSC com
Clock guest. O ADR-0026 implementou CLI/HLT e o ADR-0027 modelou a fronteira #BP
de INT3. O ADR-0028 implementou SFENCE no bus síncrono exclusivo; o bloqueio
seguinte foi CMOVNE (`0x13B88`). O ADR-0029 implementou CMOVcc e encontrou BSR
(`0x1657C`). O ADR-0030 implementou BSF/BSR e permitiu traduzir o fluxo descoberto
(8725 instruções/blocos). Após corrigir a pilha host e incluir um callback
explícito, a execução real chega a NtCreateMutant (ADR-0031), sem concluir o alpha.

### Avanço — NtCreateMutant e estado de kernel (ADR-0032)

`NtCreateMutant` (192) cria mutexes sem nome com handles guest distintos,
proprietário inicial opcional e recursão inicial 0/1. Aceita atributos nulos ou
struct Xbox de 12 bytes zerada; nomes/flags/diretórios ainda recusam com erro de
variante não implementada. Ponteiros inválidos devolvem NTSTATUS sem objeto ou
store parcial. Limite local: 4096 objetos. Espera, liberação, fechamento, abandono
e scheduler continuam pendentes; a identidade 1 é apenas a thread inicial.

`Session` agora possui `kernel: Dispatcher`; chamar `session.atender_kernel()`
sem argumento externo. O retorno é validado/capturado antes dos efeitos do
serviço. Snapshot integral de `Session` usa `AOTS` v1 e inclui CPU `AOTC` v6,
bus, `KRNL` v1 e `KMUT` v1. **Salvar apenas CPU/bus não preserva objetos HLE.**
Regenerar projetos antigos para a nova API. Carga inválida exige descartar sessão.

O hello real foi traduzido, compilado e executado com `--entry 0x19110` e passou
por NtCreateMutant. O novo bloqueio é **XcSHAInit (335)**, retorno `0x19140`,
argumento `0x28EF0`, no caminho de `init_rand_s` do nxdk. O fonte público usa um
contexto de 116 bytes e depois chama EEPROM/SHAUpdate/contador; implementar o
contrato e estado corretos de SHA antes de avançar, sem zerar contexto como stub.

Registro nessa entrega: **2/69 serviços** (variantes limitadas), 3/3 dados. Foram validados
**271 testes de workspace/doctests** e **115 casos gerados em debug/release**
(99 em outros hosts), Clippy global, fmt, loader do hello e diff. Esse avanço
não fecha kernel mínimo, threads, execução integrada nem framebuffer.
Detalhes e logs: [ADR-0032](docs/adr/0032-mutants-guest-snapshot-sessao.md).

### Avanço — SHA-1 em contexto opaco guest (ADR-0033)

`XcSHAInit`/`XcSHAUpdate`/`XcSHAFinal` (335/336/337) implementam SHA-1 com estado
inteiramente nos 116 bytes da RAM guest. Copiar o contexto cria um fluxo independente;
snapshot parcial funciona sem mapa host. O formato interno é **HLE v1**, com marcador
`XSH1`, não um layout comprovado do kernel físico. Atende código que trata o buffer
como opaco. Final preserva a origem por política local, salvo sobreposição da saída;
não prometer preservação byte a byte pelo hardware. Layout e limites estão no ADR.

Ponteiros/faltas/RO/MMIO/overflow são recusados antes de publicar mudanças.
VOID usa EAX zero por convenção local, não NTSTATUS inventado. Não houve novo
unsafe, dependência ou estado extra a serializar; versões de snapshot permanecem.
Há vetores públicos de SHA-1, referências .NET para padding/páginas, testes de
fragmentação, cópia, sobreposição e três casos no Rust gerado.

O hello real passou por SHAInit e parou em **ExQueryNonVolatileSetting (24)**,
retorno `0x19176`, argumentos `0xFFFF, 0x7000FD78, 0x7000FD80, 0x100, 0x7000FD7C`.
O próximo trabalho é modelar o contrato dessa consulta e o conteúdo/estado da
EEPROM, com origem e política explícitas, sem retornar sucesso com um buffer fictício
ou buscar dados na BIOS. SHAUpdate/Final estão validados em testes; ainda não foram
exercidos pelo hello, que parou antes deles.

Registro nessa entrega: **5/69 serviços com variantes limitadas**, 3/3 dados; **288 testes
de workspace/doctests** e **118 casos gerados em debug/release** (102 em outros
hosts), Clippy global e fmt. Logs e detalhes:
[ADR-0033](docs/adr/0033-sha1-contexto-guest.md). Kernel mínimo, threads, orçamento/
interrupção integrada e framebuffer continuam com aceite aberto.

### Avanço — consulta integral da EEPROM (ADR-0034)

`ExQueryNonVolatileSetting` (24) atende somente `XC_MAX_ALL` (`0xFFFF`), com
256 bytes da imagem autoritativa de EEPROM do dispatcher, tipo `REG_BINARY` e
tamanho resultante. Outras consultas continuam com erro de variante não modelada.
Validar as três saídas antes de escrever evita publicação parcial; buffer curto
publica só o tamanho necessário e devolve NTSTATUS, conforme política local.

`EepromState` registra bytes e origem `SyntheticV1` ou `Supplied`. O padrão AOT
reutiliza o gerador existente em NTSC-M/português; não lê BIOS, arquivo ou config
do host. API permite imagem explícita, sem recalcular bytes fornecidos.
O perfil sintético tem bloco de segurança em claro e não é dump físico válido.
Consulta devolve cópia; `ExSave`, persistência e índices individuais ainda faltam.

Snapshot agora usa **KRNL v2 + EEPR v1**, guardando imagem e origem. A carga
preserva os bytes salvos, independentemente da sessão de destino; KRNL v1 migra
explicitamente para a EEPROM sintética padrão. AOTS v1 e AOTC v6 permanecem;
**KMUT foi para a v2** (ver abaixo).

### Espera/liberação de mutex e CLC (ADR-0035, ADR-0036)

`NtWaitForSingleObject` (233) e `NtReleaseMutant` (221) estão implementados sobre o
mutant existente. A espera **nunca bloqueia a thread do host**: com uma thread guest,
cada caso tem resposta definida — adquire, recursa, `STATUS_INVALID_HANDLE`,
`STATUS_MUTANT_LIMIT_EXCEEDED`, ou erro de variante para `Alertable`, `Timeout` e
"mutant de outra thread" (que exigiria scheduler). `PreviousCount` não nulo é recusado:
ele devolve o estado de sinal anterior, cuja relação com a contagem de recursão não foi
verificada, e escrever valor suposto na memória do título seria erro silencioso.

**`KMUT` foi para a v2**, porque a v1 só representava os estados de criação (`0/0` e
`1/1`) e a espera produz contagem acima de 1. A v1 continua carregando com sua
validação restrita. `MAX_RECURSION` é `i32::MAX`, porque o campo no kernel é `LONG`.

Depois disso o fluxo exigiu **mais uma raiz explícita**: `--entry 0x145E0`. O endereço
é um construtor global alcançado por `call [eax]` em `0x18E23`, dentro do laço que o CRT
usa para percorrer a tabela de ponteiros de inicialização em `.rdata` (próxima a
`0x26B5C`). A análise não infere porque o ponteiro passa por um slot de pilha. A tabela
tem outros candidatos (`0x139B0`, `0x205E0`, `0x16DE0`), que podem exigir raízes
próprias conforme o caminho avançar.

Com essa raiz apareceu `CLC`, implementado junto com `STC`/`CMC`. As três entraram no
mesmo braço de `analysis::targets` que `Nop`/`Direction` para **não invalidar** o
rastreamento de registradores — sem isso, um `CLC` entre o carregamento de um ponteiro e
uma transferência indireta perderia um destino descoberto.

Na entrega dos ADRs 0035/0036, o hello passou por mutex, SHA, EEPROM e construtores e parou em
**`RtlInitializeCriticalSection` (291)**, retorno `0x11063`, com o objeto em `0x2810C` —
o mesmo endereço que o construtor em `0x145E0` carrega com `lea`. Essa família foi
implementada na entrega seguinte, descrita abaixo.

Registro nessa entrega: **8/69 serviços**, 3/3 dados; **308 testes workspace/doctests** e
**122 casos gerados em debug/release** (106 nos demais hosts), Clippy, fmt e loader.
Detalhes e logs: [ADR-0035](docs/adr/0035-espera-liberacao-mutant.md) e
[ADR-0036](docs/adr/0036-clc-stc-cmc.md). Kernel mínimo, threads, execução integrada
completa e framebuffer continuam pendentes.

### Seções críticas e retomada no Linux (ADR-0037)

Init/Enter/Leave/TryEnter (`291/277/294/306`) operam sobre os 28 bytes da estrutura
Xbox na RAM guest. Aquisição recursiva, liberação final e TryEnter têm efeitos
observáveis. A identidade 1 continua sintética, não KTHREAD; evento, contagens e
ponteiros internos seguem o perfil documentado no ADR. Contenção bloqueante,
esperadores e overflow recusam como variantes, sem bloquear o host ou alterar RAM.
Ponteiros, permissões e toda a faixa são validados antes de escrever.
Estado/snapshot: AOTS v1, AOTC v6, KRNL v2, KMUT v2, EEPR v1; nenhum estado host novo.

O SDK foi reconstruído neste Linux/Fedora 44 com Clang/LLD 22.1.8 e os mesmos
hashes de PE/XBE da referência Windows/WSL. Com as duas raízes anteriores,
o hello passa pelo serviço 291 e chega a `UnknownTarget` em `0x18E23 → 0x205E0`.
Traduzir com a terceira raiz recusa RDMSR em `0x207C6`; o disassembly carrega
`ECX=0x2A`. O fonte público `lib/winapi/profiling.c` identifica `tsc_freq_init`
e `get_cpu_multiplier`: consulta EBL_CR_POWERON e potencialmente PCI. A próxima
entrega precisa modelar CPU/MSRs guest em conjunto com o Clock, sem usar RDMSR
host ou valores escolhidos só para desviar do caminho.

Registro nessa entrega: **12/69 serviços**, 3/3 dados; **317 testes workspace/doctests**,
**125 casos nativos debug/release** (109 nos demais hosts). Nove testes novos de
kernel e três gerados cobrem layout, recursão, faltas, ABI, retry e snapshot.
O Clippy Linux também exigiu conversão verificada de `usize` para `i64` antes de
criar/dimensionar o memfd; o caminho Windows não exercitava essa conversão.
Detalhes: [ADR-0037](docs/adr/0037-secoes-criticas-guest.md).

Ambiente local: o Rust do sistema é 1.99; o **1.98.1 fixado** está em
`~/.cache/obsidian/rust-1.98.1/bin` e deve preceder `/usr/bin` no PATH para os
checks do projeto. As ferramentas nxdk estão extraídas, sem instalação de sistema,
em `~/.cache/obsidian/toolchain/usr`; comandos em `docs/dev/homebrew.md`.
Esses caches são locais, não acompanham o Git. Logs desta entrega têm prefixo
`target/critical-`; o projeto hello executável está em
`target/alpha-hello-critical-20261020`.

### RDMSR, perfil de CPU e próximo serviço PCI (ADR-0038)

RDMSR lê ECX e publica EDX:EAX sem tocar flags ou os demais registradores.
`CpuProfile::XboxRetailV1` (ID 1) é explícito em State: MSR 0x10 lê o mesmo Clock
que RDTSC; 0x2A devolve 0x01040000 (bus nominal 133⅓ MHz × razão 5,5). Demais
bits dessa configuração são zero por política sintética, não dump físico.
Índices sem modelo produzem erro com PC/índice, sem alterar CPU/RAM nem cobrar o
passo; WRMSR/CPL/CR4/entrega de #GP continuam pendentes. Não há RDMSR host.

**AOTC agora é v7**, com ID de perfil de CPU no final do payload; v1–v6 migram
explicitamente para o perfil 1. AOTS v1/KRNL v2/KMUT v2/EEPR v1 permanecem.
O perfil de custos de tempo continua 1 e os MSRs não ganham relógio próprio.

Com a raiz `0x205E0`, o hello traduz 9024 blocos e passa por RDMSR. O JMP em
`0x2081E` encontra o case `0x20889`, que devolve multiplicador 55 (5,5 × 10).
A tabela passa por slot de pilha e ainda não é inferida; declarar esse destino
observado com `--entry 0x20889` traduz 9027 blocos. O erro real passa a ser
**HalReadWritePCISpace (46)**, retorno `0x2064D`, argumentos
`0, 0x60, 0x6C, 0x7000FE9C, 4, 0`. A origem é o construtor de frequência em
`lib/winapi/profiling.c` do nxdk fixado. A próxima entrega deve modelar a
configuração PCI guest/clock do chipset necessária a essa leitura, sem inventar
um valor para desviar do cálculo.

Registro nessa entrega: **12/69 serviços**, 3/3 dados; **322 testes workspace/doctests** e
**131 casos nativos debug/release** (115 fora de x86_64), no Rust 1.98.1. Cinco
novos testes do recompilador/runtime e seis gerados cobrem efeitos, limites,
retry e migração/snapshot. Evidências: [ADR-0038](docs/adr/0038-rdmsr-perfil-cpu-guest.md),
logs `target/msr-*`. Projeto de execução atual: `target/alpha-hello-msr-ratio-20261020`.

### Leitura PCI do clock e próximo serviço atômico (ADR-0039)

HalReadWritePCISpace (46) atende o registro PLL do controlador DRAM, bus 0,
slot NT 0x60 (device 0, function 3), offsets 0x6C..0x70, incluindo subleituras.
O valor público 0x01230801 tem divisor 1/multiplicador 8. Escritas e outras
funções/regiões recusam; não existe tabela PCI genérica cheia de zeros.
Buffer é validado integralmente em RAM antes de escrever; função VOID usa
EAX=0 por convenção. BOOLEAN usa byte baixo. Faixa vazia não acessa Buffer.

`xbox_core::hardware::RetailClockV1` agora centraliza a configuração de CPU/PLL;
0x2A conserva exatamente seu valor do ADR-0038. A equação do nxdk dá 733333333 Hz,
verificada contra CPU_HZ por asserção de compilação e por testes. Nenhum acesso
a portas PCI host ou dump privado. Os demais efeitos dos campos do PLL não
estão implementados; escritas exigirão rever clock/dispositivo em conjunto.

**KRNL agora é v3**, acrescentando PCIC v1 (ID de perfil PCI 1); KRNL v1/v2
migram explicitamente preservando objetos/EEPROM/contadores. AOTS v1/AOTC v7/
KMUT v2/EEPR v1 permanecem. O dispatcher publica a carga apenas após validar
todas as seções. PCI/MMIO futuros devem compartilhar essa autoridade de estado.

O hello mantém 9027 blocos/quatro raízes e passa pela leitura PCI. O próximo
bloqueio real é **InterlockedExchange (54)**, retorno `0x19DC7`, argumentos
`0x29158, 1`. É FASTCALL/8 (ECX/EDX), conforme catálogo/header nxdk. Modelar
RMW real de RAM guest, retorno do valor anterior e efeitos indivisíveis sob a
posse exclusiva do Bus; não retornar sucesso sem trocar. A família pode exigir
outros membros conforme o caminho avançar.

O teste local adicional sobre o XBE real conferiu `tsc_freq` em 0x27D10/0x27D14:
733333333, parte alta zero, antes do ordinal 54. Está no artefato ignorado
`target/alpha-hello-msr-ratio-20261020/tests/pci_clock.rs`, log
`target/pci-hello-clock-test.log`. Não é fixture do repositório.

Registro nessa entrega: **13/69 serviços**, 3/3 dados; **331 testes workspace/doctests** e
**135 casos nativos debug/release** (119 fora de x86_64), no Rust 1.98.1. Nove
novos testes de kernel e quatro gerados cobrem leitura, cálculo guest, variantes,
ABI, faltas, migração e snapshot. Detalhes/fontes: [ADR-0039](docs/adr/0039-configuracao-pci-clock-guest.md).
Logs desta entrega usam prefixo `target/pci-`. Threads, execução completa e
framebuffer continuam abertos.

### InterlockedExchange e próximo contrato de tempo (ADR-0040)

InterlockedExchange (54) captura Target/Value por ECX/EDX (FASTCALL/8), valida
quatro bytes de RAM para leitura/escrita, troca o LONG e retorna os 32 bits
anteriores em EAX. Mesmo valor igual exige escrita; MMIO recusa antes de acesso.
Desalinhamento/página cruzada seguem o perfil do Bus quando toda a faixa é RAM RW.
Atomicidade/ordenação vêm de Bus síncrono/exclusivo, sem yield entre load/store,
como CMPXCHG/SFENCE; observadores concorrentes futuros exigirão novo protocolo.
Não há AtomicU32 host, unsafe ou promessa de atomicidade entre threads host.

A sessão preserva flags/outros registradores e captura o retorno antes do efeito,
inclusive quando Target aponta ao frame de retorno. FASTCALL/8 não remove oito
bytes de argumentos da pilha; só os quatro do retorno. Não há estado host novo,
então AOTS v1/AOTC v7/KRNL v3/KMUT v2/EEPR v1/PCIC v1 permanecem.
Os outros serviços Interlocked continuam ausentes; apenas 54 é importado pelo alvo.

O hello mantém quatro raízes/9027 blocos, passa pela troca e chega a
**KeQuerySystemTime (128)**, retorno `0x20D8B`, argumento `0x7000FDD8`.
Um teste local do XBE real conferiu retorno anterior em EAX, escrita em RAM,
LONG 0x29158=1 e frequência ainda 733333333 Hz. Artefato ignorado:
`target/alpha-hello-msr-ratio-20261020/tests/interlocked_effect.rs`;
log `target/interlocked-hello-effect-test.log`.

A entrega então planejada era tempo de sistema guest: saída de 8 bytes em
intervalos de 100 ns desde 1601/GMT, com época explícita, Clock único e snapshot
da referência temporal. Naquela referência o dispatcher só recebia Bus/PC/regs.
Essa entrega foi implementada no ADR-0041, descrito a seguir.

Registro nessa entrega: **14/69 serviços**, 3/3 dados; **336 testes workspace/doctests** e
**140 casos nativos debug/release** (124 fora de x86_64), no Rust 1.98.1. Cinco
novos testes de kernel e cinco gerados cobrem bits/guardas, ABI, faltas, aliases,
retorno, tail-call e snapshot. Detalhes/fontes:
[ADR-0040](docs/adr/0040-interlocked-exchange-guest.md). Logs usam prefixo
`target/interlocked-`. Threads, execução completa e framebuffer continuam abertos.

### Tempo de sistema da sessão (ADR-0041)

`KeQuerySystemTime` (128), stdcall/4 VOID, escreve LARGE_INTEGER little-endian
em RAM guest após validar todos os oito bytes. A data absoluta deriva do Clock
único da sessão: referência UTC no power-on + floor(ciclos × 10000000 / CPU_HZ).
`SystemTimeReference` tem padrão sintético **2000-01-01 UTC**
(`125911584000000000` ticks desde 1601); permite referência não negativa explícita
por `from_boot_ticks` e `Dispatcher::with_system_time`. Não usa hora host, EEPROM,
`retired`, nanos intermediários ou contador independente. O dispatcher recebe
Clock por `atender_com_clock`; a entrada anterior sem Clock recusa essa consulta.

A soma é verificada contra LARGE_INTEGER positivo. Saída inválida, MMIO, wrap,
página parcial/RO e overflow recusam sem alterar CPU/conteúdo RAM ou referência.
Traduções podem marcar Accessed/Dirty da MMU durante preflight. O HLE não cobra
ciclos próprios. Retorno capturado antes da escrita permite saída sobre a pilha.

**KRNL v4 / STIM v1** persistem a referência UTC, mantendo AOTS v1/AOTC v7/
KMUT v2/EEPR v1/PCIC v1. O Clock segue em AOTC/CLK0. KRNL v1–v3 migram para o
padrão, sem herdar configuração de destino; carga de kernel é transacional.

O hello mantém quatro raízes/9027 blocos. Na consulta real, Clock=704 ciclos,
CurrentTime=`125911584000000009`, saída `0x7000FDD8`; os oito bytes e o Clock
inalterado foram conferidos por teste local. O CRT libera depois o lock em
`0x29158`, que passa de 1 para 0. A frequência continua 733333333 Hz.
Naquela entrega o bloqueio seguinte era **KeGetCurrentIrql (103)**, retorno
`0x20E09`, sem argumentos. O contexto explícito de IRQL e seu snapshot foram
implementados no ADR-0042, descrito a seguir.

Registro nessa entrega: **15/69 serviços**, 3/3 dados; **342 testes workspace/doctests** e
**145 nativos debug/release** (129 fora de x86_64), Rust 1.98.1. Seis novos testes
de kernel e cinco nativos cobrem tempo/ABI, falhas, snapshot, migração e replay.
Clippy global/fmt/diff aprovados. Fontes, limites e reprodução no
[ADR-0041](docs/adr/0041-tempo-sistema-guest.md); logs `target/system-time-*`.
Threads, execução completa e framebuffer continuavam pendentes nessa entrega.

### Bootstrap de uma filha guest (ADR-0045)

PsCreateSystemThreadEx (255), KeGetCurrentThread (104) e PsTerminateSystemThread
(258) elevam o catálogo a **20/69 serviços**. Uma filha, identidade 2, executa
enquanto o pai, identidade 1, aguarda com continuação explícita. Pilha/TLS/TIB
próprios são RAM real zerada; o bootstrap guest inicializa slot/template/TLS/FLS.
O prefixo KTHREAD termina em TlsData+0x28 no fim da página; acessar os campos
seguintes falha na página ausente. Não representa ETHREAD completo.

Session preserva registradores/flags/FS do pai, sem copiar Clock/retired ou
perfil de CPU/IRQL. Mutants e seções críticas usam a identidade ativa. Handles
são compartilhados sem colisão entre tipos. Término noreturn ou RET local
restaura o pai; espera na filha terminada retorna WAIT_0. Recursos ficam retidos.
OOM, saídas inválidas/aliases e variantes recusadas não publicam criação parcial.

**Limites:** uma filha por sessão, sem suspensão/debugger/extensão; Passive;
pilha 64 KiB–1 MiB, TLS zero ou 4+16*n até meia pilha. A filha roda primeiro:
sem preempção, yield/Sleep ou garantia de progresso do pai durante laço da filha.
Espera viva/timeout, NtClose/liberação, KTHREAD da thread inicial e abandono de
mutant seguem ausentes. Término com mutant adquirido é recusado.

Snapshot atual: **AOTS v2, AOTC v7, KRNL v7, KMUT v3, THRD/TCXT v1**, além de
EEPR/PCIC/STIM/IRQL/VMEM v1. Valida objetos/handle/região, Bus/MMU/bitmap,
TIB/KTHREAD/guarda, código e consistência dos contextos. Formatos antigos migram
sem filha/pai; erro de carga exige descartar a sessão.

O hello real passou por criação, KeGetCurrentThread, TLS/FLS e StartRoutine.
CALL 0x18FF4 revelou main_wrapper=0x18C90; acrescentar a raiz deu 10170 blocos.
O inicializador 0x139B0, chamado em 0x18E84, elevou a **10444 blocos**. As oito
raízes são `0x19110`, `0x145E0`, `0x205E0`, `0x20889`, `0x13C70`, `0x18FC0`,
`0x18C90`, `0x139B0`. O próximo bloqueio medido é **UnknownTarget no JMP
0x14E86 → 0x14F14**, case de tabela do CRT. Investigar esse destino e continuar
por demanda; o corpo de main/framebuffer ainda não foi demonstrado.

Validação: **365 testes workspace/doctests e 162 nativos debug/release**
(146 nos demais hosts), mais execução do main gerado com RET da filha.
Clippy global/fmt/diff aprovados. Fontes, perfil, migração e reprodução no
[ADR-0045](docs/adr/0045-bootstrap-thread-guest.md); logs `target/threads-*`.

### Namespace D: e chegada ao vídeo (ADR-0046)

Incluir `--entry 0x14F14` às oito raízes anteriores destravou o case do CRT sem
alterar ISA/inferência: **10676 blocos**. A execução revelou NtOpenSymbolicLinkObject
(203), retorno 0x143B3. O automount público do nxdk consulta D:; se ausente,
obtém XeImageFileName, remove o nome do executável e cria o link para o diretório.

O namespace guest começa sem D:. A consulta de ausência devolve
STATUS_OBJECT_NAME_NOT_FOUND sem consumir handle/escrever saída; IoCreateSymbolicLink
(67) guarda uma cópia do alvo, rejeitando duplicatas com NAME_COLLISION. Open
posterior cria handle real; NtClose (187) fecha só handles de link sem desmontar.
Contador compartilhado com mutants/threads; no máximo 256 handles de link.
**23/69 serviços**, com variantes limitadas.

Perfil: D:, nomes absolutos/OBJ_CASE_INSENSITIVE/RootDirectory nulo; destino
ASCII de até 260 bytes sob CdRom0; Passive. OBJECT_STRING de 8 bytes é lida por
Length, sem exigir NUL. Ponteiros, estrutura e faixa validados como RAM antes
dos efeitos; aliases sobre argumentos/retorno usam a continuação capturada.
Outras unidades, query/delete, atributos adicionais e NtClose de mutant/thread
recusam. Não há link host ou montagem na VFS; o alvo deriva do XeImageFileName
declarado, ainda sem mídia conectada.

**KRNL v8/SYML v1** guarda destino e handles, conferindo faixa/ordem/duplicatas,
contador compartilhado e colisões entre tipos. AOTS v2/AOTC v7/KMUT v3 e demais
seções permanecem. KRNL v1–v7 migram sem D:, sem herdar o estado de destino.

O teste real confirmou D: = `\Device\CdRom0\`, thread ativa 2 e TLS self-slot em
0x4FEEC. Aos **9498 retired e 35 atendimentos**, chegou a **AvSendTVEncoderOption
(2)**, retorno **0x12769**, args `0xFD000000, 6, 0, 0x27D38`. Snapshot integral
nessa fronteira restaurou bytes idênticos, incluindo namespace/pai estacionado.
Próximo trabalho: investigar opção 6 e o caminho público de vídeo; implementar
comportamento necessário e seguir os diagnósticos. Ainda sem framebuffer/alpha
completo ou scheduler geral.

Validação: **372 testes workspace/doctests e 165 casos nativos debug/release**
(149 fora de x86_64), Clippy global/fmt/diff. Reprodução e fontes no
[ADR-0046](docs/adr/0046-namespace-drive-d.md); logs `target/links-*` e
`target/crt-case-*`. O teste real adicional está em
`target/alpha-hello-crt-case-20261021/tests/drive_mount.rs` (local, ignorado).

### Encoder inicial e pedido do framebuffer (ADR-0047)

AvSendTVEncoderOption (2), GET_SETTINGS (6), escreve padrão/flags dos bytes
EEPROM e cabo padrão. O perfil inicial retorna **0x00480101**; a preferência
480p da EEPROM não implica saída HD, pois o SDK filtra os modos pelo cabo.
UNKNOWN padrão/flags, base/opções/param sem modelo e Result fora de RAM gravável
recusam sem store parcial. A API é VOID; EAX=0 é convenção, Result contém os dados.

GET_SETTINGS destravou VIDEOENABLE (9), Param=1/Result=NULL, retorno 0x12DAD.
O estado lógico enabled agora é explícito: Param é blanking (1 desabilita,
0 habilita). Perfil cold boot inicia desabilitado e sem framebuffer herdado;
AvGetSavedDataAddress (1), retorno 0x12B45, devolve NULL por essa invariante.
AvSetSavedDataAddress permanece sem implementação. São **25/69 serviços**.
Nada disso programa a NV2A, entrega vblank ou apresenta uma imagem.

**KRNL v9/AVST v1** guarda ID StandardCableColdBootV1=1 e enabled canônico.
Padrão/flags seguem autoritativos na EEPROM; não há cache/Clock duplicado.
KRNL v1–v8 migram para saída desabilitada sem herdar estado do destino.
AOTS v2/AOTC v7/KMUT v3 e demais seções não mudaram. Formatos inválidos recusam.

As nove raízes/10676 blocos agora chegam a **MmAllocateContiguousMemoryEx (166)**,
retorno **0x12C01**, argumentos `0x12C000, 0, 0x7FFFFFFF, 0x1000, 0x404`.
São **1.228.800 bytes para 640×480×4**, alinhamento 4 KiB e RW|WRITECOMBINE.
Ainda não há framebuffer alocado. O teste real confirmou settings em 0x27D38,
blanking, D:/thread ativa e snapshot integral com replay byte a byte: **9777
retired e 38 atendimentos** nessa fronteira.

Próximo: representar frames fisicamente contíguos, faixa/alinhamento/proteção,
endereços guest/alias usados pelo vídeo, falhas sem efeitos e snapshot. Não
reutilizar a reserva virtual fragmentada fingindo contiguidade. Depois vêm
programação de modo/MMIO, imagem e apresentação. Referência pública, perfil e
reprodução no [ADR-0047](docs/adr/0047-encoder-video-cold-boot.md).

Oito testes de kernel e três novos casos no Rust emitido cobrem dados/ABI,
falhas, blanking, EEPROM e snapshots/migração. Aceite: **380 testes
workspace/doctests e 168 nativos debug/release** (152 fora de x86_64), Clippy
global, fmt, diff e links aprovados. Evidência local:
`target/video-workspace.log`, `target/video-native-release.log`,
`target/video-clippy.log`, `target/video-hello-effect.log` e teste ignorado
`target/alpha-hello-crt-case-20261021/tests/video_startup.rs`.

### Memória fisicamente contígua e chegada ao modo de vídeo (ADR-0048)

MmAllocateContiguousMemoryEx (166), stdcall/20, agora reserva frames consecutivos
no bitmap do Bus: first-fit respeitando limites físicos inclusivos, alinhamento
e tamanho arredondado a 4 KiB. Retorna **VA=0x80000000|PA**, com somente as páginas
do recurso mapeadas RW, sem execução. Não abre uma janela sobre toda a RAM nem
cria alias baixo de identidade. CPU/visão física/DMA usam a mesma arena.

Perfil: Passive, alinhamento potência de dois >=4 KiB, PAGE_READWRITE com ou sem
WRITECOMBINE. Cache/filas não são simulados: permanece a coerência síncrona do
Bus/SFENCE. Zero inicial, inclusive slack, é política local. OOM/fragmentação,
faixa insuficiente e tamanho inválido retornam NULL sem publicar recurso;
variantes não modeladas recusam. MMU/bitmap são planejados em cópias. Nenhum
unsafe novo ou compartilhamento da arena. São **26/69 serviços**.

**KRNL v10/CMEM v1** guarda PA/tamanho/alinhamento/proteção ordenados. Session
confere RAM, bitmap, tradução linear RW, posse exclusiva e ausência de aliases
externos/páginas órfãs na janela, sem marcar A/D. VMEM e CMEM não compartilham
frames. KRNL v1–v9 migram sem recursos contíguos; payload legado não pode esconder
novas páginas no Bus. AOTS v2/AOTC v7 e demais seções permanecem. Liberação,
persistência, consulta de tamanho/endereço físico e outras proteções faltam.

O hello recebeu **VA 0x80050000, PA 0x50000, 300 páginas/1.228.800 bytes**.
Teste real sujou todo o buffer com 0xA5 e comprovou que o memset guest o zerou;
depois conferiu o endereço físico passado ao vídeo e snapshot integral com
replay byte a byte. A fronteira ficou em 17213022 retired/ciclos e 39 chamadas
atendidas, com encoder desabilitado. Evidência:
`target/contiguous-hello-effect.log` e teste local ignorado
`target/alpha-hello-crt-case-20261021/tests/framebuffer_allocation.rs`.

Próximo bloqueio: **AvSetDisplayMode (3)**, retorno **0x12CA4**, args
`0xFD000000, 0, 0x04010101, 0x12, 0xA00, 0x50000`. É o modo 640×480 NTSC-M/60,
formato A8R8G8B8/pitch 2560 e PA do buffer real. Modelar modo/scanout e leitura
do framebuffer por PA, com limites/stride/formato, depois controles de encoder,
MMIO e apresentação necessários. Ainda sem imagem apresentada ou alpha completo.

Validação: **388 testes workspace/doctests e 171 casos nativos debug/release**
(155 fora de x86_64), Clippy global/fmt/diff/links. Oito testes de kernel e três
casos novos no Rust emitido cobrem efeitos, DMA, falhas e snapshots/migração.
Fontes/perfil/reprodução no [ADR-0048](docs/adr/0048-memoria-fisica-contigua.md);
logs `target/contiguous-*`.

### Modo de vídeo e leitura do framebuffer (ADR-0049)

AvSetDisplayMode (3), stdcall/24, configura Step=0, modo 0x04010101, formato
0x12 (A8R8G8B8): NTSC-M 640×480/60. EEPROM deve informar NTSC-M; pitch alinhado
a quatro, entre 2560 e 16384 bytes, PA alinhado a quatro. O intervalo inteiro
pitch*480 precisa caber em um recurso CMEM registrado, inclusive com offset.
Base/Step/modo/formato/padrão de TV não modelados recusam. Geometria/PA inválidos
recusam sem publicar modo. Valida Bus/MMU/bitmap antes do efeito; preserva
enabled, flags e Clock. Retorna Step=0, concluindo a transação HLE sem executar
a sequência interna de registradores do hardware. São **27/69 serviços**.

Dispatcher::framebuffer(&Bus) retorna metadados e uma visão emprestada por DMA.
row(y) fornece os 640 pixels BGRA visíveis, sem padding de pitch; fora de 0..480
retorna None. A visão também existe com saída desabilitada, para inspeção.
Empresta a arena, impedindo retomada mutável do Bus durante seu uso; nenhum
unsafe novo, thread host ou mudança de Send/!Sync. Apresentação futura pode
copiar as linhas antes de retomar o guest. Nenhum pixel é sintetizado.

**AVST v2** guarda presença de modo e ID/formato/pitch/PA; v1 migra sem modo,
preservando enabled e limpando modo de destino. KRNL fica v10, AOTS v2/AOTC v7/
CMEM v1 permanecem. Carga cruza modo/EEPROM/CMEM antes de publicar Dispatcher;
Session confere também RAM/MMU/bitmap/aliases. Inspeção não marca A/D.

O hello configurou pitch=2560, PA=0x50000 e avançou até **AvSendTVEncoderOption
(2), opção 11**, retorno **0x12DFA**, args `0xFD000000, 11, 5, 0`: filtro de
cintilação, ainda sem implementação. O teste real conferiu as 480 linhas pretas
zeradas pelo guest e snapshot integral byte a byte: **17213045 retired/ciclos,
40 chamadas atendidas**, saída desabilitada. Evidência local ignorada:
`target/alpha-hello-crt-case-20261021/tests/display_mode.rs` e
`target/display-hello-effect.log`. As nove raízes/10676 blocos permanecem.

Próximo: ajustes do encoder chamados pelo SDK (flicker opção 11, soften opção
14), rampa gamma via MMIO (base NV2A+0x6813C8/0x6813C9), habilitação e apresentação
do framebuffer. Ainda sem texto Hello nxdk desenhado, janela ou alpha completo.
Vblank/IRQ, outros modos/formatos e scheduler geral não foram implementados.

Validação: **394 testes workspace/doctests e 174 casos nativos debug/release**
(158 fora de x86_64), Clippy global e Rust emitido em release com `-D warnings`,
fmt/diff/links. Seis testes de kernel e três casos nativos novos cobrem efeitos,
pixels guest, ABI, falhas, snapshots e migração. Perfil, fontes e reprodução:
[ADR-0049](docs/adr/0049-modo-video-framebuffer.md), logs `target/display-*`.

### Pedidos de filtros do encoder (ADR-0050, estado atual)

AvSendTVEncoderOption (2) aceita 11/flicker com nível 0..7 e 14/soften com 0/1,
base NV2A e Result nulo. Registra pedidos independentes em VideoState,
preservando modo/blanking, RAM, CPU fora da ABI e Clock. Cold boot usa None,
distinto de pedido explícito zero/false. Parâmetros inválidos não são mascarados
nem Result desreferenciado. Não aplica filtros analógicos aos pixels; a visão
BGRA continua expondo RAM guest sem transformações. São variantes do mesmo
ordinal: **mantém 27/69 serviços**, sem aumentar o contador por opção.

**AVST v3** acrescenta dois bytes após o modo opcional: flicker=0..7 e
soften=0/1, ou 0xFF para pedido ausente. Carga recusa bytes inválidos/truncamento
sem alterar o destino. V1/v2 migram sem pedidos, sem herdar filtros do destino;
v2 conserva modo, v1 mantém a migração anterior sem modo. KRNL fica v10;
AOTS v2/AOTC v7/CMEM v1 e os cruzamentos EEPROM/CMEM/Bus permanecem.

O hello passou por flicker=5 (retorno 0x12DFA) e soften=false (retorno 0x12E63).
Próximo bloqueio: **PC=0x12EEB, escrita de um byte zero em VA=0xFD6813C8**,
NV_USER_DAC_WRITE_MODE_ADDRESS, falta **NaoMapeado** na MMU. Ainda não houve
operação de dispositivo. Fronteira: **17232328 retired/ciclos e 42 chamadas
atendidas**, saída desabilitada. Snapshot integral e repetição da falta mantêm
CPU/tempo/bytes. Teste local ignorado
`target/alpha-hello-crt-case-20261021/tests/encoder_filters.rs`, log
`target/encoder-filters-hello-effect.log`. Nove raízes/10676 blocos permanecem.

Próximo: modelo do DAC gamma com índice/componente/tabela RGB e snapshot,
mais mapeamento restrito. O SDK seleciona índice zero em 0xFD6813C8 e escreve
256 triplas RGB em 0xFD6813C9; a rampa inicial é identidade. Mapear a página no
destino MMIO genérico atual apenas descartaria os bytes, sem implementar o DAC.
Depois vêm habilitação/apresentação. Ainda sem texto, janela, vblank/IRQ ou
Sleep/escalonamento geral. Nenhum unsafe novo ou thread host; BIOS não usada.

Validação: **398 testes workspace/doctests e 177 casos nativos debug/release**
(161 fora de x86_64), Clippy global e projeto emitido em release com `-D warnings`,
fmt/diff/links. Quatro testes novos de kernel, três casos nativos novos e migração
integral de AVST v2 com modo configurado. Fontes/perfil/reprodução no
[ADR-0050](docs/adr/0050-pedidos-filtros-encoder.md), logs `target/encoder-filters-*`.

### IRQL explícito da CPU guest (ADR-0042)

`KeGetCurrentIrql` (103), stdcall/0, consulta o `Irql` guardado pelo dispatcher
para a única CPU guest. O contexto inicial é **Passive (0)**; Apc (1) e Dispatch
(2) também são representáveis/configuráveis por `Dispatcher::with_irql` para uma
sessão nova. A consulta retorna KIRQL com extensão zero em EAX e preserva flags,
registradores fora de EAX/ESP/EIP, Clock, retired, RAM e nível. Não lê argumentos;
stdcall/0 remove só a palavra de retorno. CLI modifica IF e não modifica IRQL.

**KRNL v5/IRQL v1** persiste o nível em um byte. AOTS v1/AOTC v7/KMUT v2/EEPR v1/
PCIC v1/STIM v1 permanecem; IF segue em AOTC, sem prioridade duplicada. KRNL
v1–v4 migram para Passive, sem inferir de IF nem herdar contexto de destino.
Carga de kernel continua transacional, com recusa de versões/níveis desconhecidos.

Este é um perfil de consulta de prioridade lógica. Não há entrega de IRQ/APC/DPC,
DIRQLs, APIC/PIC/KPCR físico, scheduler ou fiscalização de IRQL em todas as APIs.
KfRaiseIrql/KfLowerIrql continuam ausentes e recusam sem alterar prioridade;
o construtor de contexto não é uma transição de prioridade.

No hello real, a consulta em `0x20E09` devolveu Passive com **IF=0**, preservando
CPU/Clock. O hello mantém quatro raízes/9027 blocos e avança até
**NtAllocateVirtualMemory (184)**, retorno **0x20E68**. Argumentos:
`0x7000FD94, 0, 0x7000FD98, 0x3000, 4`; os ponteiros contêm **BaseAddress=0**
e **RegionSize=0x10000 (64 KiB)**. A reserva/commit privado usada por esse pedido foi implementada no ADR-0043,
descrito a seguir, com frames reais e snapshot das regiões.

Registro nessa entrega: **16/69 serviços**, 3/3 dados; **346 testes workspace/doctests** e
**148 nativos debug/release** (132 fora de x86_64), Rust 1.98.1. Quatro novos testes
de kernel e três nativos cobrem nível/ABI/IF, tail-call, falha/retry e snapshot.
O teste de diagnóstico sem argumentos passou a usar KeGetCurrentThread (104),
ainda ausente. Clippy global/fmt/diff aprovados. Fontes e reprodução no
[ADR-0042](docs/adr/0042-irql-guest.md), logs `target/irql-*`; teste local do XBE
real em `target/alpha-hello-msr-ratio-20261020/tests/irql_effect.rs`.
Threads, execução completa e framebuffer continuam pendentes.

### Reserva+commit privado de memória virtual (ADR-0043)

`NtAllocateVirtualMemory` (184), stdcall/20, atende *BaseAddress=0, ZeroBits=0,
MEM_RESERVE|MEM_COMMIT (0x3000), PAGE_READWRITE (4), em Passive. Bases alinhadas
em 64 KiB e tamanhos arredondados a páginas de 4 KiB. First-fit começa após o
intervalo completo do XBE, preserva buracos internos e pula páginas existentes;
o limite superior é a pilha em 0x70000000. São políticas explícitas do perfil.
Reserva isolada, commit sobre reserva prévia, base solicitada, outras proteções,
ZeroBits, top-down, reset e NtFreeVirtualMemory continuam recusados.

O serviço valida os dois parâmetros de entrada/saída como RAM R/W e recusa
sobreposição, inclusive alias físico. Planeja em cópias da MMU/bitmap, obtém
frames próprios (sem exigir contiguidade), zera os bytes e publica a alocação.
Saídas inválidas, tamanho inválido e falta de RAM retornam NTSTATUS reais do
perfil, sem alocações/saídas parciais. Variantes ausentes continuam erros claros.
HLE não altera Clock/flags/retired. Perm::RW sem execução é a política AOT, não
uma promessa de NX físico no Pentium III. Continuação capturada permite RegionSize
sobre a palavra de retorno. Limite: 4096 regiões e RAM efetivamente livre.

**KRNL v6/VMEM v1** salva intervalo do XBE e registros base/tamanho ordenados.
AOTS v1/AOTC v7/KMUT v2/EEPR v1/PCIC v1/STIM v1/IRQL v1 permanecem. MMU, frames
e RAM continuam no Bus; a carga de Session confere esses estados contra o registro,
headers XBE e posse física. Inspeção não marca Accessed/Dirty. KRNL v1–v5 migra
sem alocações; Session recupera o intervalo do XBE correspondente usado para
abri-la e confere seus headers na RAM restaurada. Dispatcher isolado precisa de
`for_image` para alocar. Sessão com erro de snapshot deve ser descartada.
FRM0 continua v1, com validação transacional de contagem/bitmap/padding/truncamento.
MMU ganhou clone independente e enumeração de páginas, sem unsafe novo.

O teste real confirmou **base 0x30000, 65536 bytes, 16 frames próprios RW zerados**.
O CRT escreve nessa região e chama **PsCreateSystemThreadEx (255)**, retorno
**0x13BF1**, argumentos `0x7000FE88, 0, 0x10000, 0x114, 0, 0x18FC0, 0x30008,
0, 0, 0x13C70`. O contexto é heap+8. Permanecem quatro raízes/9027 blocos.

Próximo passo: thread guest com handle/identidade, pilha, TLS de 0x114 bytes,
SystemRoutine=0x13C70 e StartRoutine=0x18FC0, incluindo espera/término e snapshot.
Inspecionar o CRT e descobrir as raízes necessárias; não pular para main ou
substituir a criação por sucesso genérico/thread host.

Registro atual: **17/69 serviços**, 3/3 dados; **354 testes workspace/doctests** e
**153 nativos debug/release** (137 fora de x86_64), Rust 1.98.1. Sete novos testes
de kernel, um de frames e cinco nativos cobrem efeitos, falhas, ABI e snapshot.
Clippy global/fmt/diff aprovados. Fontes, limites e reprodução no
[ADR-0043](docs/adr/0043-alocacao-virtual-guest.md); logs `target/virtual-memory-*`.

### Retomada Windows — raízes da thread e NOP multibyte (ADR-0044)

O Git chegou limpo em 94d222a. Foram revisadas as entregas de espera/liberação,
seções críticas, RDMSR/PCI, InterlockedExchange, tempo, IRQL e memória virtual.
O README ainda tinha o estado de 6 serviços; foi atualizado para 17/69.

Antes de criar a thread, incluir StartRoutine `0x18FC0` e SystemRoutine `0x13C70`
como raízes AOT revelou Nop_rm32 em `0x13C68`. O ADR-0044 baixa NOP multibyte
16/32 para a IR Nop existente, sem avaliar/acessar o operando de memória.
Flags/registradores permanecem iguais, e o custo é por instrução, não por byte.
LOCK/REP/PAUSE e segmentos fora do perfil continuam recusados.

As seis raízes traduzem **9454 instruções/blocos**, e o módulo real compilou e
executou até a mesma recusa **PsCreateSystemThreadEx (255)**, retorno `0x13BF1`.
Ainda **não há criação ou execução de thread**, nem novo serviço registrado.
Não confundir tradução das entradas com a execução do corpo da thread.

O startup público WinapiThreadStartup usa KeGetCurrentThread()->TlsData, prepara
slot/template/zero-fill, registra FLS, chama StartRoutine e termina via ExitThread.
Implementar o contrato SystemRoutine(StartRoutine, StartContext), identidade,
handle, pilha/TLS, contextos, espera/término e snapshot. Não saltar direto a main.
Mutants e seções críticas ainda usam INITIAL_THREAD; revisar a propriedade ao
ativar outra thread. O Clock precisa continuar único; IRQL pertence à CPU.

Evidência desta retomada: **357 testes workspace/doctests**, **156 casos gerados
em debug/release** (140 nos demais hosts), Clippy global, fmt e diff.
Fontes, limites e logs: [ADR-0044](docs/adr/0044-nop-multibyte-raizes-thread.md).
Teste real: `target/alpha-hello-msr-ratio-20261020/tests/virtual_memory_effect.rs`.
Threads, execução completa e framebuffer continuam pendentes.

Trabalho restante:

1. ~~Modelar os três dados exportados~~ — concluído acima.
2. ~~Implementar um dispatcher para `Exit::KernelCall`~~ — concluído acima.
3. Levantar a ordem das chamadas de inicialização a partir do CRT e do código
   compilado; implementar cada serviço necessário com teste de comportamento.
4. Ampliar o bootstrap de threads do ADR-0045 quando o caminho executado exigir:
   yield/Sleep, espera viva, progresso do pai e ciclo de vida dos handles.
   Uma filha cooperativa já existe, mas não é um scheduler geral.
5. Acrescentar memória/objetos, sincronização, tempo, arquivos e outros serviços
   conforme os caminhos realmente necessários. Manter os demais como erros claros.

**Aceite:** dados resolvidos com representação adequada, serviços implementados
com resultados/efeitos verificáveis e chamadas reais atravessando o dispatcher.
Uma tabela de zeros ou funções que sempre retornam sucesso não fecha o passo.
A integração deve também fechar as pendências do passo 2.

## Passo 4 — ISA exigida pelo compilador

**Objetivo:** traduzir os caminhos necessários do XBE real para Rust executável.

Já existem muitas operações inteiras, subregistradores, flags, pilha, branches,
CALL/JMP indiretos, multiplicação/divisão e strings REP retomáveis. A análise
descobre fluxo direto, constantes locais e ponteiros imutáveis; `--entry` permite
adicionar raízes explícitas para callbacks e outros destinos não inferidos.

O [ADR-0024](docs/adr/0024-cmpxchg-setcc.md) adicionou CMPXCHG 8/16/32,
LOCK em destino RAM e as 16 condições SETcc. A indivisibilidade usa posse exclusiva
do bus, sem yield dentro da instrução; MMIO é recusado. As flags e resultados de
CMPXCHG foram comparados com a CPU física em 147456 casos. O projeto gerado tem
agora 79 testes em x86_64, incluindo FS/SIB, aliases, RO, faltas e snapshots.

O [ADR-0025](docs/adr/0025-relogio-aot-rdtsc.md) implementou RDTSC e integrou o
`Clock` já existente no core. A política AOT v1 cobra um ciclo virtual por
instrução ou elemento REP concluído: é sintética, não latência física do Xbox.
Falhas não cobram o passo que falhou; fatias, orçamento zero e polls HLE não
inventam tempo. Snapshot AOTC v4 guarda o perfil e CLK0; v1–v3 recebem Clock zero,
sem derivar ciclos de `retired`. Perfil desconhecido e esgotamento são erros.
O Scheduler do core ainda não está integrado; futuras threads e serviços de tempo
devem compartilhar esse único relógio da sessão, não criar relógios por thread.
Essa entrega levou o projeto gerado a 84 testes em x86_64.

Falta implementar e validar as demais instruções dos caminhos necessários.
O [ADR-0026](docs/adr/0026-controle-cli-hlt.md) implementou CLI/HLT, encontrados
no caminho `_xbox_assert`. CLI limpa IF guest no perfil privilegiado; HLT guarda
o PC da parada, avança EIP e devolve `Exit::Halted` com IF. O main gerado encerra
com diagnóstico, sem bloquear o host nem anunciar retorno. Polls, inclusive com
orçamento zero, preservam estado e tempo; não há entrega de IRQ/NMI nem retomada
automática. AOTC v5 guarda a parada e migra v1–v4 com CPU ativa, preservando as
regras anteriores de tempo. Essa entrega chegou a 88 testes gerados em x86_64.

O [ADR-0027](docs/adr/0027-int3-breakpoint-aot.md) modelou INT3 (`0xCC`) como
`Exit::Breakpoint { pc }`, vetor 3, EIP seguinte e exceção persistente em polls e
snapshots. Não há IDT/SEH nem retomada automática; o main encerra com diagnóstico.
O preenchimento encontrado após CALL em `0x20D67` corresponde ao fim de abort,
confirmado pelo objeto público e suas relocações para raise/exit. Isso não prova
que esse caminho executou. A descoberta termina na exceção; branches e raízes
explícitas continuam descobrindo seus destinos, inclusive depois de INT3.
Retomada futura exige tratamento explícito e continuação traduzida.

AOTC v6 guarda o breakpoint; v1–v5 migram sem essa pendência, preservando as
regras anteriores de FS, Clock e HLT. Essa entrega chegou a 92 casos nativos
em x86_64 e encontrou SFENCE em `0x12A43`.

O [ADR-0028](docs/adr/0028-sfence-ordenacao-do-bus.md) acrescentou IR própria de
SFENCE e chamada a `Bus::ordenar_escritas`. O corpo da barreira é vazio no perfil
atual: RAM é aplicada sincronicamente, a arena é exclusiva e não há store buffer
a publicar. Isso não implementa MMIO ou conclusão de GPU. Filas de stores,
write-combining ou observadores concorrentes futuros exigem ampliar a barreira
e rever os contratos de memória; uma fence host isolada não substitui esse trabalho.
Não há estado novo: AOTC continua v6 e o perfil de tempo continua 1.
Essa entrega chegou a 96 casos gerados em x86_64 e um contrato de visibilidade
por alias/físico/DMA validado também sob Miri; o bloqueio seguinte foi CMOVNE.

O [ADR-0029](docs/adr/0029-cmovcc.md) implementou as 16 condições de CMOVcc,
destino registrador e origem registrador/memória de 16/32 bits. A origem é lida
incondicionalmente antes da condição: condição falsa não evita falta nem efeito
de leitura do bus. Flags são preservadas; word conserva a metade alta. Aliases,
FS/SIB, SP/ESP, RO, leituras cruzando página, snapshots e tempo têm testes no Rust
gerado. O novo unsafe fica apenas no oráculo opcional de xbox-cpu, com assembly
fixo em registradores locais e flags mascaradas; não recebe endereços guest.
São 32768 comparações físicas e 103 testes gerados em x86_64. Não há estado novo:
AOTC permanece v6 e o perfil de tempo permanece 1.

O [ADR-0030](docs/adr/0030-bsf-bsr.md) implementou BSF/BSR de 16/32 bits,
com origem registrador/memória e ZF definido. Para zero, o runtime preserva o
destino; normaliza CF/PF/AF/SF/OF para zero, como política para os indefinidos da
referência clássica. Word usa só os 16 bits baixos e preserva a metade alta do
destino. AOTC permanece v6, perfil de tempo 1. O novo unsafe está só no oráculo
físico opcional; AOT/runtime/bus continuam sem novo unsafe.

São 262144 comparações físicas dos campos definidos (em zero, somente ZF),
mais testes portáveis de flags/zero, RO/faltas, aliases, FS/SIB, SP/ESP, branches,
snapshots e tempo. O projeto gerado tem 110 casos em x86_64 (94 em outros hosts).
A análise real do fluxo descoberto do hello **passa com 8725 instruções e blocos**.
Isso não garante novos destinos de threads/callbacks, todos os serviços nem
execução completa: implementar/descobrir novos caminhos pode revelar mais ISA.

O [ADR-0031](docs/adr/0031-blocos-aot-pilha-host.md) corrigiu um estouro de pilha
host no primeiro executável real: cada bloco agora é função não inlineada, com
até 32 instruções. O dispatcher seleciona o ponteiro de função e faz uma chamada
por passagem; CALL/RET guest não geram recursão nativa. A inferência de destinos
ocorre antes de limitar os blocos. Orçamento de sequências longas fica mais fino,
sem alterar Clock ou AOTC v6. Há regressão em pilha host solicitada de 128 KiB.
O total dessa entrega foi 111 testes gerados em x86_64 (95 em outros hosts).

A execução revelou `UnknownTarget` no CALL `0x209D2 → 0x19110`. Essa raiz de
callback é adicionada explicitamente com `--entry 0x19110`; sem ela o erro deve
continuar explícito. Com ela, 8769 instruções/blocos compilam e executam até
**NtCreateMutant (192)**, retorno reportado `0x18CFA` (CALL em `0x18CF4`),
argumentos `0x28EEC, 0, 0`. O nxdk chama isso em `threads/mtx_init.c`.
Esse bloqueio levou à implementação de mutants e espera/liberação nos ADRs
0032/0035. RDMSR foi implementado no ADR-0038 e PCI no ADR-0039; *(histórico, na época do ADR-0043: o bloqueio era PsCreateSystemThreadEx — já resolvido pelo ADR-0045; o estado vigente está em "Estado atual", no início deste arquivo)*. O dispatcher
continua recusando serviços ausentes.
Preservar semântica de flags, acessos, atomicidade aplicável e falhas sem efeitos
parciais; comparar com referências independentes quando apropriado.

**Aceite:** tradução dos caminhos necessários do alvo, incluindo entradas das
threads/callbacks, sem decodificação x86 durante a execução. Erros de fluxo e
instruções ainda faltantes devem continuar explícitos. Os passos 3 e 4 podem
precisar avançar intercalados; corrigir imports não prova que a ISA esteja pronta.

## Passo 5 — execução integrada

**Objetivo:** conectar imagem, Rust gerado, bus, dispatcher e threads em uma
execução observável e limitada pelo host.

Reutilizar `Session`, `State`, `Exit::{Returned, Budget, KernelCall, Halted, Breakpoint}` e os
snapshots. Implementar o ciclo de despacho HLE e retomada, orçamento/interrupção,
logs úteis e relatório do primeiro erro não suportado. Manter separadas as
condições de retorno, orçamento esgotado, espera de thread e término do processo.

O `hello` configura vídeo, imprime texto e chama Sleep num laço: não se deve
exigir que ele retorne espontaneamente. A espera precisa permitir progresso do
host e das threads, e o host deve conseguir encerrar a sessão.

**Aceite:** executar código recompilado do homebrew, observar efeitos reais da
inicialização/`main`, repetir o resultado e interromper de forma controlada.
A demonstração própria com `EAX=62` permanece como regressão, não como prova de
que o homebrew real passou.

## Passo 6 — framebuffer

**Objetivo:** apresentar a imagem produzida pelo homebrew executado.

Identificar o caminho real de `XVideoSetMode` e `debugPrint` na revisão nxdk
fixada e modelar os serviços/acessos de dispositivo necessários. Conectar o
framebuffer guest ao backend de apresentação, respeitando formato, dimensões,
stride e posse da memória. Preservar a segurança de memória Rust ao compartilhar
dados com renderização; a arena atual não pode simplesmente virar `Sync`.

**Aceite:** mostrar uma imagem originada das escritas reais do homebrew, com
evidência visual e execução controlável. Um texto desenhado diretamente pelo host
ou uma janela com imagem estática própria é só um teste de apresentação.
Controle, áudio, GPU completa e compatibilidade comercial seguem fora deste aceite.

## Como começar em outra máquina ou sessão

### Ambiente e referências

- Rust fixado em **1.98.1** por `rust-toolchain.toml`; seguir também
  [docs/dev/build.md](docs/dev/build.md) para linker/dependências do host.
- Último host validado: Windows x86_64 GNU + PowerShell; SDK compilado pelo WSL
  `FedoraLinux-44`. O caminho local era `C:\Users\Altenrath\emulador-xbox`.
  Ajustar caminhos e nome da distribuição em outra máquina.
- SDK, XBEs e logs em `target` são locais e **não chegam por git clone**.
  `--offline` nos comandos Cargo pressupõe dependências já baixadas; quando
  necessário, preparar a cache com `cargo fetch --locked`.
- Ler este guia, [resumo do alpha](docs/dev/alpha.md),
  [homebrew](docs/dev/homebrew.md), [arquitetura AOT](docs/arch/recompilacao.md)
  e [ROADMAP.md](ROADMAP.md). ADRs registram decisões históricas: ADR-0021 amplia
  o suporte a TLS/imports que ainda era recusado na etapa do ADR-0020.

### Reconstruir e inspecionar o alvo

Com as dependências do SDK instaladas conforme `docs/dev/homebrew.md`:

```sh
# Linux, a partir da raiz do projeto:
bash scripts/build-homebrew.sh
```

No Windows usado como referência:

```powershell
wsl -d FedoraLinux-44 -- bash /mnt/c/Users/Altenrath/emulador-xbox/scripts/build-homebrew.sh
cargo run --offline -p ordinal-report -- target/homebrew/hello.xbe
cargo run --offline -p obsidian -- check-memory target/homebrew/hello.xbe
cargo run --offline -p xberecomp -- translate target/homebrew/hello.xbe --entry 0x19110 --entry 0x145E0 --entry 0x205E0 --entry 0x20889 --entry 0x13C70 --entry 0x18FC0 --entry 0x18C90 --entry 0x139B0 --entry 0x14F14 --out target/alpha-hello
cargo run --release --offline --manifest-path target/alpha-hello/Cargo.toml -- --janela --fatias 4000
```

Com as nove raízes, o hello chega ao seu main e apresenta o framebuffer;
limites de tempo/fatias e fechamento da janela controlam a execução. Isso não
significa GPU completa ou compatibilidade comercial. Menos raízes podem
reproduzir bloqueios históricos já superados. A tradução exige
diretório de saída novo/vazio: se o nome já foi
usado, escolher outro sem apagar trabalho anterior.

### Verificar alterações de implementação

```sh
cargo fmt --all --check
cargo test --offline --workspace --features xbox-cpu/isa-oracle
cargo clippy --offline --workspace --all-targets --features xbox-cpu/isa-oracle -- -D warnings
git diff --check
```

O teste `cargo test --offline -p xberecomp --test native` gera um projeto em
`target/recomp-tests/<id>`, compila o Rust emitido e grava seus resultados.
Para repetir também em release, depois de gerar esse projeto, no PowerShell:

```powershell
$generated = Get-ChildItem target/recomp-tests -Directory | Sort-Object LastWriteTime -Descending | Select-Object -First 1
cargo test --offline --release --manifest-path (Join-Path $generated.FullName 'Cargo.toml') --target-dir target/recomp-native-check
```

Os 227 casos atuais em x86_64 não são um número fixo de aceite: adicionar os casos
necessários e atualizar o contador esperado do teste gerador ao ampliar a suíte.
Se alterar memória/unsafe, consultar também os contratos e Miri no guia de memória.
Mídia comercial é opcional; seu teste T14 fica ignorado na execução normal.

## Arquivos locais e cuidados de continuidade

O usuário mantém ISOs comerciais e `Complex_4627v1.03.bin` localmente, ignorados
pelo Git. **A BIOS não foi usada** nas implementações acima e não é necessária
para retomar este plano. Não presumir sua presença em outra máquina nem copiar
BIOS, jogos ou bytes proprietários para código, documentação ou fixtures.
O alvo inicial é o homebrew aberto; fontes públicas fixadas estão no guia nxdk.

Preservar alterações locais do mantenedor. Os commits anteriores foram feitos
pelo usuário após sugestões de mensagem; criar este guia não autoriza publicar,
fazer push ou modificar o remoto. Commits locais seguem AGENTS.md; o modo do
PLANO.md está em modo autônomo por instrução do mantenedor em 2026-10-23. Ao avançar cada entrega, atualizar este arquivo,
o resumo do alpha, o roadmap e a evidência de validação correspondente.

## Texto pronto para iniciar com outra LLM

> Continue pela passagem de bastão. Base 3e4b36c + ADR-0064; confira Git,
> AGENTS.md e PLANO.md, em modo autônomo. Tarefas 0.0/1.1/1.2 concluídas;
> próxima 1.3: consumidor de pushbuffer real. PFIFO guarda 37 controles e aliases
> USER canal DMA zero; GET não avança sozinho, EMPTY ainda sem modelo.
> BUS0 v7/PFFO v1; v1–v6 preservam PFIFO sem modelo. PRAMIN/P14 continua
> independente e boot D3D tem posse própria. KRNL v13/AOTC v8/AOTS v3 iguais.
> Triangle: 12 raízes/17984 blocos, framebuffer diagnóstico 1580 pixels/29 linhas,
> sonda 320 fatias em EIP=0x14D36, 93 chamadas/zero recusadas, MMIO 209 -> 170.
> PUT/GET saíram da lista sem modelo. Gate 1.99: 526 testes/227 nativos debug e
> release; Clippy 1.98.1 foi validado na 0.0. Hello 38/69, triangle 42/75 serviços.
> Não inventar GET/EMPTY; provisórios P1–P15. Commits locais conforme AGENTS.md.

### Texto anterior — histórico do ADR-0050, não usar como estado vigente

> Continue o Obsidian a partir de CONTINUIDADE_ALPHA.md. Confira o Git antes de
> alterar código. Base documentada: 0d77ba5 + alterações locais do ADR-0050.
> A entrega 1 tem aceite local; TLS/ABI e bootstrap de uma filha existem, mas
> faltam serviços/escalonamento para concluir o hello. Kernel: 27/69, 3/3 dados.
> Snapshot: AOTS v2, AOTC v7, KRNL v10, KMUT v3, THRD/TCXT v1, além de
> EEPR/PCIC/STIM/IRQL/VMEM/SYML/CMEM v1 e AVST v3. Clock/retired/perfil de CPU/IRQL únicos. Sem
> scheduler geral ou IRQ/APC/DPC; prefixo KTHREAD da filha termina em TlsData+0x28.
> NtAllocateVirtualMemory (184) atende só base nula/ZeroBits=0/flags=0x3000/PAGE_RW,
> em Passive. Reserva+commit privado, first-fit após a imagem inteira, base 64 KiB,
> tamanho 4 KiB, frames próprios zerados, validação integral/sem efeitos parciais.
> RW sem execução é política AOT. Outras variantes e NtFreeVirtualMemory faltam.
> Session registra o intervalo XBE; VMEM guarda regiões, Bus guarda RAM/MMU/frames.
> Snapshot confere posse/permissões/aliases/headers. KRNL v1–v5 migra sem alocações e
> Session recupera o intervalo do XBE correspondente; erros exigem descartar a sessão.
> FRM0 v1 agora valida bitmap/contagem/padding antes do commit. Nenhum unsafe novo.
> Hello: 10676 blocos; raízes 0x19110, 0x145E0, 0x205E0, 0x20889, 0x13C70,
> 0x18FC0, 0x18C90, 0x139B0 e 0x14F14. Passa por bootstrap e automount de D:.
> Namespace inicialmente sem D:, consulta/criação/open/close de links com handles
> reais e alvo copiado; não conecta mídia à VFS. Próximo bloqueio:
> DAC gamma, escrita de um byte zero em VA=0xFD6813C8, PC=0x12EEB, NaoMapeado.
> Framebuffer já alocado/zerado: 300 frames consecutivos,
> 1.228.800 bytes, VA=0x80050000/PA=0x50000. CMEM respeita limites/alinhamento,
> RW com WRITECOMBINE opcional, mapa restrito e snapshot; sem liberação ainda.
> AvSetDisplayMode configura NTSC-M 640×480/60 A8R8G8B8, pitch 2560, PA 0x50000,
> Step=0. Dispatcher::framebuffer empresta linhas BGRA por DMA sem padding.
> Encoder guarda pedidos 11/flicker=5 e 14/soften=false, sem aplicar filtro visual.
> AVST v3 acrescenta pedidos opcionais; v1/v2 migram sem pedidos, mantendo seus
> campos anteriores. Modo continua cruzando EEPROM/CMEM. Modelar DAC gamma por
> MMIO nos offsets 0x6813C8/0x6813C9 com índice/componente/tabela e snapshot;
> não mapear sobre o MMIO genérico que descarta escritas. Depois apresentar.
> Encoder já consulta EEPROM,
> usa cabo padrão (settings 0x00480101), guarda blanking e não herda framebuffer.
> Saída ainda desabilitada; sem texto desenhado ou imagem apresentada.
> Filha roda primeiro, pai estacionado; término noreturn/RET restaura o pai.
> Espera em filha terminada funciona; espera viva, yield/Sleep, preempção,
> múltiplas filhas, NtClose de mutant/thread, liberação e abandono ainda faltam.
> Não pular para main ou criar thread host. BIOS/mídia comercial não são necessárias.
> Use Rust 1.98.1. Registro atual: 398 testes workspace/doctests e 177 casos
> nativos debug/release. Execução completa/apresentação do framebuffer
> seguem pendentes. Preserve AOT/bus/estado explícito e atualize continuidade,
> alpha, roadmap, changelog e evidências ao avançar.
