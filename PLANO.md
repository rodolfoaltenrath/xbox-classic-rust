# Plano: alpha visual reproduzível no Windows

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

Bloco vigente: **Fase 4**, acrescentada em 2026-10-23 por autorização do mantenedor.
As fases 0–3 abaixo preservam o plano anterior concluído e suas evidências.
Meta: traduzir/reproduzir hello e triangle com o código atual neste Windows,
mostrar seus frames reais na janela e comprovar encerramento controlado.
Não inclui jogos comerciais, controles, áudio, texturas ou backend Linux.

Ponto de partida histórico (CONTINUIDADE_ALPHA.md, 2026-10-22): o triangle chega ao polling de
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

- [x] 3.1 P5: análise automática de jump tables (substitui raízes manuais do triangle).
  Evidência: GATE OK 566 testes (target/gate/20261023-023145.log), 233 casos
  gerados debug/release. Dez testes de guardas, aliases, tabelas/loops e callbacks;
  três nativos executam cases/default, replay e mutação real da RAM.
  Tradução sem --entry em target/plan-31-translate-final.log: 19707 blocos,
  incluindo os 14 endereços antes manuais. Sonda target/sonda/triangle-20261023-023144.log:
  282 chamadas/zero recusadas, 305922 pixels/479 linhas, mesmas cores/CPU da
  tradução manual; probe target/plan-31-real-probe.log confirma quadro idêntico
  e snapshot/replay integral. ADR-0069, P5 encerrado para o alvo.

- [x] 3.2 P3: escalonador de threads.
  Evidência: GATE OK 570 testes (target/gate/20261023-073340.log), 238 casos
  nativos debug/release. Delay bloqueia com outra Ready, dois deadlines, quantum,
  GPR/SSE/x87 e snapshot/replay; NtClose/detach mantém a filha viva e invalida
  somente o handle, THRD v1/v2 e AOTS v3/v4 migrados sem promoção de política.
  Sonda target/sonda/triangle-20261023-073247.log: 279 chamadas/zero recusadas,
  305922 pixels/479 linhas, 9 draws/27 triângulos. Probe
  target/plan-32-real-probe.log confirma snapshot/replay integral. ADR-0070,
  P3 encerrado para novas sessões; recorte de duas threads declarado em P19.
- [x] 3.3 P7: `KeTickCount` em tempo real.
  Evidência: GATE OK 572 testes (target/gate/20261023-074259.log), 242 casos
  gerados debug/release (target/plan-33-native-release.log). Polling sem HLE
  enxerga 1 ms dentro de run, REP parcial publica sem aposentar instrução,
  wrap ULONG em 2^32 ms e snapshot/replay integral; página continua RO guest.
  AOTC v9 migra v8 para BoundariesOnly e recusa perfil desconhecido.
  ADR-0071, P7 encerrado para sessões novas.
- [x] 3.4 P10/P11: DIRQL/IF/`STI` como máscara real.
  Evidência: GATE OK 576 testes (target/gate/20261023-081048.log), 253 casos
  gerados debug/release (target/plan-34-native-release-rep.log). Guest observa
  ISR=24/DPC=2, IF/CLI/STI e DIRQL mascaram entrega; interrupção dentro de
  bloco/loop e replay por EIP interno. REP após STI permite IRQ após uma
  iteração (ECX=2), com bytes e replay integral; correção fundamentada no xemu
  fixado. Migrações AOTC v9/v10, IRQL v1/v2 e TCTX v1/v2; política legada
  preservada. P19 recusa segunda espera pendente sem sobrescrever o waiter.
  Sonda target/sonda/triangle-20261023-080822.log: 278 chamadas/zero recusadas,
  305922 pixels/479 linhas, 9 draws/27 triângulos; probe
  target/plan-34-real-probe-final.log confirma snapshot/replay de CPU, RAM,
  GPU e scheduler. ADR-0072 e limites P20; plano concluído.

## Fase 4 — Alpha visual reproduzível no Windows

Autorização: mantenedor pediu o próximo bloco seguindo AGENTS.md/PLANO.md em
2026-10-23. Executar 4.1–4.3 sem confirmação entre tarefas. Preservar os aceites
abaixo e registrar bloqueios reais conforme AGENTS.md. Ao concluir 4.3, encerrar
este plano; outro bloco de compatibilidade depende de novo escopo.

- [x] 4.1 Reproduzir os samples e o gate no Windows sem depender dos projetos
  gerados no outro computador. Tornar a sonda configurável/reprodutível para
  projetos locais, preservando logs e propagando falhas de preparação/build.
  **Aceite:**
  - `bash scripts/gate.sh` termina em `GATE OK` no Rust fixado do Windows; não
    reduzir baseline, remover/ignorar testes ou ocultar divergências por plataforma;
  - hello e triangle são retraduzidos com o emissor atual, sem lista manual de
    raízes e sem sobrescrever projetos existentes; registrar SHA-256 dos XBEs;
  - `bash scripts/sonda.sh hello` e `bash scripts/sonda.sh triangle 320` executam
    os projetos desta máquina, com `recusadas: 0`, framebuffer habilitado e
    parada por orçamento; hello tem texto e triangle tem draws e regiões R/G/B;
  - erros de arquivo ausente, projeto inválido e compilação não podem aparecer
    como sonda bem-sucedida com métricas vazias; testes da ferramenta cobrem isso;
  - ADR novo, CHANGELOG e passagem de bastão distinguem evidência Windows da
    referência Linux e identificam os artefatos locais usados.

  Evidência: GATE OK: 580 testes (target/gate/20261023-152653-XlQbs6tT.log),
  253 casos gerados; seis testes de samples e quatro de gate no WSL. Traduções
  novas hello-eNjFLeSb (12345 blocos) e triangle-NBBSOjUk (19707), sem --entry.
  Hashes e perfis no ADR-0073. Sondas target/sonda/hello-20261023-151407-gMdXUwHB.log
  e triangle-20261023-151407-XlZPyXo0.log: orçamento, recusadas=0; hello 45 chamadas,
  302 pixels/12 linhas; triangle 278 chamadas, 305922/479, 9 draws/27 triângulos,
  R=4977/G=4998/B=4977. Build separado: hello 13m58, triangle 22m03; baseline
  preservado por SO (Windows580/Linux576), logs exclusivos e métricas obrigatórias.
  Commit intermediário 1029bb0; ADR/CHANGELOG/passagem de bastão atualizados.

- [~] 4.2 Validar apresentação e encerramento do alpha real. Corrigir problemas
  observados no template/laço de apresentação mantendo CPU/Clock guest isolados
  da temporização do host e a posse exclusiva da arena.
  **Aceite:**
  - `GATE OK`, com testes que exercitam o caminho efetivamente corrigido;
  - executar hello e triangle com `--janela` no Windows; capturar a área cliente
    640×480 pelo sistema, registrar contagens de pixels/cores e comparar os
    canais RGB com os bytes do mesmo framebuffer guest, sem imagem substituta;
  - o botão fechar/WM_CLOSE durante execução longa encerra o processo em até
    cinco segundos, com saída de parada pelo host e sem retomar execução/janela;
  - fechamento após esgotar o orçamento também encerra; artefatos e logs da
    verificação ficam em target, com procedimento reproduzível versionado;
  - snapshot/replay e execução sem janela permanecem passando; documentar
    limites de P2/P6/P18 e não declarar fidelidade analógica ou compatibilidade geral;
  - ADR novo, CHANGELOG, ROADMAP §16.1 e passagem de bastão atualizados.

- [ ] 4.3 Entregar um comando de uso do alpha. Preparar/traduzir/compilar os
  samples locais e abrir a janela com opções claras, sem exigir editar caminhos
  do outro PC ou reconstruir manualmente comandos de target.
  **Aceite:**
  - `GATE OK` e testes significativos de argumentos/erros da ferramenta;
  - um comando documentado por sample abre a janela usando as entregas 4.1/4.2;
    aceitar caminhos com espaços, preservar logs/artefatos e recusar entrada
    inexistente ou configuração inválida sem sucesso silencioso;
  - preparo/build e execução têm relatos separados; não atribuir tempo de
    compilação à velocidade do guest nem anunciar suporte a jogos comerciais;
  - README e docs/dev/alpha.md trazem pré-requisitos, comandos, encerramento,
    limitações atuais e localização dos logs; atualizar CHANGELOG e passagem
    de bastão com evidências finais e manter o histórico das fases 0–3.
