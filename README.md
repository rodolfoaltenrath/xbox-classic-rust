# Obsidian — Xbox Classic Rust

Projeto experimental em Rust para executar jogos do Xbox clássico por recompilação
estática: **XBE → análise x86 → Rust gerado → executável nativo**, acompanhado de uma
camada de compatibilidade Xbox. O objetivo é uma experiência de mini console,
navegável por controle, com Windows/Linux e futuramente Linux direto em KMS/DRM.

**O alpha executa hello e triangle do nxdk e apresenta texto/triângulos em janela
Win32.** A captura da janela foi comparada com o framebuffer guest: zero diferenças
RGB em ambos. Há kernel HLE, duas threads cooperativas, interrupções e rasterização
software parcial. **Jogos comerciais ainda não executam**; áudio, controles, Linux
gráfico e compatibilidade geral continuam pendentes. O projeto não inclui jogos,
BIOS ou bibliotecas proprietárias.

## Rodar o alpha no Windows

Pré-requisitos: Git com Git Bash, Rust 1.98.1 fixado, linker funcional e os XBEs
homebrew locais (`target/homebrew/hello.xbe` e `target/homebrew2/triangle-raw.xbe`).
Para compilar os samples com o SDK público fixado, consulte
[o guia do homebrew](docs/dev/homebrew.md). Entradas e SDK ficam fora do Git.

No **Git Bash**, na raiz do repositório:

```bash
bash scripts/run-alpha.sh hello
bash scripts/run-alpha.sh triangle
```

No PowerShell, use o Bash do Git (o `bash.exe` do WSL não oferece a janela Win32):

```powershell
& "$env:ProgramFiles\Git\bin\bash.exe" scripts/run-alpha.sh hello
& "$env:ProgramFiles\Git\bin\bash.exe" scripts/run-alpha.sh triangle
```

O comando prepara a tradução atual, compila e abre a janela. O primeiro build
pode levar vários minutos; projetos idênticos reutilizam a biblioteca compilada.
Feche pelo **X**. Se o orçamento acabar antes, a imagem fica congelada até fechar;
o status 1 de orçamento é preservado e explicado. Fechar durante a execução é
parada pelo host/status 0. Logs de cada rodada e BGRA ficam em `target/alpha/runs/`.
Opções, caminhos com espaços e verificação visual em [alpha](docs/dev/alpha.md).

Para continuar até fechar, com pausa e estados completos:

```powershell
& "$env:ProgramFiles\Git\bin\bash.exe" scripts/run-alpha.sh triangle --continuo
```

**Espaço** pausa/retoma; **F5** salva uma geração nova; **F9** volta à última
geração salva nesta execução. O título indica a pausa. Os estados ficam na
pasta `states/` da rodada e `final.obsstate` é salvo ao encerrar. Para retomar
em outra execução, acrescente `--carregar "caminho/final.obsstate"` ao comando.
Esses atalhos controlam a sessão do programa; controles Xbox ainda estão pendentes.
Estados contêm memória completa e o XBE, ficam ignorados pelo Git e não
sobrescrevem estados anteriores.

## Requisitos e build

Use o toolchain fixado em `rust-toolchain.toml` (Rust 1.98.1) e um ambiente de link
compatível com Rust no Windows ou Linux. Detalhes: [build](docs/dev/build.md).

```sh
cargo build --workspace
cargo run -p obsidian -- doctor
cargo test --workspace --all-targets --features xbox-cpu/isa-oracle
```

Para validar um XBE, pasta extraída ou ISO pelo barramento, sem executar o título:

```sh
cargo run -p obsidian -- check-memory caminho/do/jogo.iso
```

O estado vigente está no [guia do alpha](docs/dev/alpha.md) e na
[passagem de bastão](CONTINUIDADE_ALPHA.md). Cobertura com variantes limitadas:
**38/69 serviços do hello e 42/75 do triangle**. Ambos traduzem sem raízes manuais;
o triangle passa por PFIFO/PRAMIN, ISR/DPC, vertex program e rasterização software.
Snapshots preservam CPU/RAM/kernel/GPU/scheduler com migrações explícitas.
Gamma/filtros e os limites da GPU continuam declarados no ROADMAP §16.1.

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
CALL/JMP indiretos usam descoberta conservadora de tabelas/callbacks; outros
destinos podem exigir entradas adicionais `--entry`;
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

- [Guia de continuidade do alpha para outra LLM](CONTINUIDADE_ALPHA.md)
- [Roadmap e progresso](ROADMAP.md)
- [Arquitetura da recompilação](docs/arch/recompilacao.md)
- [Uso do recompilador e validação](docs/dev/recomp.md)
- [Decisões de arquitetura](docs/adr/)
- [Dependências e licenças de terceiros](docs/dev/third-party.md)
- [Changelog](CHANGELOG.md)

As ISOs, XBEs locais, BIOS/binários, saves, dados de teste locais e diretórios de
build ficam fora do Git. Fixtures versionadas são geradas por código do projeto.
O Clippy do workspace passou após corrigir os avisos antigos de `xbox-mem`.
Os limites de validação e os próximos escopos estão documentados no ROADMAP.

## Licença

[Apache-2.0](LICENSE). As dependências mantêm suas respectivas licenças.
