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
Há validações locais no Windows e no Linux; os ambientes e resultados estão
registrados abaixo.

## Traduzir um XBE de teste

```powershell
cargo run -p xberecomp -- translate caminho/teste.xbe --out target/meu-recomp --run
```

O conjunto inicial é propositalmente pequeno. Um jogo real será recusado por
instrução sem suporte, perfil TLS não implementado ou dependências de kernel
pendentes. O projeto gerado referencia os
crates locais por caminho; ainda não é um pacote independente para distribuir.
`obsidian run` continua sem executar jogos comerciais.

Para callbacks e tabelas de saltos cujos destinos não foram inferidos, declare
raízes adicionais de código (endereços virtuais do XBE, não offsets de arquivo):

```powershell
cargo run -p xberecomp -- translate caminho/teste.xbe --out target/meu-recomp --entry 0x00011008 --entry 69646
```

Cada `--entry` inclui essa rotina e seu fluxo alcançável na tradução. Os endereços
do exemplo só ilustram a sintaxe; use os alvos da sua imagem. Todas as entradas são
validadas antes de gerar arquivos. Constantes locais e ponteiros de seções imutáveis
podem dispensar a opção. Valores de memória gravável não são usados para inferir
destinos. Um `UnknownTarget` em execução informa a instrução e o destino ausente;
verifique esse endereço antes de declará-lo e gerar novamente o projeto.

Com CALL/JMP indireto, cada início de instrução conhecido vira um bloco de despacho;
isso muda a granularidade do orçamento. O conjunto de alvos é fechado durante a
tradução: não há decoder ou compilação sob demanda durante a execução (ADR-0019).

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

Em x86_64, a integração executa 64 testes dentro do projeto gerado: inclui 9438
comparações de ALU com a CPU física, 262144 casos exaustivos de ADC/SBB de byte,
445686 comparações de shifts e 791764 de rotações/shifts duplos. Outros 3872 casos
verificam Jcc por relações independentes. Produtos/divisões acrescentam 483916
comparações físicas e 1000580 diagnósticos classificados. A feature
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

## ADC/SBB — validação Linux de 2026-10-19

Ambiente: Linux x86_64, Rust/Cargo/Clippy 1.99.0 do Fedora. O host não tem rustup;
o pin 1.98.1 permanece no repositório, mas não foi o toolchain desta rodada.

- `RUSTFLAGS='-D warnings' cargo test --workspace --all-targets --features xbox-cpu/isa-oracle`:
  174 testes passaram. A integração compilou e executou os 18 testes nativos.
- Os mesmos 18 testes nativos passaram em `--release`, com warnings como erros:
  9438 casos gerais de ALU, 262144 casos exaustivos de ADC/SBB de byte e 3872 casos
  de Jcc. A demonstração continua produzindo EAX=62 após 37 instruções.
- Os testes novos cobrem AH, memória de 8/16/32 bits, imediatos com sinal, carry
  entre blocos, restauração do snapshot e erro de escrita sem comprometer CPU/FLAGS.
- Dois doctests, `cargo fmt --all --check`, `git diff --check` e `obsidian doctor`:
  passaram. Clippy dos três pacotes alterados com `--no-deps --all-targets` e
  `xbox-cpu/isa-oracle`: passou sem warnings.
- Clippy global com warnings como erros continua falhando nas 19 pendências de
  `xbox-mem`. O Clippy 1.99 também aponta `assert_is_empty` no teste preexistente
  de `xbox-formats/src/xdvdfs.rs`. Esses arquivos não foram alterados nesta etapa.

Logs locais: `target/adc-sbb-workspace-tests.log`,
`target/adc-sbb-native-release-tests.log`, `target/adc-sbb-doctests.log` e
`target/adc-sbb-workspace-clippy.log`. Nenhuma execução remota de CI, validação de
jogo comercial ou execução de Miri é declarada por esta rodada.

## SHL/SHR/SAR — validação Linux de 2026-10-19

Mesmo ambiente Linux x86_64 com Rust/Cargo/Clippy 1.99.0 do Fedora; o pin 1.98.1
permanece no repositório. Política de contagens e flags: [ADR-0015](../adr/0015-flags-shifts-inteiros.md).

- `RUSTFLAGS='-D warnings' cargo test --workspace --all-targets --features xbox-cpu/isa-oracle`:
  177 testes passaram, incluindo a integração que compila o Rust gerado e executa
  seus 24 testes nativos. Os mesmos 24 testes passaram em `--release`.
- 52470 comparações físicas cobrem SHL/SHR/SAR × 8/16/32 bits, CL, imediatos e um
  implícito. Outros 393216 casos esgotam valores de byte e contagens 0..255 com
  dois estados de flags de entrada. A matriz mascara somente flags indefinidas;
  testes separados exigem sua normalização para zero.
- Testes estruturais cobrem AH/CL/CX/ECX, memória de 8/16/32 bits, contagem zero
  ou mascarada para zero, falhas de leitura/escrita sem comprometer CPU e snapshot
  entre shift e ADC. Dois doctests e Clippy dos três pacotes alterados com
  `--no-deps --all-targets --features xbox-cpu/isa-oracle -- -D warnings` passaram.
- O executável nativo otimizado continua produzindo EAX=62 em 37 instruções.
  As pendências globais de Clippy registradas acima continuam fora desta etapa;
  não há resultado novo de CI remoto, Miri ou jogo comercial.

Logs: `target/shifts-workspace-tests.log`, `target/shifts-native-release-tests.log`
e `target/shifts-doctests.log`. O log dos 24 testes gerados também fica em
`target/recomp-tests/<execução>/native-test-results.txt`.

## Rotações e shifts duplos — validação Linux de 2026-10-19

Mesmo host Linux x86_64 com Rust/Cargo/Clippy 1.99.0 do Fedora. O pin 1.98.1
permanece no repositório. Regras e limites: [ADR-0016](../adr/0016-rotacoes-shifts-duplos.md).

- `RUSTFLAGS='-D warnings' cargo test --workspace --all-targets --features xbox-cpu/isa-oracle`:
  179 testes passaram. A integração compilou e executou 35 testes nativos; os
  mesmos 35 passaram em `--release`, também com warnings como erros.
- 70488 comparações físicas cobrem ROL/ROR/RCL/RCR em 8/16/32 bits com CL,
  imediatos e um implícito. Outros 524288 casos esgotam valores/contagens de byte
  e os dois carries. SHLD/SHRD têm 196988 comparações de resultados definidos.
- Foram verificados AH/CL/CX/ECX, aliases entre fonte/destino/contagem, voltas
  completas, memória de largura exata, falhas de leitura/escrita e snapshot entre
  RCL/RCR/SHLD. Contagens efetivas 17..31 de word são verificadas como erros
  `UndefinedDoubleShift` com PC, sem modificar CPU ou acessar o operando de memória.
- Dois doctests, formatação, verificação do diff e Clippy dos três pacotes
  alterados (`--no-deps --all-targets --features xbox-cpu/isa-oracle -- -D warnings`)
  passaram. O executável nativo otimizado continua com EAX=62 e 37 instruções.
- As pendências globais de Clippy registradas acima permanecem. Esta etapa não
  declara Miri, CI remoto, FPS, comportamento de resultados indefinidos em Xbox
  real ou compatibilidade comercial.

Logs: `target/rotations-workspace-tests.log`,
`target/rotations-native-release-tests.log`, `target/rotations-doctests.log` e o
`native-test-results.txt` da execução em `target/recomp-tests`.

## MUL/IMUL e DIV/IDIV — validação Linux de 2026-10-19

Mesmo host Linux x86_64, Rust/Cargo/Clippy 1.99.0 do Fedora; pin 1.98.1 preservado.
Decisões e limites: [ADR-0017](../adr/0017-multiplicacao-divisao-de.md).

- `RUSTFLAGS='-D warnings' cargo test --workspace --all-targets --features xbox-cpu/isa-oracle`:
  180 testes passaram. O projeto gerado passou em 45 testes nativos, em debug e
  release, também com warnings como erros.
- MUL/IMUL: 2156 comparações gerais e 131072 pares exaustivos de byte contra a
  referência física. As formas truncadas de IMUL são comparadas com os bits baixos
  e CF/OF do produto implícito equivalente.
- DIV/IDIV: 40548 entradas de vetores/geração determinística e 1310720 entradas
  cobrindo todos os dividendos AX com dez divisores selecionados. Foram 350688
  resultados físicos estreitos e 1000580 diagnósticos classificados.
- No total, 483916 comparações físicas novas. A referência pré-checa a faixa por
  divisão física de 64 bits e não executa #DE nativo; entrega de exceção por IDT
  não foi implementada nem validada. O runtime retorna erro com PC e motivo.
- Testes estruturais cobrem AH/EAX/EDX como fontes, partes preservadas, memória
  read-only, leitura inválida, resto negativo, overflow, zero, CBW/CWDE/CWD/CDQ e
  snapshot entre produto e divisão. O binário otimizado mantém EAX=62, 37 instruções.
- Dois doctests, `cargo fmt --all --check`, `git diff --check` e Clippy dos três
  pacotes alterados com `--no-deps --all-targets --features xbox-cpu/isa-oracle`
  e `-D warnings` passaram. Pendências globais de Clippy anteriores permanecem;
  sem declaração de CI remoto, Miri, FPS ou compatibilidade comercial nesta etapa.

Logs: `target/muldiv-workspace-tests.log`, `target/muldiv-native-release-tests.log`,
`target/muldiv-doctests.log` e o `native-test-results.txt` da execução. A integração
agora usa `--nocapture` no projeto gerado para registrar separadamente resultados
físicos e diagnósticos de divisão.

## Loops e strings REP — validação Windows de 2026-10-20

Host x86_64-pc-windows-gnu com o toolchain fixado Rust 1.98.1.
Regras e limites: [ADR-0018](../adr/0018-strings-rep-retomaveis.md).

- `RUSTFLAGS='-D warnings' cargo test --workspace --all-targets --offline --features xbox-cpu/isa-oracle`:
  183 testes passaram. O projeto gerado passou em 55 testes nativos em debug e
  release, também com warnings como erros. Dois doctests passaram.
- 3456 comparações de strings com a CPU física verificam memória, EAX, índices,
  contador e flags em 8/16/32 bits, com DF nas duas direções e diferentes prefixos.
  As 72 variantes geradas incluem address16, verificado por testes semânticos.
- Oito variantes de JCXZ/JECXZ/LOOP/LOOPE/LOOPNE cobrem wrap, CX alto preservado,
  flags e caminhos tomados/não tomados. ECX inicial zero em LOOP faz wrap;
  orçamento interrompe esse laço e REP com ECX máximo sem depender do relógio.
- Snapshots no meio de REP preservam a continuação, incluindo flags originais de
  comparações. Há retomada após falta/remapeamento, sobreposição de MOVS, zero sem
  acesso à memória, CLD/STD e falha de escrita sem alteração parcial da RAM.
- `AOTC` v2 guarda a comparação em andamento; v1 migra com continuação vazia.
  Não há mudança nos bytes de RAM nem no formato do snapshot do bus.
- Formatação, diff e Clippy dos pacotes `xbox-recomp`, `xberecomp` e `xbox-cpu`
  passaram com `--no-deps --all-targets --features xbox-cpu/isa-oracle -- -D warnings`.
  `xbox-mem` mantém os 19 avisos globais anteriores; nenhum novo nesta etapa.
  Isso não declara Miri, CI remoto, validação Linux desta alteração ou jogos comerciais.

Logs: `target/strings-workspace-tests.log`, `target/strings-native-release-tests.log`,
`target/strings-doctests.log` e `target/strings-memory-clippy.log`. O caminho do
projeto gerado validado está em `target/strings-native-project.txt`.

## Controle indireto — validação Windows de 2026-10-20

Host x86_64-pc-windows-gnu, Rust 1.98.1 fixado. Decisões e limites:
[ADR-0019](../adr/0019-controle-indireto-aot.md).

- Workspace: 189 testes passaram com `--workspace --all-targets --offline
  --features xbox-cpu/isa-oracle` e `RUSTFLAGS=-D warnings`. Dois doctests passaram.
- O Rust gerado passou em 64 testes nativos em debug e release, também com warnings
  como erros. A prova original mantém EAX=62 e 37 instruções concluídas.
- Seis testes de análise verificam descoberta iterativa, MOV/LEA locais, ponteiros
  imutáveis, limites de seção, raízes adicionais, invalidação conservadora e limites
  de recursos. Não há varredura linear de dados nem pressuposto de valores de RW.
- Nove testes nativos novos cobrem 15 combinações de CALL/JMP por registrador,
  callbacks mutáveis, SIB, tabelas indexadas, CALL [ESP]/[ESP-4], retorno validado,
  faltas de leitura/execução/pilha, destinos desconhecidos, recursão, orçamento e
  snapshot com CALL pendente. Os resultados esperados são independentes do emissor;
  não há novo oráculo que execute instruções guest ou ponteiros host arbitrários.
- A CLI é exercitada com entradas decimais/hexadecimais repetidas, duplicatas e
  destinos inválidos, antes de criar artefatos. O manifesto/source esperado é
  comparado com a tradução equivalente pela API.
- Formatação, diff e Clippy de `xbox-recomp`, `xberecomp` e `xbox-cpu` passaram com
  `--all-targets --features xbox-cpu/isa-oracle --no-deps -- -D warnings`. Não há
  mudança em `xbox-mem`; as pendências globais anteriores continuam registradas.
  Esta etapa não declara Miri, CI remoto, validação Linux, FPS ou jogos comerciais.

Logs: `target/indirect-workspace-tests.log`, `target/indirect-native-release-tests.log`
e `target/indirect-doctests.log`. O caminho do projeto gerado validado está em
`target/indirect-native-project.txt`.

## Loader compartilhado — validação Windows de 2026-10-20

A sessão AOT agora usa `xbox-kernel::loader::Image` em modo Aot e reserva sua pilha
separadamente. A inspeção de mídia real usa outro modo, com alias físico e sem
execução. Páginas RWX continuam recusadas. O perfil TLS nxdk e imports de funções
agora usam FS explícito e `Exit::KernelCall`; serviços e dados exportados ainda
estão pendentes. Veja [homebrew.md](homebrew.md) para a build real e a fronteira ABI.

Passaram 200 testes de workspace, 64 testes nativos em debug/release e dois
doctests. Clippy global passou, corrigindo os 19 avisos de memória registrados
nas entradas históricas acima. T14 real e contratos sob Miri estão detalhados em
[memoria.md](memoria.md). O marco 1/6 do alpha está em [alpha.md](alpha.md).

## CMPXCHG/LOCK em RAM e SETcc — ADR-0024

CMPXCHG suporta 8/16/32 bits, registradores e RAM, incluindo LOCK em destino
de memória. A escrita do destino acontece também quando a comparação falha;
falhas de página/permissão não comprometem CPU ou parte da RAM. O acesso é
indivisível no runtime com posse exclusiva do bus, sem yield durante a instrução.
MMIO e CMPXCHG8B continuam recusados. SETcc suporta as 16 condições em byte
baixo/alto/memória, sem alterar flags.

O teste gerador passou a 79 casos em x86_64, com 147456 comparações adicionais
de CMPXCHG contra a CPU física, mais aliases, FS/SIB, RO, página cruzada e
snapshot/reexecução. Nessa entrega, o nxdk hello passou a parar em RDTSC (`0x2055B`) durante a
tradução. Detalhes e contrato de LOCK: [ADR-0024](../adr/0024-cmpxchg-setcc.md).

## Clock determinístico e RDTSC — ADR-0025

RDTSC lê `State::clock`, reutilizando `xbox_core::time::Clock`, em EDX:EAX e
preserva flags. O AOT cobra um ciclo virtual por instrução ou elemento REP
concluído; esse custo é sintético e não representa a latência física do Pentium III.
Fatias diferentes preservam os timestamps; faltas, orçamento zero e polls HLE
não cobram passos extras. O limite u64 causa erro antes da próxima instrução.

Snapshot AOTC v4 inclui perfil de tempo 1 e CLK0. Versões v1–v3 migram com Clock
zero, sem inferir tempo de `retired`. O Scheduler ainda precisa ser conectado ao
relógio único da sessão; não criar relógios independentes para threads futuras.
Projetos antigos devem ser regenerados para incluir a contabilização.

Nessa entrega, o projeto gerado chegou a 84 testes em x86_64 e a tradução encontrou CLI no
PC `0x1409E`, seguido de HLT no caminho de assert do SDK. Isso não significa que
o homebrew tenha executado ou um assert tenha disparado. Contrato, migração e
limites: [ADR-0025](../adr/0025-relogio-aot-rdtsc.md).

## CLI e HLT — ADR-0026

CLI limpa apenas IF guest no perfil privilegiado atual. HLT conclui e suspende
a CPU, com EIP na instrução seguinte e `Exit::Halted { pc, interrupts_enabled }`.
A continuação também é traduzida; um opcode faltante após HLT continua sendo erro.
O main gerado relata PC, próximo EIP e IF e encerra com erro, sem travar o host.

Não há IRQ/NMI ou retomada automática: polls preservam CPU, memória e tempo,
mesmo com orçamento zero. AOTC v5 salva a parada; v1–v4 migram com CPU ativa,
conservando as regras anteriores de tempo. Essa entrega chegou a 88 testes nativos em x86_64, mais
verificação do diagnóstico do main. Nessa referência, o bloqueio real era INT3 em `0x20D6C`,
numa região `0xCC` após CALL, ainda durante a tradução. Investigar o fluxo sem
presumir execução nem remover bytes. Contratos e referências:
[ADR-0026](../adr/0026-controle-cli-hlt.md).

## INT3 e fronteira #BP — ADR-0027

INT3 curto (`0xCC`) conclui com EIP seguinte e `Exit::Breakpoint { pc }`, vetor 3.
Preserva o contexto anterior à entrega da exceção; o main relata #BP e encerra
com erro. Não há IDT, frame de interrupção, SEH ou retomada automática. Polls
continuam devolvendo a exceção sem executar código ou acrescentar tempo.

O caminho termina na exceção, sem fallthrough. Destinos posteriores alcançados
por branches/`--entry` continuam sendo traduzidos. O preenchimento após abort
explica o bloqueio encontrado, mas não autoriza retirar bytes ou presumir
`noreturn` para funções arbitrárias. AOTC v6 salva o breakpoint; v1–v5 migram
sem essa pendência, mantendo FS, tempo e HLT conforme cada versão.

Nessa entrega, o conjunto chegou a 92 testes do Rust gerado em x86_64; os mains
de HLT e INT3 também foram executados. O bloqueio seguinte foi SFENCE em
`0x12A43`, tratado no ADR-0028 abaixo. Detalhes da fronteira #BP:
[ADR-0027](../adr/0027-int3-breakpoint-aot.md).

## SFENCE e ordenação do bus — ADR-0028

SFENCE tem operação própria e chama `Bus::ordenar_escritas`. Stores de RAM já
são síncronos e observáveis em ordem pelas APIs da arena exclusiva; não há fila
a drenar. A barreira atual tem corpo vazio por esse contrato, sem fence física
host nem publicação fictícia de dados de GPU. Antes de habilitar escritas
diferidas ou observadores concorrentes, ampliar o protocolo do bus.

Preserva registradores/flags, continua a descoberta e custa um passo sintético.
Snapshots continuam AOTC v6: não há estado novo. Quatro testes nativos adicionais
cobrem aliases/DMA, tempo, orçamento, permissões e replay, levando o conjunto a
96 casos em x86_64. O bloqueio seguinte foi CMOVNE em `0x13B88`. Contrato,
limites de MMIO e referências: [ADR-0028](../adr/0028-sfence-ordenacao-do-bus.md).

## CMOVcc — ADR-0029

As 16 condições suportam destino registrador e origem registrador/memória de
16/32 bits. A origem é lida antes da condição: uma condição falsa não evita
falta de página ou efeito de leitura MMIO. Flags são preservadas; word conserva
a metade alta do destino. Não há variante byte, LOCK ou REP habilitados.

Há 32768 comparações do Rust gerado com CMOV físico no oráculo opcional,
mais testes portáveis de condições, memória RO, faltas, aliases, FS/SIB, SP/ESP,
snapshots e tempo. São 103 testes nativos em x86_64, com AOTC ainda v6.
O próximo bloqueio real é BSR em `0x1657C`, seguido de JZ; tratar origem zero,
ZF e a política de resultados indefinidos ao implementar. Detalhes:
[ADR-0029](../adr/0029-cmovcc.md).
