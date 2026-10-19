# ADR-0008: Toolchain no Windows e o bloqueio do windows-sys

- **Status:** aceito
- **Data:** 2026-10-19
- **Fase:** 0

## Contexto

A máquina de desenvolvimento tem apenas o toolchain `x86_64-pc-windows-gnu`, sem MinGW
completo e sem MSVC. O rustup empacota um MinGW reduzido em
`lib/rustlib/x86_64-pc-windows-gnu/bin/self-contained/` com `ld`, `gcc` e `dlltool`,
mas **sem `as.exe`**.

O `windows-sys` 0.60+ usa `raw-dylib`, o que faz o rustc chamar `dlltool`, que tenta
chamar o assembler e falha com `CreateProcess`. Resultado: qualquer crate que dependa
de `windows-sys` não compila neste host.

Na Fase 0, todos os consumidores de `windows-sys` eram **cosméticos**:

| Crate | Por que puxava windows-sys |
|---|---|
| anstream, anstyle-wincon, anstyle-query | cor no terminal (feature `color` do clap) |
| nu-ansi-term | cor no log (feature `ansi` do tracing-subscriber) |
| is-terminal, winapi-util | detecção de TTY e walkdir, via criterion |

## Alternativas consideradas

1. **Instalar MSVC Build Tools (~2-3 GB) ou MinGW-w64 completo** — resolve de vez;
   exige download e intervenção do usuário, e não era necessário para a Fase 0.
2. **Desligar as features cosméticas e isolar o criterion** — zero download, e as duas
   mudanças se defendem por mérito próprio.
3. **Fixar uma versão antiga de windows-sys** — impossível: as versões com import lib
   são semver-incompatíveis com o que as dependências pedem.

## Decisão

Opção 2, em três partes:

1. `clap` com `default-features = false` (sem `color`).
2. `tracing-subscriber` com `default-features = false` (sem `ansi`).
3. Benchmarks em um pacote separado, `crates/xbox-core/bench`, **excluído do
   workspace** — porque dev-dependencies são compiladas em todo `cargo test`, e manter
   criterion no workspace quebraria o teste neste host.

## Consequências

- Positivas: `build`, `test`, `clippy` e `fmt` passam no host atual sem instalar nada;
  log sem ANSI é o que o appliance da Fase 10 quer de qualquer forma.
- Negativas / dívida aceita: sem cor em log e em `--help`; benchmarks rodam com
  `cd crates/xbox-core/bench && cargo bench`, não com `cargo bench` na raiz.
- **Prazo de validade:** as Fases 5 (wgpu), 6 (WASAPI) e 7 (XInput/RawInput) exigem
  `windows-sys` de verdade. Antes da Fase 5 este host **precisa** de MSVC Build Tools
  ou de um MinGW-w64 completo. Registrado como pré-requisito na Fase 5 do ROADMAP.

## Como reverter

Instalar o toolchain adequado e reverter as três partes: religar `color` e `ansi`,
e mover o bench de volta para o workspace. Nenhuma decisão de arquitetura depende disso.

## Atualização de 2026-10-19 — a dívida é menor do que parecia

A sonda de memória da Fase 2 (`docs/arch/memoria.md`) descobriu um contorno melhor:
declarar as funções do Win32 à mão resolve o problema sem `windows-sys`.

```rust
#[link(name = "kernel32")]
unsafe extern "system" {
    fn VirtualAlloc(addr: *mut u8, size: usize, tipo: u32, protecao: u32) -> *mut u8;
}
```

Isso linka contra a import library que o componente `rust-mingw` já traz, sem passar
por `raw-dylib` e portanto sem precisar de `dlltool` nem de `as.exe`. Verificado:
compila e roda neste host, com `VirtualAlloc`, `VirtualProtect`,
`AddVectoredExceptionHandler` e `GetLastError`.

Consequências:

- **`xbox-mem` e `xbox-platform` não precisam de `windows-sys`.** O que cada um precisa
  do Win32 é um punhado de funções, e declará-las à mão é menos dependência, não mais.
- O prazo do ADR-0008 continua valendo **só para a Fase 5**, por causa do `wgpu`, que
  arrasta `windows-sys` por dentro e não está sob nosso controle.
- A regra do ROADMAP §13.3 continua: essas declarações ficam confinadas a
  `xbox-platform` (e, na Fase 2, ao módulo de arena de `xbox-mem`), com `// SAFETY:` em
  cada chamada.
