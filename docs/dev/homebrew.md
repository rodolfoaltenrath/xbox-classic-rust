# Homebrew nxdk e fronteira do kernel

## Build local

Alvo: `samples/hello` do nxdk, revisão
`14d5ee97e73347c973f1f57b68b79ec08c9e77f2`. O SDK e seus binários ficam fora
do versionamento. O runtime e o recompilador do Obsidian continuam em Rust;
o sample C é uma entrada externa para testar compatibilidade.

A referência Windows foi compilada no WSL `FedoraLinux-44`:

```powershell
wsl -d FedoraLinux-44 -- bash -lc 'dnf install -y clang lld llvm make cmake bison flex gcc-c++ git python3'
wsl -d FedoraLinux-44 -- bash /mnt/c/Users/Altenrath/emulador-xbox/scripts/build-homebrew.sh
cargo run --offline -p ordinal-report -- target/homebrew/hello.xbe
cargo run --offline -p obsidian -- check-memory target/homebrew/hello.xbe
```

Em Linux, execute `bash scripts/build-homebrew.sh`. O script baixa o SDK na cache
do usuário e fixa os seis submódulos necessários. `NXDK_BUILD_DIR` seleciona outra
cache; revisão diferente ou alterações em arquivos rastreados causam erro.
`BUILD_JOBS` controla o paralelismo. Não há instalação automática de pacotes pelo
script. Clang/LLD, llvm-dlltool, GNU make, GCC/G++, CMake, flex, bison, Git e Python
precisam estar disponíveis.

Neste PC Linux, as ferramentas faltantes foram extraídas de RPMs no cache do
usuário. O Rust 1.98.1 oficial também foi preparado no cache, com SHA-256 conferido.
Para reutilizar este ambiente local (essas pastas não acompanham o Git):

```sh
export PATH="$HOME/.cache/obsidian/rust-1.98.1/bin:$HOME/.cache/obsidian/toolchain/usr/bin:$PATH"
export LD_LIBRARY_PATH="$HOME/.cache/obsidian/toolchain/usr/lib64${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BISON_PKGDATADIR="$HOME/.cache/obsidian/toolchain/usr/share/bison"
export M4="$HOME/.cache/obsidian/toolchain/usr/bin/m4"
bash scripts/build-homebrew.sh
```

Saídas em `target/homebrew`: `hello.exe`, `hello-raw.xbe`, `hello.xbe`,
`build.log`, `build-manifest.txt` e `SHA256SUMS`. O manifesto registra ferramentas
e revisões; `make -B` recompila também as bibliotecas para evitar objetos antigos.
O script não gera ISO e não executa o XBE.

O PE usa `-timestamp:0` e caminhos de fonte normalizados. O cxbe fixado ignora
`SOURCE_DATE_EPOCH` e copia dois bytes não inicializados depois do NUL em seu nome
de biblioteca `CXBE0`. O XBE normalizado ajusta os timestamps de header/certificado
para `1704067200` e zera somente esse padding. O original é preservado. Bytes de
código, seções, TLS e imports não são alterados.

Em 2026-10-20, builds completas repetidas com Clang/LLD 22.1.8, GCC 16.2.1 e
Fedora 44 produziram estes hashes após normalização:

| Artefato | SHA-256 |
|---|---|
| hello.exe | `d958267727f33ed1ee46e9942595a07c38d0413a7edf74f341246b03bee8bbf6` |
| hello.xbe | `baaa88110014743864d0f38606e18eb423b0bf0da5bb4083a3e55b89a0a2af7f` |

A igualdade foi verificada neste ambiente; outra versão de compilador pode emitir
outro código. Os hashes identificam a build de referência, não qualquer build nxdk.

## O que o alvo exige

O loader verificou as 4 seções, 27 páginas e 104141 bytes; entry point `0x18C00`.
Há 72 entradas: 69 funções e 3 dados, sem ordinal desconhecido no catálogo.
Os dados são `LaunchDataPage` (164), `PsThreadObjectType` (259) e
`XeImageFileName` (326). Esses dados agora têm representação guest (ADR-0022),
com ponteiros de procedimento não suportados envenenados; isso não implementa
objetos, threads ou todos os serviços que os usam. Resolver um ordinal de função
também não significa implementar seu serviço. O caminho NT declarado ainda precisa
ser ligado à mídia/VFS real.

TLS do alvo: template `0x2A000..0x2A110` (272 bytes), zero-fill 0, índice
em `0x28108`, callbacks ausentes, characteristics `0x00300000` (alinhamento 4).
`ordinal-report` inspeciona esses campos e todas as importações sem execução.
O dispatcher atende `RtlInitAnsiString` (289), `NtCreateMutant` (192) sem nome,
espera/liberação de mutant (233/221), SHA-1 Init/Update/Final (335/336/337),
consulta integral de EEPROM (24), seções críticas (291/277/294/306) e
leitura do PLL PCI (46):
**13/69 serviços implementados**, com variantes limitadas,
e 3/3 dados representados. O relatório consulta a lista real do dispatcher.

O CRT nxdk configura TLS e cria uma thread para `main`. Serão necessários criação
e término de threads, objetos/sincronização e outros serviços além do transporte
de chamadas. A build contém `LOCK CMPXCHG` em `0x209C3` e `SETE` em `0x209C9`,
agora implementados no ADR-0024. RDTSC em `0x2055B` foi implementado no ADR-0025:
lê o Clock guest com política sintética determinística, sem tempo host. O bloqueio
seguinte foi CLI/HLT no caminho de assert do SDK, agora implementados no
ADR-0026. INT3 em `0x20D6C`, no preenchimento após a chamada a exit em abort,
agora tem fronteira #BP (ADR-0027). SFENCE em `0x12A43` chama a barreira do bus
síncrono exclusivo (ADR-0028). CMOVNE em `0x13B88` está coberto pelas 16 condições
CMOVcc de 16/32 bits (ADR-0029). BSF/BSR destravaram o fluxo descoberto (ADR-0030).
Separar funções de bloco corrigiu a pilha host do primeiro executável real.
Com `--entry 0x19110`, o hello traduz 8769 instruções/blocos e executa até
**NtCreateMutant (192)**, retorno reportado `0x18CFA`, argumentos `0x28EEC, 0, 0`
(ADR-0031). O ADR-0032 implementou essa criação com objeto/handle guest e
snapshot integral. Nessa entrega a execução passou por ela e parou em
**XcSHAInit (335)**, retorno `0x19140`, argumento `0x28EF0`, em `init_rand_s`.
O ADR-0033 implementou a família SHA-1 com contexto opaco
HLE de 116 bytes na RAM. SHAInit foi atendido no hello; Update/Final foram
validados em testes. O bloqueio observado nessa etapa foi **ExQueryNonVolatileSetting
(24)**, retorno `0x19176`, consulta de EEPROM de 256 bytes com índice `0xFFFF`,
na referência do ADR-0033. O ADR-0034 implementou essa consulta usando a imagem
da sessão; nessa entrega a execução parou em **NtWaitForSingleObject (233)**,
retorno `0x16B99`, argumentos `4, 0, 0`. Espera/liberação de mutant foram
implementadas no ADR-0035, e o construtor global `0x145E0` exigiu raiz adicional
e CLC/STC/CMC (ADR-0036). Init de seção crítica foi atendido no ADR-0037.
Nessa etapa o CALL em `0x18E23` chegou ao construtor `0x205E0`; incluí-lo
revelou RDMSR em `0x207C6`, `ECX=0x2A`, no cálculo de frequência de CPU em
`lib/winapi/profiling.c`. O ADR-0038 implementou RDMSR/perfil de CPU e
incluiu o case observado do multiplicador (`0x20889`). Com quatro raízes,
nessa entrega o hello chegou a HalReadWritePCISpace (46), retorno `0x2064D`.
O ADR-0039 atende essa consulta com configuração PCI de clock explícita;
o hello calcula 733333333 Hz e avança até InterlockedExchange (54), retorno
`0x19DC7`, argumentos `0x29158, 1`. O próximo passo é a troca atômica guest.
A análise estática não oferece a lista completa da ISA necessária
para futuros callbacks/threads.

## Infraestrutura implementada

`Image::prepare_aot` constrói uma imagem completa ou retorna erro. Reserva pilha
de 64 KiB em `0x70000000`, um NT_TIB mínimo em `0x7FFD0000` e, quando há imports,
uma página de trampolins RX em `0x7FFE0000`. Essas faixas não podem sobrepor a
imagem. O perfil não implementa KPCR/KTHREAD completos, scheduler ou exceções SEH.

No perfil TLS nxdk, a área fica no topo da pilha: `align16(raw + zero_fill) + 4`
bytes, com slot apontando ao payload alinhado. O índice contém o deslocamento
negativo em palavras a partir de `FS:[4]`. Template e zero-fill são inicializados;
ESP e retorno sentinela ficam abaixo da área. O tamanho é limitado a meia pilha.
Callbacks e alinhamento acima de 16 bytes são recusados. Apenas o perfil inicial
de uma thread está implementado; o CRT pode escrever novamente o mesmo índice.

FS é uma base guest explícita em `State`, aplicada aos operandos de memória;
LEA usa só o offset. Inferência estática não confunde leitura por FS com um
ponteiro absoluto do arquivo. Strings com segmento e os demais segmentos
explícitos continuam recusados. Snapshot `AOTC` v7 salva FS, perfil de CPU, perfil de tempo/Clock, estado
de HLT e breakpoint; v1–v6 migram para o perfil de CPU inicial; v1–v5 migram sem breakpoint, v1–v4 com CPU ativa, e v1–v3
recebem Clock zero, pois não
guardavam tempo. RAM/MMU existentes
guardam TLS e trampolins.

Cada import de função recebe o endereço guest `0x7FFE0000 + ordinal * 8`.
Somente slots importados têm marcador válido. O código gerado contém os destinos
explicitamente e devolve `Exit::KernelCall`, sem executar os bytes do marcador,
decodificar x86 em execução ou guardar ponteiros host. Patches em dados RO
restauram as proteções e nunca alteram páginas executáveis.

`Session::kernel_call` lê o frame e os argumentos. `complete_kernel_call` é a
fronteira para um dispatcher que realmente implemente o serviço: atualiza EAX,
ESP e EIP, preservando os demais registradores e flags. Stdcall remove os
argumentos; cdecl deixa a limpeza ao chamador; fastcall usa ECX/EDX e remove apenas
a parte na pilha. Os testes fornecem respostas controladas para validar ABI;
essas respostas não são serviços de produção. A CLI gerada para por nome/ordinal
ao encontrar uma chamada sem implementação. Uma chamada pendente é restaurada
por EIP, registradores e memória, sem continuação oculta no host.

CLI limpa IF guest; HLT preserva o PC da parada, avança EIP e entrega
`Exit::Halted` ao host. O main gerado relata a suspensão, sem bloquear o host
nem anunciar retorno do título. Ainda não há entrega de interrupções ou retomada
automática. Polls não avançam o Clock ([ADR-0026](../adr/0026-controle-cli-hlt.md)).

INT3 (`0xCC`) entrega `Exit::Breakpoint` ao host com PC e EIP seguinte; a CPU
preserva a exceção pendente em polls/snapshots. Sem entrega via IDT/SEH, o main
encerra com diagnóstico #BP. A análise não segue a exceção como fallthrough;
continuações precisam de um branch/raiz explícita. Não há retomada automática
([ADR-0027](../adr/0027-int3-breakpoint-aot.md)).

SFENCE chama `Bus::ordenar_escritas`: RAM é aplicada imediatamente e as visões
virtual/física/DMA compartilham a arena exclusiva. Não há filas pendentes a
drenar. Essa ordenação não implementa GPU ou MMIO; filas, write-combining ou DMA
concorrente futuros exigem ampliar o protocolo e rever o acesso à arena
([ADR-0028](../adr/0028-sfence-ordenacao-do-bus.md)). Essa entrega manteve AOTC v6; a versão atual é v7 (ADR-0038).

CMOVcc lê a origem antes de testar a condição, inclusive se falsa, preservando
faltas e efeitos de leitura do bus. Suporta word/dword, registrador/memória,
FS/SIB e destinos SP/ESP, sem alterar flags. Essa entrega manteve AOTC v6; 32768 casos
comparados com CPU física no oráculo opcional
([ADR-0029](../adr/0029-cmovcc.md)).

BSF/BSR definem ZF; o perfil preserva destino em origem zero e zera as demais
flags aritméticas indefinidas. O código gerado usa funções por bloco, com até
32 instruções, e preserva a pilha guest em RAM. AOTC atual é v7. Reprodução em
diretório novo/vazio:

```powershell
cargo run --offline -p xberecomp -- translate target/homebrew/hello.xbe --entry 0x19110 --entry 0x145E0 --entry 0x205E0 --entry 0x20889 --out target/alpha-hello
cargo run --offline --manifest-path target/alpha-hello/Cargo.toml
```

Sem a primeira raiz, a execução para no CALL de `0x209D2` para `0x19110`.
Com as três primeiras, 9024 blocos executam até JMP `0x2081E` → `0x20889`,
o case do multiplicador 5,5 ainda não inferido pela análise. Com as quatro,
são **9027 blocos** e o bloqueio real é **InterlockedExchange (54)**, retorno
`0x19DC7`, argumentos `0x29158, 1`, depois de calcular a frequência por PCI.
Isso ainda é execução inicial, sem framebuffer.
Contrato/evidência atual: [ADR-0039](../adr/0039-configuracao-pci-clock-guest.md).

SHA-1 usa formato opaco próprio do perfil HLE, sem compatibilidade declarada
com campos privados de um kernel físico. Seu estado está inteiramente na RAM:
cópia e snapshot não dependem de registro host. Final preserva o contexto por
política local. Limites: [ADR-0033](../adr/0033-sha1-contexto-guest.md).

O padrão AOT usa EEPROM sintética NTSC-M/português do gerador existente. API
permite bytes explícitos; origem e conteúdo ficam em KRNL v3/EEPR v1 no snapshot atual.
Não há leitura automática de arquivo/BIOS, ligação com SystemConfig ou persistência.
Outros índices e escrita de EEPROM ainda faltam. Limites e políticas locais:
[ADR-0034](../adr/0034-consulta-eeprom-sessao.md).

`Session` possui `kernel: Dispatcher`; a API atual é `session.atender_kernel()`.
Para preservar handles/objetos HLE, salvar/restaurar a `Session` inteira por
`Snapshot` (`AOTS` v1), e não só `state`/`bus`. O snapshot CPU agora é `AOTC`
v7, com perfil de CPU/MSRs guest. Regenerar projetos antigos que passavam um dispatcher externo.

Seções críticas mantêm evento, contagens e proprietário na RAM guest, sem registro
host por endereço. `OwningThread=1` é uma identidade sintética do perfil, não
KTHREAD. Init/Enter/Leave são VOID; TryEnter retorna BOOLEAN. Contenção bloqueante,
esperadores e overflow recusam sem efeitos; Init valida 28 bytes antes do store.
O formato atual é AOTS v1/AOTC v7/KRNL v3/KMUT v2/EEPR v1/PCIC v1. Detalhes das seções críticas:
[ADR-0037](../adr/0037-secoes-criticas-guest.md).

RDMSR opera sobre ECX completo e escreve EDX:EAX, preservando flags/outros
registradores. O perfil funcional nominal `XboxRetailV1` lê 0x2A como 0x01040000
(bus 133⅓ MHz × razão 5,5; demais campos sintéticos) e 0x10 como o mesmo Clock
de RDTSC. Não há RDMSR host. MSR sem modelo dá erro de suporte, sem #GP/IDT
implícito, efeitos ou cobrança do passo; WRMSR continua recusado. AOTC v7 salva
ID de perfil 1; v1–v6 migram para ele. Evidência nessa entrega: 322 testes workspace/doctests e
131 casos nativos debug/release, Rust 1.98.1 (ADR-0038).

HalReadWritePCISpace lê o registro PLL 0:0:3, 0x6C..0x70, incluindo subleituras;
escritas e áreas desconhecidas recusam. `RetailClockV1` do core centraliza CPU/PLL,
com readback público 0x01230801 (divisor 1/multiplicador 8). KRNL v3/PCIC v1
salva o perfil; v1/v2 migram preservando estado anterior. O hello real teve
`tsc_freq=733333333 Hz` conferido em RAM. Validação atual: 331 testes workspace/
doctests e 135 casos nativos debug/release, mais a verificação local do XBE.
Detalhes: [ADR-0039](../adr/0039-configuracao-pci-clock-guest.md).

## Referências públicas

- [Export declarations nxdk (CC0-1.0)](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/xboxkrnl/xboxkrnl.exe.def):
  nomes, ordinais, decorações de ABI e distinção função/dado do catálogo de 371
  exports. Metadados gerados por `scripts/generate-kernel-catalog.py`, seguido de
  `cargo fmt --all`; não são implementações de funções do SDK.
- [CRT nxdk-pdclib](https://github.com/XboxDev/nxdk-pdclib/blob/9d257524e20e3e1ac90163b37f07702053e36321/platform/xbox/crt0.c)
  e [_tls_array](https://github.com/XboxDev/nxdk-pdclib/blob/9d257524e20e3e1ac90163b37f07702053e36321/platform/xbox/_tls_array.s):
  cálculo do índice e uso de FS:[4].
- [Inicialização da thread nxdk](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/winapi/thread.c):
  alinhamento, slot autorreferente, cópia e zero-fill.
- [Estrutura de seção crítica Xbox (CC0)](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/xboxkrnl/xboxdef.h)
  e [assinaturas de kernel (CC0)](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/xboxkrnl/xboxkrnl.h):
  layout de 28 bytes e retorno VOID/BOOLEAN.
- [Construtor da frequência de CPU nxdk](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/winapi/profiling.c):
  leitura do MSR 0x2A e possível consulta PCI no próximo caminho a modelar.
- [cxbe](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/tools/cxbe/Xbe.cpp):
  timestamps e biblioteca placeholder usados na normalização de metadata.

A BIOS e as ISOs comerciais não foram usadas nesta etapa.

Validação inicial do ADR-0021: testes do workspace e doctests aprovados, mais 69 testes do Rust
gerado em debug e release (release com `-D warnings`). Clippy global com
`xbox-cpu/isa-oracle`, `cargo fmt --all --check` e `git diff --check` passaram.
O teste T14 opcional de mídia comercial permaneceu ignorado nesta etapa.
