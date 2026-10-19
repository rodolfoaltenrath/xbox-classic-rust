# Usar o recompilador Rust experimental

Requer o toolchain fixado em `rust-toolchain.toml`, Cargo e o ambiente de link do
projeto. A primeira compilação do workspace precisa obter `iced-x86`. A execução do
projeto gerado usa `--offline` e as dependências já baixadas pelo workspace.

## Demonstração

Na raiz do workspace:

```powershell
cargo run -p xberecomp -- demo --out target/recomp-demo --run
```

A saída de sucesso termina com:

```text
10 instruções alcançáveis, 5 blocos Rust
Execução nativa concluída: EAX=62, instruções=37
```

`demo` escreve `Cargo.toml`, `src/image.xbe`, `src/generated.rs` e `src/main.rs`.
`--run` compila e executa o projeto gerado. O diretório precisa estar vazio: a
ferramenta recusa sobrescrever arquivos existentes.

Depois da primeira geração, executar novamente o projeto já criado:

```powershell
cargo run --offline --manifest-path target/recomp-demo/Cargo.toml
```

Para compilar com otimização e executar o artefato no Windows:

```powershell
cargo build --offline --release --manifest-path target/recomp-demo/Cargo.toml
.\target\recomp-demo\target\release\obsidian-native.exe
```

No Linux, o artefato equivalente é `target/recomp-demo/target/release/obsidian-native`.
Esta rodada foi verificada no Windows; a execução no Linux continua a cargo do CI.

## Traduzir um XBE de teste

```powershell
cargo run -p xberecomp -- translate caminho/teste.xbe --out target/meu-recomp --run
```

O conjunto inicial é propositalmente pequeno. Um jogo real será recusado por
instrução sem suporte, TLS ou imports do kernel. O projeto gerado referencia os
crates locais por caminho; ainda não é um pacote independente para distribuir.
`obsidian run` continua sem executar jogos comerciais.

Use uma pasta dentro de `target` para artefatos: o Rust gerado de um jogo pode
conter tradução de código proprietário e não deve entrar no repositório.

## Validação

```powershell
cargo test -p xbox-recomp
cargo test -p xberecomp --test native
cargo test -p xbox-cpu --features isa-oracle
cargo clippy -p xbox-recomp -p xberecomp -p xbox-cpu --features xbox-cpu/isa-oracle --no-deps --all-targets -- -D warnings
```

O teste `native` cria projetos únicos em `target/recomp-tests`, compila o Rust
emitido e roda os testes e o executável resultantes. O cache da compilação fica em
`target/recomp-native-check`, separado do target principal para evitar lock recursivo
do Cargo durante `cargo test --workspace`. A primeira execução desse teste compila
as dependências desse target e pode demorar mais.

O snapshot da prova inclui CPU e bus. Não equivale a save state de um jogo completo:
kernel, GPU, áudio e input ainda não participam.

Em x86_64, a integração executa 13 testes dentro do projeto gerado: inclui 7986
comparações de ALU com a CPU física e 3872 de Jcc por relações independentes. A feature
`isa-oracle` só entra como dependência de desenvolvimento desse projeto, sem alterar
o executável normal. O log fica em `target/recomp-tests/<execução>/native-test-results.txt`.

Para repetir a verificação com warnings promovidos a erro, como no CI:

```powershell
$env:RUSTFLAGS = '-D warnings'
cargo test --workspace --all-targets --features xbox-cpu/isa-oracle
```

## Resultado local de 2026-10-19

- `cargo test --workspace --all-targets`: passou (170 testes no workspace, incluindo
  o teste que compila outro projeto e executa seus seis testes nativos).
- `cargo test --workspace --doc`: passou (dois doctests).
- `cargo fmt --all --check`: passou.
- Compilação `--release` do projeto de demonstração e execução direta de
  `target/recomp-demo/target/release/obsidian-native.exe`: EAX=62, 37 instruções.
- Clippy dos três pacotes alterados com `--no-deps`: passou sem warnings.
- Clippy incluindo dependências locais encontra 19 avisos preexistentes em
  `xbox-mem`, promovidos a erro por `-D warnings`; incluem `must_use`, simplificações
  de `match` e `Box<Vec<_>>`. Não foram suprimidos ou tratados como sucesso.

Não foi executado Miri nesta rodada, nem feito teste de compatibilidade comercial.

## Ampliação inteira — validação de 2026-10-19

- `cargo test --workspace --all-targets --features xbox-cpu/isa-oracle`, com
  `RUSTFLAGS=-D warnings`: 173 testes passaram. A integração compilou o Rust gerado
  e seus 13 testes nativos também passaram, com os mesmos flags de compilação.
- Os 13 testes do projeto nativo passaram também em `--release`, incluindo as
  7986 comparações ALU e 3872 verificações de Jcc.
- Doctests e `cargo fmt --all --check`: passaram.
- Clippy dos pacotes `xbox-cpu`, `xbox-recomp` e `xberecomp`, incluindo o oráculo e
  `--no-deps --all-targets -- -D warnings`: passou. As pendências preexistentes do
  Clippy em `xbox-mem` não foram alteradas por esta ampliação.
- O CI passa a executar a suíte com `xbox-cpu/isa-oracle` nos runners Windows/Linux;
  esta rodada foi executada localmente no Windows, sem alegar execução remota do CI.

O arquivo `Complex_4627v1.03.bin` disponibilizado pelo mantenedor permanece local e
coberto pela regra `*.bin` do `.gitignore`. Só foi verificada sua presença e tamanho
(1 MiB); seu conteúdo não foi consultado nem usado nesta etapa.
