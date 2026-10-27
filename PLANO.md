# Plano: leitura síncrona do executável pela mídia guest

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

Bloco vigente: **Fase 7 concluída**, acrescentada em 2026-10-26 ao pedido de prosseguimento.
As fases 0–6 preservam as entregas concluídas e suas evidências. Meta: o guest
abrir e ler os bytes originais de D:\default.xbe por I/O síncrona, com handles,
EOF e posições reproduzíveis. Validar com um homebrew próprio compilado pelo
nxdk fixado. Não inclui escrita, diretórios, ISO, arquivos arbitrários do host,
I/O assíncrona ou a promessa de filesystem completo.

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

- [x] 4.2 Validar apresentação e encerramento do alpha real. Corrigir problemas
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

  Evidência: GATE OK: 580 testes (target/gate/20261023-154903-qFHAXziu.log),
  257 casos gerados debug/release com -D warnings; parser/arquivo real/HLT+INT3
  com captura indisponível. verify-window.ps1 compara RGB do DC com guest.bgra:
  zero diferenças em ambos, hello 302/12 e triangle 305922/479 (R4977/G4998/B4977).
  Relatórios em target/visual-{hello,triangle}-{verified,during}/: fechamento
  durante=191,1977/173,9636 ms, status0/host sem janela congelada; após orçamento
  =95,7878/52,2096 ms. Sondas headless repetem CPU/pixels da 4.1, sem recusas;
  snapshots/replay passam. ADR-0074 documenta os comandos, hashes e limites.

- [x] 4.3 Entregar um comando de uso do alpha. Preparar/traduzir/compilar os
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

  Evidência: GATE OK: 580 testes (target/gate/20261023-162456-sLpqiXCh.log),
  257 gerados; seis testes novos de launcher, seis de preparo/sonda e quatro
  de gate (16 ferramentas, separados). run-alpha.sh hello e triangle abriram
  as janelas sem --projeto: runs hello-XfFakWTu/triangle-mM890OSS, mesmo BGRA do
  ADR-0074, X em 58,3149/54,859 ms e status 1/orçamento preservado; Cargo
  reutilizado em 0,20/0,23 s. Entrada e estado com espaços em
  target/alpha spaces/runs/hello-CU9eCXwu, mesmo hash do hello. Cache compara
  seis arquivos com a tradução/lock atuais, preserva projetos/logs e arquiva
  índice inválido após rebuild aprovado. ADR-0075, README, alpha, CHANGELOG,
  ROADMAP e passagem de bastão atualizados. Fase 4 e plano concluídos.

## Fase 5 — Alpha contínua e retomável

- [x] 5.1 Arquivos de estado completos e vinculados ao XBE. Criar contêiner
  versionado com integridade, limite de tamanho, recusa de imagem divergente e
  restauração em sessão nova; publicar arquivo sem sobrescrever outro. Expor
  `--salvar` e `--carregar` no executável gerado.

  **Aceite:** gate verde; testes de ida/volta CPU/RAM/kernel, replay, imagem
  divergente, corrupção, truncamento, versão futura, bytes excedentes e arquivo
  existente; testes no main realmente gerado salvam/carregam e preservam a
  causa original de HLT/INT3 se salvar falhar. Snapshots internos antigos
  continuam com seus formatos/migrações. ADR, CHANGELOG e continuidade atualizados.

  Evidência: `GATE OK: 583 testes`, target/gate/20261023-164712-eg76sf5k.log;
  259 casos nativos/243 fora de x86-64. Três testes checkpoint validam CRC,
  integridade/versões/limites e publicação; `arquivo_de_estado_retoma_call_real_e_replay_integral`
  compara sessão completa após CALL pendente. Main salvo/carregado em processos
  distintos avança CPU/Clock; HLT/INT3 preservados com falha de gravação. ADR-0076.

- [x] 5.2 Execução contínua e comandos de sessão no host. `--continuo` com janela
  repete fatias finitas sem reiniciar a sessão; Espaço pausa/retoma, F5 salva
  geração nova em `--estados`, F9 restaura a última geração desta execução.
  Fechamento tem prioridade, repetição automática da tecla é ignorada; os
  comandos não são enviados ao controle Xbox. Sem progresso guest durante pausa.

  **Aceite:** gate verde; testes exercitam pausa sem avanço CPU/Clock, retomada,
  falha de restauração mantendo a sessão anterior e recusa de combinação inválida
  de argumentos; eventos Win32 testados; nenhum erro guest diferente de orçamento
  é tratado como continuação. ADR, CHANGELOG, ROADMAP e continuidade atualizados.

  Evidência: `GATE OK: 586 testes`, target/gate/20261023-165351-qTfDPCiI.log;
  261 casos gerados/245 fora de x86-64. `pausa_nao_executa_guest_e_continuacao_so_aceita_orcamento`,
  `geracoes_nao_substituem_arquivo_e_restore_falhado_preserva_sessao` e
  `atalhos_preservam_ordem_ignoram_autorepeticao_e_close_tem_prioridade` passando;
  `controle_continuo_pausa_codigo_nativo_e_retoma_sem_perder_clock` conclui demo
  real com EAX=62/37 instruções após pausa sem mudança de snapshot. ADR-0077.

- [x] 5.3 Integrar o iniciador e validar o fluxo real. Encaminhar modo contínuo,
  restauração e saída de estado pelo comando do alpha, mantendo execução finita
  padrão e logs separados. Automatizar os atalhos contra uma janela real e
  documentar os comandos.

  **Aceite:** gate verde e sonda hello/triangle com zero recusas e os pixels de
  referência; no Windows, hello e triangle continuam além do orçamento padrão,
  pausam, salvam, avançam e restauram CPU/Clock/RAM/GPU/kernel, sem avanço durante
  pausa; fechamento durante execução e pausa em até 5 s. Testes de launcher
  passam; README, docs/dev/alpha.md, ADR, CHANGELOG, ROADMAP e passagem de bastão
  registram evidências e limitações. Commit por tarefa e nenhum push.

  Evidência: `GATE OK: 586 testes`, target/gate/20261026-101417-ic6bJkvN.log;
  261 casos gerados debug e release (release: target/phase5-native-release-final.log).
  `TOOLS OK`, 19 casos (9 launcher, 6 preparo/sonda, 4 gate),
  target/phase5-tools-20261026.log. Sondas de 26/10:
  hello-20261026-101636-4TGqAGqT.log = 45/0, 302 pixels/12 linhas;
  triangle-20261026-101800-pq23VpMD.log = 278/0, 305922/479,
  R4977/G4998/B4977, nove draws/27 triângulos. Hello validado em
  target/session-hello-phase5/ (23/10): pause/restore/erro recuperável e outro
  processo; WM_CLOSE pausado/executando 593,7587/722,6875 ms. Triangle em
  target/session-triangle-phase5-isolated-20261026/: mesmos quatro cenários,
  snapshot integral idêntico antes/depois, 508359498 -> 785161898 instruções
  antes de restaurar; close 2082,6871/2421,4008 ms. Progresso usou o parâmetro
  existente TimeoutSeconds=300, sem alterar o prazo de close de 5 s; tentativa
  concorrente com build atingiu 120 s e não foi usada como aceite.
  Readback RGB: zero diferenças em ambos, pastas session-hello-frame-phase5/
  e session-triangle-frame-phase5-20261026/. Comando público com
  --continuo --carregar validado: hello-LAfc6doM (alpha-live-smoke-hello) e
  triangle-LqNrxSMj (alpha/runs), status 0, final.obsstate preservado; Cargo
  reutilizado em 0,42/1,46 s. ADR-0078, README, alpha, CHANGELOG, ROADMAP e
  passagem de bastão atualizados. Fases 0–5 concluídas.

## Fase 6 — Gamma do NV2A e fades reais

- [x] 6.1 Modelar o DAC observado em MMIO. Documentar consumidor nxdk e referência
  pública fixada; implementar índice, componentes RGB de 8 bits e avanço do
  cursor, com larguras recusadas antes de efeitos. BUS0 novo e seção própria
  versionada, preservando a ausência do modelo em snapshots antigos.

  **Aceite:** GATE OK; testes de identidade, ordem RGB, programação parcial,
  reset de índice, limite/wrap do perfil público, aliases físicos/virtuais,
  larguras inválidas sem efeito, snapshot entre componentes, corrupção e
  migração de BUS0 v9 e anteriores sem ativação implícita. Atualizar fixtures
  legadas pelo layout real, preservando asserções. ADR, CHANGELOG, ROADMAP
  §16.1 e passagem de bastão registram fonte, incertezas e limites.

  Evidência: `GATE OK: 592 testes`, target/gate/20261026-104736-VwWbcpTU.log;
  261 casos gerados. Seis testes em xbox-mem/tests/gamma.rs cobrem RGB/alias,
  preflight, wrap, replay parcial, corrupção e BUS0 v9. Migrações v1/v2 também
  conferem LegacyUnmodeled; fixtures legadas nativas preservam seus contratos.
  Teste histórico de MMIO sem modelo mantém asserções em portas vizinhas.
  BUS0 v10/GAMM v1, ADR-0079; fontes fixadas e incertezas registradas em P21.

- [x] 6.2 Aplicar a rampa à apresentação, preservando a memória guest. Manter
  --quadro como BGRA bruto e oferecer exportação separada da saída apresentada.
  Nenhum título ou shader reconhecido por nome/hash; nenhum empréstimo da arena
  ao backend de janela. Snapshot antigo continua com a apresentação legada.

  **Aceite:** GATE OK; Rust realmente gerado comprova RAM inalterada e saída
  transformada por rampa não identidade, com alfa preservado, canais corretos,
  exportação sem sobrescrita, replay e restore legado. Identidade mantém as
  imagens de hello/triangle; testes de opções/exportação passam. ADR, CHANGELOG,
  docs/dev/alpha.md e continuidade explicam captura bruta versus saída do DAC.

  Evidência: `GATE OK: 593 testes`, target/gate/20261026-105644-WDLEgZ1e.log;
  265 casos gerados/249 fora de x86-64. `gamma_verification` comprova transformação,
  RAM/alfa intactos, replay parcial e restore BUS0 v9; `check_gamma_export`
  executa main emitido e verifica os dois arquivos e recusa de sobrescrita.
  Quadro testa identidade em todos os 256 níveis de cada canal: conserva cada
  byte RGB das imagens de referência; revalidação dos homebrews reais na 6.3.
  Opções de saída independentes/repetidas/incompletas cobertas; ADR-0080.

- [x] 6.3 Entregar gamma-fade como terceiro homebrew reproduzível. Integrar
  preparo/sonda/launcher, com build nxdk fixado e fontes fora do Git. Resolver
  somente bloqueios concretos de tradução/kernel encontrados no alvo, sem
  sucesso artificial. Preservar modo contínuo e estados da fase anterior.

  **Aceite:** GATE OK e testes de ferramentas; sondas hello/triangle mantêm zero
  recusas e imagens de referência. Gamma-fade executa pelo menos uma transição
  real de rampa além da identidade, com zero serviços recusados; janela Win32
  coincide com a saída exportada do DAC e difere da RAM bruta nesse ponto.
  Save/restore com rampa não identidade preserva saída e continuação. README,
  ADR, CHANGELOG, ROADMAP §16.1 e passagem de bastão registram comandos,
  métricas, hashes e limites. Commit por tarefa, nenhum push.

  Evidência: `GATE OK: 595 testes`, target/gate/20261026-111413-Waq1DjEq.log;
  266 casos gerados debug/release, native-release-final.log; 21 ferramentas,
  TOOLS OK em target/gamma-phase6/tools-final-source.log. Hello/64: 45/zero,
  302 pixels/12 linhas, log hello-20261026-111845-AhleASQU.log. Triangle/320:
  278/zero, 305922/479, R4977/G4998/B4977, nove draws/27 triângulos,
  log triangle-20261026-111855-dp7h9SQX.log. Gamma-fade/128: 161/zero,
  255 entradas não identidade, 299222 pixels RAM, zero MMIO sem modelo;
  log gamma-fade-20261026-112938-2cwi0vUS.log. Readback Win32 release:
  zero diferenças RGB contra DAC e SHA distinto da RAM; visual-gamma-release/.
  Session-gamma-release/: pausa/restauração integral idêntica, 26646187 ->
  30056597 instruções antes de F9, restore inválido preservado e outro processo
  continua; close com save 1160,5467/959,7528 ms. Duas continuações e replay
  release mantêm snapshot SHA e650acbc...611f55 e imagem 22cc6a75...7b0839.
  Comando público contínuo/carregar validado: gamma-fade-SMskuCZc, host/status0,
  close 804,7775 ms, final.obsstate; todas as pastas acima em target/gamma-phase6
  salvo runs/sonda. ADRs 0081–0082, fontes/hashes/limites documentados.

## Fase 7 — Arquivo de mídia e leitura síncrona

- [x] 7.1 Kernel de arquivo somente leitura. Perfil explícito do executável
  imutável, NtCreateFile/NtReadFile/NtQueryInformationFile e NtClose, usando o
  mesmo alocador de handles dos outros objetos. KRNL novo/CDFS versionado;
  versões antigas preservam ausência de mídia, sem promovê-las ao restaurar.

  **Aceite:** GATE OK; testes de bytes reais, opens independentes, EOF/leitura
  curta, offsets explícitos/correntes, informações suportadas, fechamento,
  namespace/case, parâmetros inválidos e variantes recusadas. Preflight impede
  escritas parciais e consumo de posição/handle em falhas. Snapshots conferem
  corrupção/limites/colisões e migração KRNL v13/anteriores sem ativação.
  Fixtures legadas preservam suas asserções. ADR, CHANGELOG, ROADMAP §16.1
  e continuidade declaram o recorte e as fontes do ABI Xbox/contrato NT.

  Evidência: `GATE OK: 606 testes`, target/gate/20261026-121358-goLFnxPD.log;
  266 casos gerados. Onze testes novos em xbox-kernel/tests/files.rs verificam
  bytes/EOF/cursor, namespace/share, preflight e aliases físicos, corrupção,
  limites, colisão com mutant e migração KRNL v13. GINS mantém as asserções por
  tag; fixture nativa v12 remove CDFS pelo layout. ADR-0083/P22. O produtor
  nxdk compila com asserts de ABI e traduz 12982 blocos sem --entry; montagem
  automática ainda é 7.2, execução real do produtor será validada em 7.3.

- [x] 7.2 Integrar a mídia à sessão e ao Rust gerado. Sessão nova publica o
  XBE original como default.xbe; restauração usa somente a mídia serializada.
  Leituras não usam bytes modificados da RAM nem arquivos abertos no host.

  **Aceite:** GATE OK; caso compilado do Rust emitido abre/lê/consulta/fecha
  com ABI correta, retorna bytes do XBE e reproduz continuação desde handle
  aberto. Recusa preserva CPU e efeitos guest; snapshots legados continuam
  sem mídia. Replay não depende de arquivo host. ADR e documentação atualizados.

  Evidência: `GATE OK: 607 testes`, target/gate/20261026-123910-iFilAuiI.log;
  269 casos gerados. files_verification confere stdcall, bytes originais com
  headers RAM alterados, handle/cursor/replay integral e recusa com CPU intacta;
  KRNL v13 continua sem mídia. SAVE recusa mídia divergente com CRC válido.
  ADR-0084; comparações integrais legadas preservadas com ausência explícita.

- [x] 7.3 Homebrew file-read e fluxo público. Fonte de teste própria no Git,
  build isolado com SDK fixado, preparo/sonda/launcher para o quarto alvo.
  Exercitar CreateFileA/ReadFile/GetFileSizeEx/CloseHandle e exibir resultado
  calculado dos bytes lidos, com erro observável para qualquer divergência.

  **Aceite:** GATE OK e testes das ferramentas; sonda file-read com zero
  recusas e diagnóstico de leitura válida no framebuffer real, conferido
  contra o XBE do build. Janela Win32 corresponde à exportação. Estado salvo
  com arquivo aberto restaura mídia/posição e continua de forma determinística.
  Sondas hello/triangle/gamma-fade preservam zero recusas e referências de
  pixels/rampa. README, ADR, CHANGELOG, ROADMAP e passagem de bastão contêm
  comandos, evidências e limites; commit por tarefa e nenhum push.

  Evidência: `GATE OK: 607 testes`, target/gate/20261026-131313-X96CyqRI.log;
  269 gerados debug/release com -D warnings, 26 ferramentas/TOOLS OK e três
  regressões PowerShell/SESSION LOG OK. Produtor nxdk: 12982 blocos sem --entry,
  XBE SHA 72580ecc...6eb5e, bytes=102400/fnv=9C261314 conferidos na captura real.
  Sonda file-read/512: 493/zero recusas, 1810 pixels/39 linhas, zero MMIO;
  file-read-20261026-130946-K5t5oU7W.log. Win32 visual-final: zero diferenças
  RGB, close 82,5002 ms. Session-verified: pausa/F9 com arquivo inteiro idêntico,
  falha de restore preservada, outro processo continua; close 770,2522/621,276 ms.
  CDFS tem mídia exata e handle 48/posição 38912; replay-g/h chega a 43008 e mantém
  snapshot SHA 94e05311...f10c31 e imagens idênticos. Pastas em target/file-read-phase7.
  Launcher contínuo/carregar: file-read-TPl4J4VX, host/status0, final salvo,
  close 767,5781 ms. Hello/64 mantém 45/0/302 pixels; triangle/320 mantém 278/0,
  305922/479/R4977/G4998/B4977/9 draws/27 triângulos; gamma/128 mantém 161/0,
  299222/480/gamma 255. Logs hello-20261026-130638-grgtSC01,
  triangle-20261026-131338-HRtFMUsM e gamma-fade-20261026-131338-oZR5OYsA.
  ADR-0085 registra fontes/hashes/recortes e corrida corrigida no polling de logs.
  Fases 0–7 concluídas; três commits locais, sem push.

## Fase 8 — Apresentação no Linux desktop (P6, parte desktop)

Motivo: o mantenedor desenvolve no Fedora (GNOME/Wayland com XWayland) e hoje a
janela só existe no Windows. A Fase 10 (DRM/KMS) continua fora deste recorte.

- [x] 8.1 Backend X11 em `xbox-platform`, Rust puro sobre socket Unix (protocolo
  core, autenticação MIT-MAGIC-COOKIE-1 do `XAUTHORITY`), atrás do mesmo
  `Apresentador`. Sem dependência nova e sem `unsafe`. PutImage em faixas que
  respeitam o tamanho máximo de requisição; WM_DELETE_WINDOW → `Fechar`;
  Espaço/F5/F9 via mapa de teclado do servidor; Expose repinta o último quadro.
  Sem DISPLAY ou sem servidor X: erro explicando a lacuna, nunca janela fictícia.
  **Aceite:**
  - `GATE OK`;
  - testes unitários (sem servidor) para: parse de Xauthority, codificação das
    requisições usadas, divisão de PutImage em faixas, decodificação de eventos
    (ClientMessage, KeyPress com mapa sintético, Expose, erro do servidor);
  - teste do `criar_janela` fora do Windows atualizado: sem `DISPLAY` o erro cita a
    variável; o teste antigo "Fases 8 e 10" deixa de valer só onde há backend;
  - ADR novo; P6 no ROADMAP §16.1 reescrito (desktop Linux via X11/XWayland
    entregue; DRM/KMS e Wayland nativo continuam pendentes).

  Evidência: `GATE OK: 621 testes` (baseline Linux 602 -> 621),
  target/gate/20261026-212858-CgETsn5X.log. 21 testes de protocolo sem servidor
  (Xauthority, setup aceito/recusado/truncado/visual, requisições, faixas de
  PutImage 102 linhas, pacotes, mapa de teclado, autorrepetição, Expose, fila) e
  3 de erro do `Janela::nova_em` (sem DISPLAY cita a variável e a Fase 10;
  remoto; socket inexistente). Teste real opt-in `OBSIDIAN_X11_REAL=1` no
  Xwayland do Fedora 44: GetImage de outra conexão com zero pixels RGB
  diferentes; Espaço/F5/F9 em ordem; WM_DELETE_WINDOW -> Fechar. ADR-0086; P6
  reescrito; teste "Fases 8 e 10" restrito a sistemas sem backend.

- [x] 8.2 Verificação visual real no Linux, equivalente ao `verify-window.ps1`.
  Ferramenta que localiza a janela do Obsidian por outra conexão X, captura o
  conteúdo com GetImage, compara com a exportação `--quadro-apresentado` e
  envia WM_DELETE_WINDOW medindo o tempo até o processo sair.
  **Aceite:**
  - `GATE OK`;
  - hello/64 e triangle/320 com `--janela`: **zero diferenças RGB** entre a captura
    e a exportação; contagens de pixels iguais às sondas (hello 302/12, triangle
    305922/479, salvo mudança documentada);
  - fechamento durante a execução e após o orçamento: processo sai em < 5 s com
    "interrompida pelo host";
  - modo `--continuo`: Espaço/F5/F9 enviados por XSendEvent produzem as linhas de
    pausa/salvar/restaurar no log;
  - capturas PNG/PPM e logs em `target/visual-linux-*`; README e CONTINUIDADE
    com o comando Linux.

  Evidência: `GATE OK: 624 testes`, target/gate/20261026-214442-4B9op0iB.log;
  `TOOLS OK` (27 testes Python, incluindo janela Linux no launcher).
  `tools/verificar-janela` no Xwayland/Fedora 44: hello/64 e triangle/320 com
  **0 diferenças RGB** entre captura GetImage e `--quadro-apresentado`;
  302/12 e 305922/479 pixels/linhas, iguais às sondas. Fechamento após o
  orçamento 33,0/18,6 ms (status 1; o guest já parou e a linha de orçamento é
  preservada, não há execução para o host interromper); durante a execução
  6,3/31,0 ms com "interrompida pelo host", status 0. Contínuo (hello): Espaço
  pausa/retoma no mesmo contador, F5 salvou checkpoint-000001, F9 restaurou,
  fechou em 20,6 ms com parada pelo host. Capturas e logs em
  target/visual-linux-{hello,triangle}-{apos,durante} e
  target/visual-linux-hello-continuo; PNG do triangle inspecionado (triângulo
  RGB e texto Triangle Demo). README, ADR-0086 e CONTINUIDADE com o comando Linux.

## Fase 9 — Sample `mesh` (textura, depth e matrizes)

Referência visual pública: `samples/mesh/screenshot.png` do nxdk fixado.

- [x] 9.1 Inventário: compilar o sample com `build-nxdk-sample.sh`, integrar ao
  `prepare-sample.sh`/`sonda.sh` e registrar o primeiro bloqueio (instrução
  recusada na análise, serviço de kernel, método NV097 sem efeito).
  **Aceite:** ADR novo com a lista ordenada de bloqueios observados (log da sonda
  citado) e a fonte pública de cada recorte necessário; nenhuma emulação nova.

  Evidência: `GATE OK: 624 testes`, target/gate/20261026-214957-zmsBFH1V.log;
  `TOOLS OK`. Build Fedora com toolchain local: target/homebrew-mesh/mesh-raw.xbe.
  Sonda mesh para na tradução em 0x1269E `Fmulp_sti_st0`
  (target/samples/mesh-Jk0wfCeX.translate.log); ordinal-report 45/75 serviços.
  ADR-0087: x87 faltante = FMULP/FDIVRP/FSQRT/FSIN/FCOS (desmontagem completa),
  SSE coberto, sem MXCSR/FLDCW; kernel a observar; PGRAPH: TX_*, INDEX_DATA,
  VP 44 instruções, combiner tex0×col0, depth. Nenhuma emulação nova.

- [x] 9.2 Desbloquear CPU/kernel até o mesh submeter geometria: implementar, com
  oráculo ou referência para cada instrução, o que 9.1 listar (x87/SSE de P8/P9,
  serviços de kernel). Cada recorte com teste próprio.
  **Aceite:** `GATE OK`; sonda mesh com `recusadas: 0` e `GPU raster` com
  triângulos > 0; P8/P9 reescritos com o recorte entregue.

  Progresso (commit intermediário, gate 643): x87 do mesh entregue (ADR-0088,
  P9); sonda mesh traduz e executa 320 fatias com 281 chamadas e `recusadas: 0`.
  `GPU raster` ainda 0: os desenhos chegam por INDEX_DATA (0x1800), sem modelo,
  e o pipeline do mesh (textura, combiner, VP) é recusado pelo SoftwareV1. O
  critério de triângulos > 0 depende do PGRAPH, trabalhado em seguida.

  Evidência: `GATE OK: 658 testes`, target/gate/20261026-230330-mTdAKvod.log.
  Sonda mesh/320 (target/sonda/mesh-20261026-224137-aMy2E7hf.log): 281
  chamadas, `recusadas: 0`, `GPU raster: chamadas=54 triângulos=3888
  fragmentos=179834`. CPU: x87 do ADR-0088 (P9 reescrito; P8 não é usado pelo
  mesh, sem MXCSR/FLDCW no binário). Para os triângulos aparecerem foi preciso o
  desenho indexado e o pipeline do ADR-0089 (índices, VP, combiner, textura,
  depth W), que também adianta a 9.3. Triangle/320 sem regressão: PPM com SHA
  1c2d8fc5…7c280c, igual ao ADR-0069.

- [~] 9.3 Texturas e depth no PGRAPH SoftwareV1: formato de textura que o mesh
  usa, endereçamento/wrap/filtro configurados, teste de profundidade e
  combiner com textura. Nada reconhecido por título ou hash.
  **Aceite:** `GATE OK`; testes golden sintéticos (textura 4×4, depth com dois
  triângulos sobrepostos); sonda mesh com framebuffer comparado ao
  `screenshot.png` por métrica declarada no ADR (silhueta/cobertura e cor média por
  região), com a diferença anotada na Evidência; snapshots versionados.

## Fase 10 — Kernel e HAL em samples pequenos

- [ ] 10.1 `led`: `XSetCustomLED` → SMBus/SMC com estado observável e
  snapshot; recusa para comandos não cobertos.
  **Aceite:** `GATE OK`; sonda led com `recusadas: 0`; estado do LED reportado no
  resumo da execução; ADR.

- [ ] 10.2 `winapi_filefind` e `winapi_drivelist`: enumerar o que P22 permite
  (um `default.xbe`) e recusar o resto com variante explícita.
  **Aceite:** `GATE OK`; sondas com `recusadas: 0` ou bloqueio documentado com
  o serviço exato; P4/P22 atualizados.
