# Plano: do polling de DMA_GET ao triângulo na tela

Modo: pausado

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

- [~] 0.1 Implementar `MmFreeContiguousMemory` liberando a alocação real no
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

## Fase 1 — PFIFO e pushbuffer com efeito real

- [ ] 1.1 Inventário e evidência, sem código de emulação. Listar todo registrador
  PFIFO/NV_USER/PRAMIN que o triangle toca entre pb_init e o polling, com a fonte
  pública de cada um (como no ADR-0056) e o valor que o guest escreve.
  **Aceite:**
  - ADR novo com a tabela registrador → fonte → valor observado → consumidor no pbkit;
  - a tabela cita o log da sonda usado (caminho em `target/sonda/`);
  - define quais registradores a tarefa 1.2 modela e quais continuam lendo zero
    declarado.

- [ ] 1.2 Modelar os registradores escolhidos em 1.1 (DMA_PUT, DMA_GET, estado do
  canal e o que mais o ADR listar) com leitura/escrita persistente no `Bus`.
  **Aceite:**
  - `GATE OK`;
  - sonda triangle: a linha `toca 0xFD800040: sem modelo` e a linha
    `toca 0xFD800044: sem modelo` não aparecem mais;
  - `mmio_sem_modelo` da sonda menor que o valor medido antes da tarefa (anotar os
    dois números na Evidência);
  - snapshot BUS0 com versão nova e teste de migração da versão anterior;
  - provisórios novos declarados no ROADMAP §16.1 com condição de saída.

- [ ] 1.3 Consumir o pushbuffer: ler comandos entre GET e PUT pelos descritores de
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

- [ ] 1.4 Estado do PFIFO no snapshot no meio do pushbuffer.
  **Aceite:**
  - `GATE OK`;
  - teste que salva o snapshot com comandos pendentes, restaura e obtém o mesmo
    DMA_GET final e a mesma lista de métodos de uma execução sem interrupção.

## Fase 2 — PGRAPH mínimo (NV097) para o triangle

- [ ] 2.1 Inventário dos métodos NV097 recebidos (saída da 1.3) e escolha do recorte
  mínimo para limpar a tela e desenhar.
  **Aceite:** ADR novo com método → fonte → efeito, e a lista dos que ficam sem efeito
  declarado.

- [ ] 2.2 Configuração de surface e `CLEAR_SURFACE` escrevendo no framebuffer guest.
  **Aceite:**
  - `GATE OK`;
  - teste unitário: clear de uma surface pequena produz a cor e o retângulo pedidos;
  - sonda triangle: a linha `framebuffer` muda em relação aos 1580 pixels/29 linhas do
    diagnóstico, de acordo com a cor de clear que o triangle envia (citar o valor na
    Evidência).

- [ ] 2.3 Vértices (inline ou arrays, o que o triangle usar) e rasterização em software
  de um triângulo de cor interpolada no framebuffer guest.
  **Aceite:**
  - `GATE OK`;
  - teste golden: framebuffer de um triângulo sintético comparado com uma imagem de
    referência versionada em `tests/`;
  - sonda triangle: pixels coloridos acima do valor do clear e presença das três
    cores dos vértices (anotar contagem e cores na Evidência);
  - CONTINUIDADE_ALPHA.md "Passagem de bastão" reescrita com o novo estado.

## Fase 3 — Backlog (só depois das fases 0 a 2)

Cada item termina quando a **condição de saída** escrita para ele no ROADMAP §16.1 for
atendida, com `GATE OK`, ADR e CHANGELOG.

- [ ] 3.1 P5: análise automática de jump tables (substitui raízes manuais do triangle).
- [ ] 3.2 P3: escalonador de threads.
- [ ] 3.3 P7: `KeTickCount` em tempo real.
- [ ] 3.4 P10/P11: DIRQL/IF/`STI` como máscara real.
