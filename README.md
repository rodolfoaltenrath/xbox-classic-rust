# Obsidian — Xbox Classic Rust

Projeto experimental em Rust para executar jogos do Xbox clássico por recompilação
estática: **XBE → análise x86 → Rust gerado → executável nativo**, acompanhado de uma
camada de compatibilidade Xbox. O objetivo é uma experiência de mini console,
navegável por controle, com Windows/Linux e futuramente Linux direto em KMS/DRM.

**Jogos comerciais ainda não executam.** Atualmente há parsers de mídia, VFS,
memória/MMU/bus e um subconjunto inteiro de x86 que já gera programas Rust executáveis.
Kernel/TLS, ponto flutuante, gráficos, áudio e frontend continuam
em desenvolvimento. O projeto não inclui jogos, BIOS ou bibliotecas proprietárias.

## Requisitos e build

Use o toolchain fixado em `rust-toolchain.toml` (Rust 1.98.1) e um ambiente de link
compatível com Rust no Windows ou Linux. Detalhes: [build](docs/dev/build.md).

```sh
cargo build --workspace
cargo run -p obsidian -- doctor
cargo test --workspace --all-targets --features xbox-cpu/isa-oracle
```

## Demonstração de recompilação

```sh
cargo run -p xberecomp -- demo --out target/recomp-demo --run
```

A fixture foi escrita pelo projeto: soma 1..=10, chama uma função, grava o resultado
na memória e retorna. O programa nativo imprime `EAX=62, instruções=37`. Na primeira
geração, o diretório de saída precisa estar vazio. Depois, execute o projeto criado:

```sh
cargo run --offline --manifest-path target/recomp-demo/Cargo.toml
```

O subconjunto atual inclui operandos de 8/16/32 bits, registradores parciais,
instruções de pilha, ADC/SBB, shifts, rotações, MUL/IMUL, DIV/IDIV e as 16 condições
Jcc, JCXZ/JECXZ, LOOP e strings com REP de execução limitada e snapshot parcial.
CALL/JMP indiretos usam descoberta conservadora e entradas adicionais `--entry`;
destinos desconhecidos são erros recuperáveis antes de alterar CPU/pilha.
Os testes do Rust gerado verificam 9438 casos de ALU e mais 262144 casos
exaustivos de ADC/SBB de byte
contra a CPU física. Outros 445686 casos verificam shifts e suas flags definidas;
791764 verificam rotações e shifts duplos com resultados definidos.
Produtos/divisões acrescentam 483916 comparações físicas e 1000580 diagnósticos
classificados; #DE é reportado ao host, sem entrega por kernel/IDT ainda.
3872 saltos são verificados por relações independentes.
Strings acrescentam 3456 comparações com a CPU física, além de testes de faltas,
direção reversa, sobreposição e retomada no meio da repetição.
Isso ainda não comprova compatibilidade com jogos comerciais ou com toda a ISA.

## Documentação

- [Roadmap e progresso](ROADMAP.md)
- [Arquitetura da recompilação](docs/arch/recompilacao.md)
- [Uso do recompilador e validação](docs/dev/recomp.md)
- [Decisões de arquitetura](docs/adr/)
- [Dependências e licenças de terceiros](docs/dev/third-party.md)
- [Changelog](CHANGELOG.md)

As ISOs, XBEs locais, BIOS/binários, saves, dados de teste locais e diretórios de
build ficam fora do Git. Fixtures versionadas são geradas por código do projeto.
Pendências conhecidas de validação, incluindo Clippy em `xbox-mem`, estão registradas
na documentação; nem todas as fases ou verificações do roadmap estão concluídas.

## Licença

[Apache-2.0](LICENSE). As dependências mantêm suas respectivas licenças.
