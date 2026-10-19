# xbox-core

Raiz do grafo de dependências: não depende de nenhum outro crate do projeto e não sabe
o que é um XBE, uma GPU ou um sistema operacional.

## `error` — erros

Um enum `Error` por todo o projeto, com `Result<T>` próprio. Duas regras:

- dado do usuário malformado (XBE, ISO, snapshot, config) é **erro tipado**, nunca
  pânico (ROADMAP §13.3);
- caminho previsto mas não escrito devolve `Error::NotImplemented { what, phase }`, que
  imprime a fase do roadmap responsável. É por isso que `obsidian ui` hoje responde
  "ainda não implementado (Fase 8)" em vez de `todo!()`.

## `snapshot` — serialização de estado

Formato binário explícito, com assinatura `OBSIDSNP`, versão de container e **seções
versionadas individualmente** (tag de 4 bytes + versão). Escrita à mão, sem `serde`:

- um save state antigo continua legível quando só um subsistema mudou de versão;
- se um tipo **não** consegue se serializar, é porque guardou um ponteiro do host — o
  que viola o ADR-0004. A serialização manual é o que torna essa violação visível.

Invariante testada: `save` duas vezes produz bytes idênticos, e `save → load → save`
é estável. São os testes que sustentam save state e replay determinístico.

## `time` — relógio virtual e agendador

Um único relógio, contado em **ciclos da CPU do guest** (733⅓ MHz). Todos os outros
domínios derivam dele: `RDTSC`, contador de performance (3,375 MHz),
`KeTickCount` (1 ms), amostras de áudio (48 kHz), vblank (59,94 / 50 Hz).

Duas decisões não óbvias:

1. **Arredondamento assimétrico.** Tempo decorrido arredonda para baixo; duração pedida
   arredonda para cima. Um `KeDelayExecutionThread(16ms)` que vencesse em 15,999 ms
   seria um bug observável pelo guest. Efeito colateral útil: `ms → ciclos → ms` é
   estável.
2. **Eventos são enums, não closures.** Uma fila de `Box<dyn FnMut>` não serializa.
   `EventKind` é `Copy`, pequeno e codificado como (tag, payload) estável; quem consome
   o evento decide o que fazer.

Desempate determinístico: eventos com o mesmo prazo vencem na ordem de inserção
(campo `seq`), porque a ordem interna de um heap binário é estável mas não é
*garantida* — e determinismo precisa de garantia, não de sorte.

`run_until(limite)` consome um evento por chamada e, quando não há mais nada na janela,
posiciona o relógio exatamente em `limite`. Assim o laço
`while let Some(e) = sched.run_until(fim)` termina com o tempo correto.

## `config` — configuração

TOML via `serde`, com `default` em tudo (arquivo ausente é válido) e
`deny_unknown_fields` (typo em opção é erro, não silêncio). `validate()` cobre o que o
sistema de tipos não pega: RAM só 64 ou 128 MiB, escala interna 1–4, latência de áudio
1–200 ms.

## `log` — logging

`tracing` com filtro por subsistema; `OBSIDIAN_LOG` tem precedência sobre `-v`.
Sem ANSI (ADR-0008) — o appliance da Fase 10 não tem terminal.
