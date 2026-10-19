# Build e ambiente de desenvolvimento

## Requisitos

- Rust **1.98.1** (fixado em `rust-toolchain.toml`, com `rustfmt` e `clippy`).
- Nada mais na Fase 0. A partir da **Fase 5** é preciso um toolchain capaz de linkar
  `windows-sys` no Windows — ver [Toolchain no Windows](#toolchain-no-windows).

## Comandos

```sh
cargo build --workspace            # build de desenvolvimento
cargo build --workspace --release  # build otimizado (LTO fat, 1 codegen unit)
cargo test  --workspace --all-targets
cargo lint                         # alias: clippy -D warnings
cargo fmt --all
cargo run -p obsidian -- doctor    # relatório do ambiente e das fases
```

Benchmarks ficam em um pacote separado (ADR-0008):

```sh
cd crates/xbox-core/bench && cargo bench
```

## Toolchain no Windows

O host de desenvolvimento atual usa `x86_64-pc-windows-gnu` **sem MinGW completo**.
O MinGW reduzido que o rustup empacota não tem `as.exe`, e por isso `dlltool` falha ao
gerar a import library que o `windows-sys` 0.60+ precisa (`raw-dylib`).

Sintoma:

```
error: dlltool could not create import library with ...dlltool.exe ...
       dlltool.exe: CreateProcess
```

Contorno em vigor na Fase 0 (ADR-0008): nenhuma dependência do workspace puxa
`windows-sys` — `clap` e `tracing-subscriber` estão sem as features de cor, e o
criterion ficou num pacote fora do workspace.

**Isso tem prazo.** `wgpu` (Fase 5), WASAPI (Fase 6) e XInput/RawInput (Fase 7) exigem
`windows-sys`. Antes da Fase 5, instale uma das duas opções:

| Opção | O que resolve | Custo |
|---|---|---|
| **MSVC Build Tools** + `rustup toolchain install stable-x86_64-pc-windows-msvc` | É o toolchain padrão do Rust no Windows e o que o CI usa | download grande (~2-3 GB) |
| **MinGW-w64 completo** (MSYS2 ou WinLibs) no PATH | Mantém o toolchain `gnu` atual; fornece `as.exe` | download médio |

Depois de instalar, religue as features cosméticas e volte o bench para o workspace
(o ADR-0008 descreve exatamente o que reverter).

## Se precisar linkar hoje com o MinGW empacotado

O `dlltool`/`ld` do rustup não estão no PATH por padrão. Para um build que não use
`windows-sys`, não é necessário; se algum caso exigir, acrescente ao PATH:

```sh
$HOME/.rustup/toolchains/1.98.1-x86_64-pc-windows-gnu/lib/rustlib/x86_64-pc-windows-gnu/bin/self-contained
```

## Linux

Nada especial na Fase 0. A Fase 10 acrescenta `x86_64-unknown-linux-musl`, headers de
DRM/KMS, ALSA e KVM — documentado quando chegarmos lá.
