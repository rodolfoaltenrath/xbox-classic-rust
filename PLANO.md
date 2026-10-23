# Plano: do polling de DMA_GET ao triângulo na tela

Modo: autônomo

<!--
Para os agentes trabalharem sem parar, troque a linha acima por "Modo: autônomo".
Para interromper, volte para "Modo: pausado".
Regras de execução, commits e quando parar: AGENTS.md.

Marcas: [ ] aberta · [~] em andamento · [x] concluída (com linha "Evidência:")
Uma tarefa bloqueada recebe "BLOQUEADO: <motivo>" e o trabalho para ali.
Só o mantenedor altera o texto dos critérios de "Aceite".

Ferramentas de aceite:
  bash scripts/gate.sh             -> última linha "GATE OK: N testes" (N >= baseline)
  bash scripts/sonda.sh triangle   -> bloco "== RESUMO" com parada, chamadas,
                                      recusadas, framebuffer, mmio_sem_modelo, toca <addr>
"Todo ADR" = próximo número livre em docs/adr/, no formato do ROADMAP §12.4.
-->

Ponto de partida (CONTINUIDADE_ALPHA.md, 2026-10-22): o triangle chega ao polling de
NV_USER DMA_GET (0xFD800044, sem modelo), DMA_GET fica em zero por 12402025 leituras,
pb_init/pb_kill expiram e a execução para no ordinal 171 (MmFreeContiguousMemory),
com 81 chamadas e 1 recusada. O framebuffer mostra o diagnóstico (1580 pixels/29
linhas), sem triângulo.

## Fase 0 — Liberar memória contígua (ordinal 171)

- [x] 0.0 Gate verde no toolchain desta máquina. O `cargo` do Fedora (1.99, sem rustup)
  ignora o `rust-toolchain.toml` (1.98.1), e o clippy 1.99 traz o lint
  `assert_is_empty`, que falha em 13 pontos (`xdvdfs.rs:516` e testes do
  xbox-kernel: boot_dma, contiguous, gpu_instance, threads, virtual_memory).
  Corrigir com `assert_eq!`/`assert_ne!` de forma que compile nos dois toolchains,
  sem `#[allow]`.
  **Aceite:** `GATE OK` com cargo 1.99; nenhum `#[allow(clippy::assert_is_empty)]`
  novo no código.
  Evidência: Rust/Cargo 1.99.0, `GATE OK: 519 testes` em
  target/gate/20261023-001813.log; 13 asserções ajustadas sem allow; Clippy
  workspace/all-targets também passa com Rust 1.98.1 (target/plan-00-clippy198.log).
  Modo autônomo retomado por instrução do mantenedor em 2026-10-23.

- [x] 0.1 Implementar `MmFreeContiguousMemory` liberando a alocação real no
  `contiguous`, recusando endereço desconhecido, o framebuffer e a instância GPU
  (trabalho já em andamento na árvore: `dispatch/contiguous.rs`, `tests/free_contiguous.rs`).
  **Aceite:**
  - `GATE OK`;
  - `crates/xbox-kernel/tests/free_contiguous.rs` cobre: liberação válida, endereço
    desconhecido, framebuffer, instância GPU, e snapshot com versão nova e teste de
    migração, se o formato mudou;
  - sonda triangle: `recusadas: 0` e `parada` deixa de citar o ordinal 171;
  - ADR novo e entrada no CHANGELOG; contagem "triangle N/75" atualizada no
    CONTINUIDADE_ALPHA.md.

  Evidência: `GATE OK: 519 testes` com Rust 1.98.1 (target/gate/20261023-001255.log);
  seis testes em free_contiguous.rs e três novos casos gerados, 224 debug/release;
  sonda triangle 320: recusadas=0, chamadas=93, parada por orçamento em 0x14D36,
  sem ordinal 171 (target/sonda/triangle-20261023-001300.log). Sonda de recursos:
  quatro alocações/133 frames liberados, boot/framebuffer/pool GPU preservados,
  snapshot/replay integral estável (target/free-triangle-probe.log). Formatos
  mantidos; ADR-0062/CHANGELOG/CONTINUIDADE atualizados, triangle 42/75.
  A tarefa 0.0 (Clippy 1.99) continua aberta; o modo do plano permanece pausado.

## Fase 1 — PFIFO e pushbuffer com efeito real

- [x] 1.1 Inventário e evidência, sem código de emulação. Listar todo registrador
  PFIFO/NV_USER/PRAMIN que o triangle toca entre pb_init e o polling, com a fonte
  pública de cada um (como no ADR-0056) e o valor que o guest escreve.
  **Aceite:**
  - ADR novo com a tabela registrador → fonte → valor observado → consumidor no pbkit;
  - a tabela cita o log da sonda usado (caminho em `target/sonda/`);
  - define quais registradores a tarefa 1.2 modela e quais continuam lendo zero
    declarado.
  Evidência: ADR-0063 lista 37 controles PFIFO, dois endereços USER e os 5120
  DWORDs PRAMIN por faixa/estrutura; captura de 12183 acessos PRAMIN em
  target/sonda/triangle-pfifo-inventory-20261023.log, até o primeiro GET,
  54 chamadas/zero recusadas. Sonda oficial target/sonda/triangle-20261023-002421.log:
  93 chamadas/zero recusadas, 209 MMIO sem modelo, 1580 pixels/29 linhas.
  GATE OK: 519 testes (target/gate/20261023-002549.log). Sem emulação nova.

- [x] 1.2 Modelar os registradores escolhidos em 1.1 (DMA_PUT, DMA_GET, estado do
  canal e o que mais o ADR listar) com leitura/escrita persistente no `Bus`.
  **Aceite:**
  - `GATE OK`;
  - sonda triangle: a linha `toca 0xFD800040: sem modelo` e a linha
    `toca 0xFD800044: sem modelo` não aparecem mais;
  - `mmio_sem_modelo` da sonda menor que o valor medido antes da tarefa (anotar os
    dois números na Evidência);
  - snapshot BUS0 com versão nova e teste de migração da versão anterior;
  - provisórios novos declarados no ROADMAP §16.1 com condição de saída.

  Evidência: GATE OK: 526 testes (target/gate/20261023-003811.log); 227 casos
  gerados debug/release. Sonda target/sonda/triangle-20261023-003811.log, 320 fatias:
  MMIO sem modelo 209 -> 170, sem as linhas toca PUT/GET; 93 chamadas/zero recusadas,
  EIP=0x14D36, framebuffer 1580 pixels/29 linhas. BUS0 v7/PFFO v1; sete testes de
  memória e três nativos cobrem aliases, preflight, replay e migração v6.
  ADR-0064/P15: controles persistem, GET não avança sem consumo; EMPTY sem modelo.

- [x] 1.3 Consumir o pushbuffer: ler comandos entre GET e PUT pelos descritores de
  DMA na PRAMIN, decodificar cabeçalhos (incremento, não incremento, jump, call/return
  se o triangle usar) e avançar DMA_GET conforme consome. Métodos sem efeito ainda
  ficam registrados na telemetria, nunca descartados em silêncio.
  **Aceite:**
  - `GATE OK`;
  - testes unitários com pushbuffer sintético para cada tipo de cabeçalho usado e
    para um descritor inválido, que deve ser recusado;
  - sonda triangle: o polling termina por GET == PUT, não por timeout. `parada` não
    vem de pb_kill, e o log lista os métodos NV097 recebidos (classe, método, contagem);
  - ADR novo ou adendo ao de 1.1 explicando a origem dos descritores.

  Evidência: GATE OK 535 testes (target/gate/20261023-010738.log), 230 casos
  gerados debug/release. Sonda de bootstrap target/sonda/triangle-dma-root13-probe-20261023.log:
  GET=PUT=0x18C000 após consumir jump, retired=25749621, 54 chamadas/zero recusadas,
  snapshot integral estável. Sonda oficial target/sonda/triangle-20261023-010537.log:
  66 chamadas/zero recusadas, parada UnknownTarget 0x1E9B6 -> 0x1E9B8 fora de pb_kill;
  257 chaves/376 parâmetros, NV097 com 249 chaves/366 parâmetros (inclui binding).
  Métodos sem efeito gráfico são relatados. ADR-0065/BUS0 v8/PFFO v2; 13 raízes/
  17997 blocos, acrescida 0x196CF para o switch de cor de 32 bits. P16 declara limites.

- [x] 1.4 Estado do PFIFO no snapshot no meio do pushbuffer.
  **Aceite:**
  - `GATE OK`;
  - teste que salva o snapshot com comandos pendentes, restaura e obtém o mesmo
    DMA_GET final e a mesma lista de métodos de uma execução sem interrupção.

  Evidência: GATE OK: 538 testes (target/gate/20261023-011523.log). Testes
  snapshot_no_meio_do_pacote_e_dos_parametros_reproduz_get_e_lista_de_metodos e
  snapshot_com_call_ativo_e_objeto_ligado_reproduz_return_e_parametros comparam
  GET, registros de métodos e snapshot integral com execução contínua; terceiro
  teste recusa PCMD/bindings/metadados corrompidos antes de publicar PFIFO.
  Adendo ADR-0065; formatos mantidos, 230 casos gerados permanecem passando.

## Fase 2 — PGRAPH mínimo (NV097) para o triangle

- [x] 2.1 Inventário dos métodos NV097 recebidos (saída da 1.3) e escolha do recorte
  mínimo para limpar a tela e desenhar.
  **Aceite:** ADR novo com método → fonte → efeito, e a lista dos que ficam sem efeito
  declarado.

  Evidência: ADR-0066 + docs/compat/triangle-nv097-20261023.csv, 274 chaves NV097
  com fontes, valores e efeito planejado; sonda target/sonda/triangle-20261023-012028.log,
  14 raízes/19391 blocos, 226 chamadas/zero recusadas, 40 DRAW_ARRAYS de nove vértices.
  Gate 538 testes (target/gate/20261023-012445.log). Captura observacional confirma
  oito instruções de vertex program (MOV/MUL/ADD/MAD/RCP), sem copiar bytecode.
  IRQ de NOP/descritores e scanout fazem parte do recorte necessário para 2.2;
  efeitos gráficos ainda não implementados, framebuffer preto.

- [x] 2.2 Configuração de surface e `CLEAR_SURFACE` escrevendo no framebuffer guest.
  **Aceite:**
  - `GATE OK`;
  - teste unitário: clear de uma surface pequena produz a cor e o retângulo pedidos;
  - sonda triangle: a linha `framebuffer` muda em relação aos 1580 pixels/29 linhas do
    diagnóstico, de acordo com a cor de clear que o triangle envia (citar o valor na
    Evidência).

  Evidência: GATE OK 547 testes (target/gate/20261023-014538.log), 230 casos
  gerados debug/release; clear_retangulo_inclusivo_preserva_vizinhos_padding_e_mascaras_de_cor
  e oito testes de limites/trap/replay/migração/scanout. Sonda triangle 320:
  target/sonda/triangle-20261023-014055.log, 282 chamadas/zero recusadas, framebuffer
  1542 pixels/32 linhas (diagnóstico 1580/29); primeira cor de clear 0x00000000,
  1830 clears. Probe target/plan-22-real-clear-probe.log confirma 307200 DWORDs
  zerados na base 0x33C000 pelo descritor 0x11150 atualizado pelo ISR guest,
  snapshot/replay integral estável. BUS0 v9/PGRF v1, perfil público P17/ADR-0067.

- [x] 2.3 Vértices (inline ou arrays, o que o triangle usar) e rasterização em software
  de um triângulo de cor interpolada no framebuffer guest.
  **Aceite:**
  - `GATE OK`;
  - teste golden: framebuffer de um triângulo sintético comparado com uma imagem de
    referência versionada em `tests/`;
  - sonda triangle: pixels coloridos acima do valor do clear e presença das três
    cores dos vértices (anotar contagem e cores na Evidência);
  - CONTINUIDADE_ALPHA.md "Passagem de bastão" reescrita com o novo estado.

  Evidência: GATE OK 556 testes (target/gate/20261023-020429.log), 230 casos
  gerados debug/release. Golden arrays_vertex_program em tests/reference/triangle-4x4.ppm
  (seis pixels calculados analiticamente), mais oito testes de VP/configuração,
  erro atômico, limites, snapshot/replay, corrupção e migração ClearV1.
  Sonda triangle 320 target/sonda/triangle-20261023-020000.log: 282 chamadas/zero
  recusadas, 305922 pixels/479 linhas (clear+texto 1542/32), clear 0x00000000;
  regiões R=4977/G=4998/B=4977 (canal >=192, outros <=32), dez draws/30 triângulos.
  Probe target/plan-23-real-raster-probe.log: scanout 0x210000, captura PPM/PNG e
  replay integral byte a byte. PGRF v2/PRND v1 preserva perfis legados;
  ADR-0068/P18, passagem de bastão reescrita.

## Fase 3 — Backlog (só depois das fases 0 a 2)

Cada item termina quando a **condição de saída** escrita para ele no ROADMAP §16.1 for
atendida, com `GATE OK`, ADR e CHANGELOG.

- [ ] 3.1 P5: análise automática de jump tables (substitui raízes manuais do triangle).
- [ ] 3.2 P3: escalonador de threads.
- [ ] 3.3 P7: `KeTickCount` em tempo real.
- [ ] 3.4 P10/P11: DIRQL/IF/`STI` como máscara real.
