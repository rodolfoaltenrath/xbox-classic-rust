# Dependências de terceiros

Registro exigido pelo ROADMAP §14: toda dependência entra aqui com versão, licença e
motivo. Antes de adicionar uma nova, verifique a licença e some uma linha.

## Workspace (Fase 0)

| Crate | Versão | Licença | Para quê |
|---|---|---|---|
| `thiserror` | 2 | MIT OR Apache-2.0 | derivar os tipos de erro de `xbox-core` |
| `tracing` | 0.1 | MIT | instrumentação; spans por subsistema |
| `tracing-subscriber` | 0.3 | MIT | subscriber de log; `env-filter`. Sem `ansi` (ADR-0008) |
| `serde` | 1 | MIT OR Apache-2.0 | configuração (não é usado para snapshots) |
| `toml` | 0.8 | MIT OR Apache-2.0 | formato do arquivo de configuração |
| `clap` | 4 | MIT OR Apache-2.0 | CLI do `obsidian` e das ferramentas. Sem `color` (ADR-0008) |
| `iced-x86` | 1.21.0 | MIT | decoder x86 escrito em Rust para análise AOT; somente `std`, `decoder`, `instr_info` (ADR-0013) |

## Referência do recompilador (Fase 3-AOT)

- [iced-x86](https://docs.rs/iced-x86/1.21.0/iced_x86/): decoder incorporado como
  dependência Cargo; licença verificada no projeto oficial.
- [XboxXBERecomp](https://github.com/TGP482/XboxXBERecomp), revisão
  `99078b119b5d5e0d1d6afb1390e29437c0c984c0`, MIT: referência arquitetural da
  avaliação em `docs/arch/avaliacao-xbe-recomp.md`. Não foi copiado código-fonte
  desse repositório para esta prova; analisador, IR, emissor e runtime foram escritos
  em Rust neste workspace.

## Fora do workspace

| Crate | Versão | Licença | Para quê |
|---|---|---|---|
| `criterion` | 0.5 | MIT OR Apache-2.0 | benchmarks em `crates/xbox-core/bench` |

## Política

- Preferir dependências pequenas e sem build script quando houver escolha.
- Nenhuma dependência pode introduzir código de origem proprietária.
- `unsafe` em dependência é aceitável; `unsafe` em código nosso só nos crates
  autorizados (ROADMAP §13.3).

## Fuzzing (nightly, fora do workspace)

| Crate | Versão | Licença | Para quê |
|---|---|---|---|
| `libfuzzer-sys` | 0.4 | MIT OR Apache-2.0 OR NCSA | alvos de fuzz em `crates/xbox-formats/fuzz` |
