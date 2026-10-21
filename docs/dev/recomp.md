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

O snapshot da prova inicial inclui CPU e bus. Para serviços com objetos HLE,
usar `Snapshot` da `Session` completa, incluindo `kernel` (ADR-0032).
Não equivale a save state de um jogo completo:
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
O bloqueio seguinte foi BSR em `0x1657C`, seguido de JZ, tratado no ADR-0030.
Detalhes de CMOVcc:
[ADR-0029](../adr/0029-cmovcc.md).

## BSF/BSR — ADR-0030

BSF/BSR suportam 16/32 bits e origem registrador/memória. Para origem não zero,
gravam o índice do bit menos/mais significativo e limpam ZF. Em zero, marcam
ZF e preservam o destino como política determinística do runtime. As demais
flags aritméticas indefinidas são zeradas; IF/DF e demais bits são preservados.
Não habilita TZCNT/LZCNT. AOTC permanece v6 e o tempo continua no perfil 1.

O projeto gerado tem 110 testes em x86_64 (94 em outros hosts). As 262144
comparações físicas conferem ZF e, em origem não zero, o destino; os valores
indefinidos da CPU física não são usados como contrato. Outros testes cobrem
word, memória RO/faltante, aliases, FS/SIB, SP/ESP, JZ, snapshots e tempo.

A análise do fluxo atualmente descoberto do hello passa com 8725 instruções e
blocos. Isso não garante todas as entradas de threads/callbacks nem serviços
implementados. Contratos e resultado integrado:
[ADR-0030](../adr/0030-bsf-bsr.md).

## Funções de bloco e execução real — ADR-0031

O primeiro binário do hello compilou, mas estourou a pilha host por concentrar
milhares de blocos em uma função. Agora cada bloco é uma função não inlineada;
o dispatcher seleciona um ponteiro de função e chama um bloco por passagem.
Limite final: 32 instruções, aplicado depois da inferência de destinos.
CALL/RET guest continuam em EIP/ESP/RAM, sem recursão nativa. Sequências longas
podem ceder antes no orçamento de blocos, sem mudar tempo guest ou AOTC v6.

Uma regressão executa 1000 ADDs lineares e 1000 pares ADD/JMP em pilha host
solicitada de 128 KiB. O conjunto final tem 111 testes nativos em x86_64.
Com `--entry 0x19110`, o hello real traduz 8769 instruções/blocos, compila e
executa até NtCreateMutant (192), retorno reportado `0x18CFA`, argumentos
`0x28EEC, 0, 0`. Sem essa raiz explícita, a execução recusa o destino do CALL
em `0x209D2`. Reprodução e limites: [ADR-0031](../adr/0031-blocos-aot-pilha-host.md).

## NtCreateMutant e snapshot integral — ADR-0032

O runtime agora mantém `Session { state, bus, kernel }`; a chamada HLE usa
`session.atender_kernel()` sem dispatcher externo. `NtCreateMutant` cria mutexes
sem nome, devolve handles guest e registra propriedade inicial. Nomes/flags,
espera, liberação, fechamento e threads ainda precisam de implementação.

O snapshot integral usa `AOTS` v1 e inclui `AOTC` v6, bus, `KRNL` v1 e `KMUT` v1.
Um snapshot só de CPU/bus não preserva objetos de kernel. A continuação da
chamada é validada e capturada antes de efeitos, inclusive se a saída sobrepõe
a pilha de retorno. Regenerar projetos que usam a API anterior.

O conjunto gerado tem **115 testes em x86_64** (99 nos outros hosts), em debug
e release; quatro casos adicionais validam estado/snapshot, replay, retorno
inválido, sobreposição e falha NTSTATUS sem objeto. O workspace tem 271 testes
com doctests. O relatório registra **2/69 serviços**, com variantes limitadas.

No hello real, usando a mesma raiz explícita, a execução passou por NtCreateMutant
e chegou a **XcSHAInit (335)**, retorno `0x19140`, argumento `0x28EF0`.
O próximo passo nessa entrega era o contrato SHA do caminho `init_rand_s`.
Detalhes: [ADR-0032](../adr/0032-mutants-guest-snapshot-sessao.md).

## SHA-1 em contexto opaco guest — ADR-0033

`XcSHAInit`, `XcSHAUpdate` e `XcSHAFinal` compartilham estado de 116 bytes na
RAM guest, sem tabela host de contextos. Há inicialização real, compressão SHA-1,
digest de 20 bytes e validação antes de stores. Cópia do contexto e snapshot
parcial funcionam; não mudaram as versões de snapshot da sessão/CPU/kernel.

O formato é **HLE v1**, não um layout privado comprovado do kernel físico.
Final preserva a origem por política local; saída sobreposta a modifica por
store normal. Casos que inspecionam campos privados exigirão nova evidência.
Sem estado oculto, dependência nova ou unsafe.

O conjunto gerado tem **118 testes em x86_64** (102 nos demais hosts), em debug
e release. **288 testes de workspace/doctests** passaram. Os testes usam vetores
públicos, referências .NET em bordas de padding/páginas e sete divisões de entrada,
além de erros, cópia, proteção, MMIO, sobreposição e replay da sessão.

O hello real passou por SHAInit e parou em **ExQueryNonVolatileSetting (24)**,
retorno `0x19176`, índice `0xFFFF`, buffer de 256 bytes em `0x7000FD80`.
Update/Final foram validados em testes, sem execução observada no hello ainda.
O relatório registrava **5/69 serviços**; a consulta de EEPROM era o próximo passo.
Detalhes e logs: [ADR-0033](../adr/0033-sha1-contexto-guest.md).

## Consulta integral da EEPROM — ADR-0034

`ExQueryNonVolatileSetting` (24) atende `0xFFFF`, copiando os 256 bytes da EEPROM
da sessão, com tipo/tamanho e NTSTATUS. Demais índices permanecem como variantes
sem suporte. As três saídas são validadas antes da primeira escrita; sondagem
com buffer curto publica só o tamanho, conforme política local documentada.

`EepromState` registra bytes e origem. O padrão usa o gerador sintético existente
em NTSC-M/português; não lê BIOS, arquivo ou configuração do host. API permite
uma imagem explícita, preservando bytes fornecidos. Consulta devolve cópia;
persistência, `ExSave` e ligação a `SystemConfig` continuam pendentes.
`KRNL` v2 acrescenta `EEPR` v1, mantendo bytes/origem no snapshot; KRNL v1 migra
para o padrão sintético. AOTS v1/KMUT v1/AOTC v6 não mudaram.

**301 testes de workspace/doctests** e **121 testes nativos em x86_64** (105 nos
demais hosts), em debug/release, cobrem consulta, erros, aliases e snapshot,
incluindo imagem fornecida com hash esperado calculado independentemente.
O relatório registra **6/69 serviços**. O hello passou pela consulta e parou
em **NtWaitForSingleObject (233)**, retorno `0x16B99`, argumentos `4, 0, 0`.
O próximo trabalho nessa etapa era aquisição/espera real de mutex. Sem framebuffer ainda.
Detalhes e logs: [ADR-0034](../adr/0034-consulta-eeprom-sessao.md).

## Sincronização e perfil de MSRs guest — ADRs 0035–0038

Espera/liberação de mutant e seções críticas recursivas já têm estado guest
efetivo; casos que exigem scheduler continuam recusados. O kernel tem 12/69
serviços, com variantes limitadas. Seções críticas usam os 28 bytes Xbox na RAM,
sem tabela host por endereço (ADR-0037).

RDMSR consulta ECX completo e publica EDX:EAX, preservando flags e outros
registradores. `State::cpu_profile` é `CpuProfile::XboxRetailV1` (ID 1):
MSR 0x10 lê o mesmo Clock de RDTSC; 0x2A devolve a configuração nominal
0x01040000, bus 133⅓ MHz × razão 5,5. Demais campos são sintéticos, não dump de
Xbox. Outros índices dão erro com PC/índice sem efeitos ou cobrança do passo.
WRMSR/CPL/CR4/#GP via IDT continuam pendentes; não há RDMSR host.

Na entrega do ADR-0038, o snapshot integral era AOTS v1 com **AOTC v7**, bus, KRNL v2, KMUT v2 e
EEPR v1. AOTC v7 acrescenta o perfil de CPU e migra v1–v6 explicitamente para
o perfil 1; Clock continua único, com custo sintético do perfil de tempo 1.
IDs desconhecidos, versão zero e truncamentos são recusados.

Com `--entry 0x19110 --entry 0x145E0 --entry 0x205E0 --entry 0x20889`, o hello
traduz **9027 blocos** e executa até **HalReadWritePCISpace (46)**, retorno
`0x2064D`, leitura de quatro bytes do offset PCI `0x6C` (bus 0, slot `0x60`).
A última raiz é o case que retorna multiplicador 55; a tabela passa por um slot
de pilha ainda não inferido. Não é remoção de código ou desvio do construtor.

Validação: **322 testes workspace/doctests e 131 casos nativos em debug/release**
(115 fora de x86_64), Rust 1.98.1. Os seis casos novos de Rust emitido cobrem
preservação, TSC de 64 bits, erro/retry, tempo/orçamento/polls, snapshot integral
e falta de store posterior à leitura. Próximo passo: configuração PCI guest e
clock do chipset para atender o serviço 46. Threads/execução completa/framebuffer
permanecem abertos. Contrato e fontes: [ADR-0038](../adr/0038-rdmsr-perfil-cpu-guest.md).

## Configuração PCI de clock — ADR-0039

HalReadWritePCISpace (46) atende somente o PLL do controlador DRAM: bus 0,
slot NT 0x60 (device 0/function 3), offsets 0x6C..0x70. O valor público
0x01230801 tem divisor 1/multiplicador 8. Subleituras retornam fatias little-endian;
o destino inteiro é validado como RAM gravável antes de escrever. Escritas,
outras funções e regiões recusam explicitamente. VOID retorna EAX=0 por convenção.

`xbox_core::hardware::RetailClockV1` centraliza EBL_CR_POWERON/PLL. A frequência
derivada é 733333333 Hz, coerente com Clock; outros efeitos dos campos não são
modelados. `PciConfig` pertence ao dispatcher. **KRNL v3/PCIC v1** salva o perfil;
KRNL v1/v2 migram para ele preservando o estado anterior. AOTS v1/AOTC v7/KMUT v2/
EEPR v1 permanecem. PCI/MMIO futuros devem usar a mesma autoridade de estado.

O hello mantém as quatro raízes/9027 blocos e passa pela consulta PCI. Uma
verificação local do XBE real confirmou `tsc_freq=733333333` em 0x27D10/0x27D14.
O próximo bloqueio é **InterlockedExchange (54)**, retorno 0x19DC7, argumentos
0x29158 e 1, FASTCALL/8 (ECX/EDX). Troca e valor anterior precisam de efeitos
reais na RAM, sem sucesso genérico. **331 testes workspace/doctests e 135 casos
nativos debug/release**, mais a verificação local do hello; 13/69 serviços com
variantes limitadas. Fontes, snapshot, limites e logs:
[ADR-0039](../adr/0039-configuracao-pci-clock-guest.md).

## InterlockedExchange em RAM — ADR-0040

O ordinal 54 agora troca LONG de 32 bits e retorna os bits anteriores. Argumentos
são ECX/EDX (FASTCALL/8); a retomada só remove os quatro bytes do retorno. O alvo
inteiro precisa ser RAM RW, inclusive se o valor não mudar. Desalinhamento e
páginas cruzadas seguem o Bus funcional; MMIO/RO/W sem R e faixas inválidas
recusam antes de acessar o alvo. Não há yield entre load/store e a ordenação usa
o contrato síncrono/exclusivo do Bus. Observadores concorrentes futuros exigem
rever esse protocolo; não há CAS/fence físico host implícito.

Sem estado host novo, AOTS v1/AOTC v7/KRNL v3/KMUT v2/EEPR v1/PCIC v1 permanecem.
Snapshot preserva chamada pendente, troca concluída e efeitos na RAM. A sessão
captura o retorno antes do serviço, incluindo Target sobre a pilha de retorno.
Outros serviços Interlocked continuam ausentes; somente 54 é importado pelo hello.

Nessa entrega, o XBE real manteve quatro raízes/9027 blocos, passou pela troca e
parou em **KeQuerySystemTime (128)**, retorno 0x20D8B, argumento 0x7000FDD8.
O contrato temporal e o snapshot dessa referência foram implementados no ADR-0041,
descrito a seguir, usando o Clock da sessão e uma referência UTC explícita.
Validação: **336 testes workspace/doctests e 140 nativos debug/release**, mais
verificação local do efeito no XBE real; 14/69 serviços. Fonte/contratos/logs:
[ADR-0040](../adr/0040-interlocked-exchange-guest.md).

## Tempo de sistema derivado do Clock — ADR-0041

KeQuerySystemTime (128), stdcall/4 VOID, escreve LARGE_INTEGER little-endian
em RAM gravável após validar os oito bytes. A referência UTC no power-on é
explícita: padrão sintético 2000-01-01, ou valor fornecido por
SystemTimeReference::from_boot_ticks. A consulta soma floor(ciclos × 10000000 /
CPU_HZ), com u128 intermediário e limite de LARGE_INTEGER. O dispatcher recebe
o Clock real de Session por atender_com_clock; atender sem contexto recusa essa
consulta. Não usa hora host, retired, Clock::nanos ou outro contador temporal.

KRNL v4/STIM v1 guarda a referência, mantendo AOTS v1/AOTC v7/KMUT v2/EEPR v1/
PCIC v1 e o Clock em AOTC/CLK0. KRNL v1–v3 migram para o padrão, sem herdar a
configuração de destino. Carga transacional, saída parcial/MMIO/RO/overflow,
continuação sobre a pilha e chamadas pendentes são cobertas por testes.

Naquela entrega, o hello manteve quatro raízes/9027 blocos. Aos 704 ciclos, a
consulta real escreveu 125911584000000009 em 0x7000FDD8 e avançou até
**KeGetCurrentIrql (103)**, retorno **0x20E09**. O modelo explícito de IRQL e seu
snapshot foram implementados no ADR-0042, descrito a seguir.

Validação: **342 testes workspace/doctests e 145 nativos debug/release**, seis
novos testes de kernel e cinco nativos, mais teste local sobre o XBE real;
**15/69 serviços**. Fontes, contrato e logs:
[ADR-0041](../adr/0041-tempo-sistema-guest.md).

## Consulta de IRQL explícito — ADR-0042

KeGetCurrentIrql (103), stdcall/0, consulta o Irql guardado pelo kernel para a
única CPU guest. Padrão Passive (0), com Apc (1) e Dispatch (2) representáveis
como contexto inicial. CLI e EFLAGS.IF não alteram IRQL. A consulta estende KIRQL
a u32 em EAX e preserva flags/Clock/retired/RAM; a continuação remove só a palavra
de retorno. Tail-call e chamada pendente usam o contrato existente de Session.

KRNL v5/IRQL v1 salva o nível em um byte. KRNL v1–v4 migram para Passive, sem
inferência de IF ou herança do destino. Carga de kernel é transacional, recusando
versões/níveis sem modelo e truncamento. AOTS v1/AOTC v7/KMUT v2/EEPR v1/PCIC v1/
STIM v1 permanecem. Raise/Lower, DIRQL, IRQ/APC/DPC e KPCR/APIC físicos faltam;
configurar o contexto inicial não executa transições ou entrega de eventos.

Naquela entrega, o hello manteve quatro raízes/9027 blocos e consultou Passive
com IF=0. O bloqueio seguinte era **NtAllocateVirtualMemory (184)**, retorno **0x20E68**:
argumentos 0x7000FD94, 0, 0x7000FD98, 0x3000, 4; *BaseAddress=0 e
*RegionSize=0x10000 (64 KiB). Esse caminho foi implementado no ADR-0043,
descrito a seguir, com reserva+commit privado e snapshot das regiões.

Validação: **346 testes workspace/doctests e 148 nativos debug/release**, quatro
novos testes de kernel e três nativos, mais teste local do XBE real; **16/69 serviços**.
Fontes, contrato e logs: [ADR-0042](../adr/0042-irql-guest.md).

## Reserva+commit privado guest — ADR-0043

NtAllocateVirtualMemory (184), stdcall/20, atende base nula/ZeroBits=0/flags 0x3000/
PAGE_READWRITE em Passive. First-fit após o intervalo completo XBE, base 64 KiB,
tamanho 4 KiB e limite inferior à pilha. Frames próprios são zerados antes de
publicar cópias planejadas de MMU/bitmap, parâmetros e região lógica. A arena não
é copiada. Saídas passam por validação R/W/alias físico; faltas e OOM não deixam
alocação parcial. RW sem execução é política AOT; outras variantes e liberação faltam.

KRNL v6/VMEM v1 salva intervalo XBE e regiões; Bus guarda RAM/MMU/bitmap. Session
confere headers, posse física, permissões e aliases. KRNL v1–v5 migra sem regiões;
Session recupera o intervalo do XBE correspondente. FRM0 v1 passa a validar contagem/
bitmap/padding antes de alterar estado. Nenhum unsafe novo. AOTS v1/AOTC v7/KMUT v2/
EEPR v1/PCIC v1/STIM v1/IRQL v1 permanecem.

O hello mantém quatro raízes/9027 blocos. O teste real confere 64 KiB em 0x30000,
16 frames zerados/RW e uso pelo CRT, que passa heap+8 à nova thread. O próximo
bloqueio é **PsCreateSystemThreadEx (255)**, retorno **0x13BF1**: pilha de 64 KiB,
TLS de 0x114 bytes, StartRoutine=0x18FC0, contexto=0x30008, SystemRoutine=0x13C70.
Modelar thread guest e continuação serializável, sem pular para main ou criar thread host.

Validação: **354 testes workspace/doctests e 153 nativos debug/release**, sete
novos testes de kernel, um de frames e cinco nativos, mais teste local do XBE real;
**17/69 serviços**. Fontes/contrato/logs: [ADR-0043](../adr/0043-alocacao-virtual-guest.md).

## NOP multibyte e entradas de thread — ADR-0044

NOP r/m16 e r/m32 (`0F 1F /0`) usam a IR Nop existente. ModR/M/SIB/deslocamento
não provocam cálculo ou acesso ao endereço descrito. Flags/registradores ficam
preservados; EIP avança pelo comprimento completo, com custo de uma instrução
do perfil AOT. LOCK/REP/PAUSE e segmentos fora do perfil seguem recusados.
Snapshots e dependências não mudaram; sem novo unsafe.

Os testes cobrem comprimentos, destino indireto através do NOP, todas as flags
aritméticas, endereço ausente/MMIO/FS, orçamento, Clock e snapshot. A suíte tem
**357 testes workspace/doctests e 156 casos nativos debug/release** em x86_64
(140 em outros hosts).

Antes de criar a thread, informar também `--entry 0x13C70 --entry 0x18FC0`.
Essas entradas revelaram o NOP em `0x13C68`; com ele, as seis raízes traduzem
**9454 blocos**. O módulo real compila/executa, mas ainda para em
**PsCreateSystemThreadEx (255)**. Traduzir o startup não cria nem executa a thread.
O próximo trabalho continua sendo o modelo guest com identidade, pilha/TLS,
SystemRoutine, contextos, espera/término e snapshot. Fonte e evidência:
[ADR-0044](../adr/0044-nop-multibyte-raizes-thread.md).

### Bootstrap de thread guest (ADR-0045)

Session executa uma filha com pilha/TLS/TIB próprios enquanto guarda a continuação
do pai. O guest passa por SystemRoutine/StartRoutine; o kernel implementa criação,
KeGetCurrentThread e término, com 20/69 serviços. Clock, retired, perfil de CPU e
IRQL são únicos. Mutants e seções críticas usam a identidade ativa. Espera na
filha terminada funciona; espera viva, preempção, yield/Sleep, múltiplas filhas,
NtClose e liberação seguem pendentes.

O main gerado trata RET da filha como término local antes de anunciar retorno
do processo. Snapshot AOTS v2/KRNL v7/KMUT v3 acrescenta THRD/TCXT v1; versões
antigas migram sem filha/pai suspenso. Usar Snapshot de Session completo.

O hello executa bootstrap/TLS/FLS e revela duas raízes novas: `0x18C90`
(main_wrapper) e `0x139B0` (inicializador). Somadas às seis anteriores, traduzem
10444 blocos. O módulo executado para no **JMP 0x14E86 → 0x14F14**, próximo case
a incluir/inferir. Ainda sem execução completa ou framebuffer. Validação atual:
365 testes workspace/doctests, 162 casos gerados debug/release (146 fora de
x86_64). Fontes, variantes, snapshot e comando completo no
[ADR-0045](../adr/0045-bootstrap-thread-guest.md).

### Namespace de D: (ADR-0046)

A raiz adicional `--entry 0x14F14` resolve o próximo case do formatador sem
alterar a ISA/inferência; as nove raízes traduzem 10676 blocos. NtOpenSymbolicLinkObject
(203), IoCreateSymbolicLink (67) e NtClose (187, só links) atendem o automount do
nxdk. D: começa ausente e recebe uma cópia do diretório de CdRom0 fornecido pelo
guest; abrir/fechar usa o contador compartilhado de handles. Não há montagem de
mídia na VFS, outras unidades ou query/delete de links.

KRNL v8 acrescenta SYML v1; v1–v7 migram sem namespace, sem herdar o destino da
sessão de restauração. O hello confirmou D: = `\Device\CdRom0\` e chegou à
chamada de vídeo **AvSendTVEncoderOption (2), retorno 0x12769, opção 6**. Ainda
sem implementação desse serviço/framebuffer. Reprodução, perfil e evidência no
[ADR-0046](../adr/0046-namespace-drive-d.md).

### Encoder inicial de vídeo (ADR-0047)

AvSendTVEncoderOption atende GET_SETTINGS (6) derivado da EEPROM e cabo padrão,
e VIDEOENABLE (9) com estado lógico de blanking. AvGetSavedDataAddress usa cold
boot sem framebuffer herdado. São 25/69 serviços com variantes limitadas. Result
é RAM guest validada integralmente; EAX=0 nas chamadas VOID não é o resultado.
Base/opção/parâmetros/configuração sem modelo recusam, sem efeito parcial.

KRNL v9/AVST v1 guarda ID de perfil e enabled; v1–v8 migram para cold boot com
saída desabilitada. Padrão/flags continuam na EEPROM, sem cache ou Clock novo.
As mesmas nove raízes/10676 blocos agora chegam a **MmAllocateContiguousMemoryEx
(166), retorno 0x12C01**, pedido de 1.228.800 bytes/4 KiB/RW|WRITECOMBINE para
640×480×32. Framebuffer, programação de modo/MMIO e apresentação ainda faltam.
Reprodução/evidência no [ADR-0047](../adr/0047-encoder-video-cold-boot.md).

### Memória fisicamente contígua (ADR-0048)

MmAllocateContiguousMemoryEx (166), stdcall/20, reserva frames consecutivos no
bitmap do Bus, respeita limites físicos inclusivos/alinhamento e retorna
VA=0x80000000|PA. Apenas o recurso ganha páginas RW, sem execução. O tamanho é
arredondado a 4 KiB; OOM/fragmentação/faixa insuficiente retorna NULL sem efeito.
Passive e proteção RW com WRITECOMBINE opcional são o perfil atual. A coerência
virtual/física/DMA é síncrona; não há simulação de cache ou arena compartilhada.

KRNL v10/CMEM v1 guarda PA/tamanho/alinhamento/proteção; Session cruza posse,
traduções, permissões, bitmap e ausência de aliases/páginas órfãs na janela.
KRNL v1–v9 migram sem recursos. Liberação/persistência ainda faltam.

O hello alocou 300 páginas, VA=0x80050000/PA=0x50000, zerou o framebuffer e
chegou a **AvSetDisplayMode (3), retorno 0x12CA4**: modo 0x04010101, formato 0x12,
pitch 2560 e PA real. Ainda sem programação/apresentação da imagem. Perfil,
fontes e reprodução no [ADR-0048](../adr/0048-memoria-fisica-contigua.md).

### Modo HLE e leitura do framebuffer (ADR-0049)

AvSetDisplayMode (3), stdcall/24, aceita Step=0 e NTSC-M 640×480/60,
modo 0x04010101, A8R8G8B8. Pitch/PA devem cobrir 480 linhas dentro de um recurso
CMEM registrado; a EEPROM precisa informar NTSC-M. Todas as validações precedem
a publicação do modo. Retorna Step=0 com efeito real sobre VideoState,
preservando blanking e Clock. Variantes/endereços inválidos recusam.

Dispatcher::framebuffer(&Bus) devolve metadados e linhas BGRA emprestadas por
DMA físico, excluindo padding de pitch. O empréstimo impede retomar a CPU com
Bus mutável enquanto usado; não torna a arena Sync. AVST v2 persiste o modo e
migra v1 sem modo; KRNL v10 permanece. Snapshot confere modo/EEPROM/CMEM e a
restauração integral também confere RAM/MMU/bitmap/aliases.

O hello configura o modo com pitch 2560/PA 0x50000, avança após Step=0 e para
em AvSendTVEncoderOption (2), opção 11/Param=5, retorno 0x12DFA. Ainda com saída
desabilitada e buffer preto zerado pelo guest; ajustes de encoder, gamma MMIO
e apresentação faltam. Validação: 394 testes workspace/doctests e 174 nativos
debug/release (158 fora de x86_64). Perfil, fontes e reprodução:
[ADR-0049](../adr/0049-modo-video-framebuffer.md).

### Pedidos de filtros do encoder (ADR-0050, estado atual)

AvSendTVEncoderOption (2), stdcall/16, aceita flicker (11, nível 0..7) e soften
(14, 0/1), com a base NV2A e Result nulo. Guarda cada pedido independentemente,
preservando modo, blanking, RAM e Clock. Ausência de pedido no cold boot é None,
distinta de zero explícito. Parâmetros inválidos recusam sem mascaramento.
Isso não simula o filtro analógico nem transforma os bytes do framebuffer.

AVST v3 acrescenta dois bytes canônicos após o modo opcional; v1/v2 migram sem
pedidos e não herdam os valores do destino. V2 mantém o modo; v1 mantém a
migração anterior sem modo. KRNL continua v10, contador fica em 27/69 serviços.
O próximo bloqueio é a escrita do DAC gamma em 0xFD6813C8, sem mapeamento/modelo;
a saída permanece desabilitada. Não mapear MMIO sobre um destino que descarte
essas escritas. O hello chegou a PC=0x12EEB, com 17232328 retired/ciclos e 42
chamadas atendidas; snapshot e repetição da falta foram conferidos. Validação:
398 testes workspace/doctests e 177 nativos debug/release (161 fora de x86_64).
Perfil, fontes e reprodução:
[ADR-0050](../adr/0050-pedidos-filtros-encoder.md).
