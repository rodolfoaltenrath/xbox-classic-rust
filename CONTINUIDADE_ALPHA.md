# Continuidade do alpha — guia para outra LLM

Atualizado em **2026-10-20**, com base no commit **`e9446fb`** e nas alterações
locais do **ADR-0033**, ainda sem novo commit nesta referência. Este documento
registra o contexto necessário para continuar o projeto sem o histórico da conversa.
Antes de trabalhar, confira `git status`, os commits posteriores e as instruções
locais: o código pode ter evoluído depois desta referência.

## Objetivo do software e do primeiro alpha

O projeto **Obsidian / xbox-classic-rust** pretende executar software do Xbox
clássico por **recompilação estática para Rust**, com uma camada de compatibilidade
Xbox implementada em Rust. Repositório:
<https://github.com/rodolfoaltenrath/xbox-classic-rust>.

O caminho implementado é:

```text
XBE → análise x86 antes da execução → IR → código Rust → Cargo/rustc → programa nativo
                                                          ↓
                                             memória guest e kernel HLE
```

O código Rust gerado representa as operações do executável original; não recupera
seu código-fonte nem converte automaticamente todo o jogo em Rust idiomático.
Executar o código recompilado nativamente ainda exige representar memória,
serviços do Xbox e dispositivos. A meta de Rust se aplica ao recompilador/runtime
do projeto; o SDK externo nxdk usa C/C++ para produzir a entrada de teste.

**Meta do alpha:** executar o `samples/hello` real do nxdk, com saída verificável,
controle de execução pelo host e apresentação do framebuffer produzido pelo
homebrew. Não há compromisso de executar jogos comerciais neste primeiro alpha.
Controle, áudio e uma interface de mini console ficam para etapas posteriores.

**Estado atual:** a demonstração própria já executa Rust recompilado. O fluxo
descoberto do hello traduz e inicia nativamente; passou por NtCreateMutant (192)
e por XcSHAInit (335). Agora para em ExQueryNonVolatileSetting (24), ainda não
implementado, na consulta de EEPROM de `init_rand_s`.
A execução integrada completa e o framebuffer ainda estão pendentes.
Jogos comerciais ainda não executam.

## As seis entregas e o estado das duas já trabalhadas

| Passo | Entrega | Situação nesta referência | Critério para concluir |
|---|---|---|---|
| 1 | Memória e loader | **Concluído no aceite local** | Imagem real conferida pelo bus; contratos, permissões e segurança de memória validados |
| 2 | TLS e importações | **Infraestrutura implementada e testada; aceite integrado aberto** | TLS correto e chamadas por ordinal funcionando com o ambiente real requerido pelo homebrew |
| 3 | Kernel mínimo | **Dados, dispatcher, mutexes e SHA-1; 5/69 serviços com variantes limitadas** | Implementar dados e serviços necessários ao caminho executado pelo alvo, com erro explícito nos demais |
| 4 | ISA necessária | BSF/BSR implementados; fluxo descoberto do hello traduz (8725 instruções); entradas de threads/callbacks ainda precisam de aceite | Traduzir os caminhos necessários do XBE real, validar semântica e descobrir seus destinos |
| 5 | Execução integrada | Execução inicial real até ExQueryNonVolatileSetting; aceite aberto | Rodar o homebrew com dispatcher, orçamento, logs e interrupção controlada |
| 6 | Framebuffer | Pendente | Apresentar uma imagem produzida pela execução do homebrew |

Há **duas entregas de implementação registradas em commits**, mas isso não
significa dois aceites completos do alpha. O passo 2 depende de partes do passo 3
para fechar sua integração. As seis entregas não são uma estimativa de seis
commits, seis dias ou uma garantia de compatibilidade.

## Passo 1 — memória e loader

Commit: **`42e5c60` — `feat(loader): share XBE loading and validate the memory subsystem`**.

### O que foi feito

- Loader compartilhado `xbox_kernel::loader::Image`: headers, seções, BSS,
  alocação de frames, validação de faixas e permissões por página.
- `Mode::Inspect` permite inspecionar imagens com páginas RWX e oferece alias
  físico em `0x80000000 + PA`, sem executar código.
- `Mode::Aot` recusa páginas simultaneamente graváveis e executáveis, inclusive
  páginas compartilhadas entre código e dados; não oferece o alias físico RW.
- `Image::verify` compara a imagem com o arquivo e a BSS, confere entry point,
  ordinais pelo bus e uma página não mapeada.
- CLI `obsidian check-memory` aceita XBE, pasta com `default.xbe` ou ISO; a leitura
  da ISO ocorre sob demanda e a inspeção não executa o título.
- Escritas do bus validam a faixa inteira antes de alterar RAM/MMIO, evitando
  stores parcialmente aplicados quando uma página posterior falha.
- Contratos de memória, benchmark seguro, auditoria de unsafe e job Miri no CI.
  A arena mantém posse exclusiva: `Send`, sem `Sync` e sem `Clone`.

### Evidência registrada

O teste T14 local com NFS Underground 2 verificou **14 seções, 1132 páginas,
4634697 bytes e 152 ordinais**. Nenhuma instrução comercial foi executada.
Seis contratos passaram sob Miri; o teste nativo de cenários comparou 20000 casos.
Os testes de workspace e do Rust gerado passaram, assim como Clippy e formatação.
Os resultados detalhados são históricos dessa entrega; não são novas execuções
feitas ao escrever este guia.

Arquivos para retomar:

- [Loader](crates/xbox-kernel/src/loader.rs) e [verificação](crates/xbox-kernel/src/loader/verify.rs).
- [Testes do loader](crates/xbox-kernel/tests/loader.rs).
- [Guia de memória e comandos](docs/dev/memoria.md).
- [ADR-0020](docs/adr/0020-loader-memoria-validada.md) e [auditoria de unsafe](docs/arch/memoria-unsafe.md).

**Limite:** conferir os bytes de uma imagem comercial não demonstra que o jogo
execute. A aceitação de RWX na inspeção não autoriza habilitar RWX no AOT sem
resolver a correspondência entre código traduzido e bytes modificáveis.

## Passo 2 — TLS, imports e build real do nxdk

Commit: **`478c76d` — `feat(kernel): add nxdk TLS initialization and AOT import trampolines`**.

### O que foi feito

- Build do `samples/hello` com nxdk fixado na revisão
  `14d5ee97e73347c973f1f57b68b79ec08c9e77f2` e submódulos fixados.
  `scripts/build-homebrew.sh` registra ferramentas, hashes e logs em `target`.
- Build repetida no WSL/Fedora com Clang/LLD 22.1.8. O script preserva o XBE
  original e normaliza timestamps e dois bytes de padding de metadata do cxbe.
  Não altera código, TLS ou imports para contornar incompatibilidades.
- `ordinal-report` deixou de ser esqueleto: informa TLS, ordinais, nomes, ABI,
  distinção entre funções/dados e o primeiro bloqueio da análise AOT.
- Catálogo de 371 exports gerado das declarações públicas CC0 do nxdk. Ter o
  nome e a assinatura no catálogo não significa implementar o serviço.
- `Image::prepare_aot` prepara o perfil inicial de uma thread, com pilha,
  NT_TIB mínimo, TLS nxdk e resolução de imports de funções.
- TLS com template, zero-fill, payload alinhado e índice negativo relativo a
  `FS:[4]`. A área ocupa o topo da pilha; ESP e retorno ficam abaixo dela.
- `State::fs_base` guarda um endereço guest. Operandos de memória usam FS;
  LEA calcula só o offset. Leituras por FS não são inferidas como ponteiros
  absolutos do arquivo.
- Trampolins RX com endereços guest e marcadores por ordinal. Somente destinos
  importados são incluídos no despacho gerado; não há ponteiros host no guest.
- Saída `Exit::KernelCall`, leitura de argumentos por `Session::kernel_call`
  e retorno por `Session::complete_kernel_call`, com convenções
  stdcall/cdecl/fastcall. As respostas controladas dos testes validam a ABI;
  não são implementações de serviços do Xbox.
- Snapshot `AOTC` v3 preserva FS e migra v1/v2 com base zero. Uma chamada pendente
  é reconstruída por EIP, registradores e RAM, sem continuação oculta no host.

### Contratos que a próxima implementação deve preservar

| Elemento | Contrato atual |
|---|---|
| Pilha inicial | 64 KiB a partir de `0x70000000` |
| NT_TIB mínimo | `0x7FFD0000`; não equivale a KPCR/KTHREAD completos |
| Trampolins | Página RX em `0x7FFE0000`; slot em `base + ordinal * 8` |
| TLS nxdk | `align16(raw + zero_fill) + 4`; slot autorreferente e índice negativo |
| Proteções | Patches em dados RO restauram permissões; páginas executáveis não são alteradas |
| ABI | Stdcall limpa argumentos; cdecl deixa a limpeza ao chamador; fastcall usa ECX/EDX e limpa só a parte na pilha |
| Estado de execução | CPU, RAM, continuações e objetos HLE; usar snapshot integral de Session (AOTS v1) |
| Falta de suporte | Erro identificável; não retornar sucesso genérico para avançar |

O perfil limita TLS a meia pilha e recusa callbacks e alinhamentos acima de
16 bytes. Strings com segmento continuam recusadas. Na entrega do ADR-0021,
ainda não havia serviços HLE ou dados exportados; os avanços posteriores estão
no passo 3 abaixo. O scheduler continua pendente.

### O que foi descoberto no XBE real

| Item | Resultado da build de referência |
|---|---|
| Imagem | 4 seções, 27 páginas, 104141 bytes verificados pelo loader |
| Entry point | `0x00018C00` |
| Imports | 72 entradas: **69 funções e 3 dados**, sem ordinal desconhecido |
| Template TLS | `0x2A000..0x2A110`, 272 bytes; zero-fill 0 |
| Índice TLS | Endereço `0x28108` |
| Callbacks/alinhamento | Sem callbacks; characteristics `0x00300000`, alinhamento 4 |
| Análise AOT atual | 8725 instruções/blocos; 8769 com `--entry 0x19110`; sem recusa de ISA nesse fluxo |
| Bloqueio de execução atual | ExQueryNonVolatileSetting (24), retorno reportado `0x19176`; índice `0xFFFF`, buffer de 256 bytes; mutex e SHAInit já atendidos |
| ISA antes bloqueante, agora implementada | `LOCK CMPXCHG` em `0x209C3`; `SETE` em `0x209C9` (ADR-0024) |

Os três dados importados são `LaunchDataPage` (164), `PsThreadObjectType` (259)
e `XeImageFileName` (326). O CRT nxdk cria uma thread para executar `main`.
Logo, TLS e transporte de chamadas, isoladamente, não bastam para rodar o alvo.
As 69 funções são **importações estáticas**; só a execução prova quais são chamadas
em um caminho. O fluxo já exerceu NtCreateMutant e XcSHAInit, chegando à EEPROM; isso não
fornece um inventário completo das chamadas futuras.

### Evidência e arquivos

Passaram testes do workspace e doctests, **69 testes do Rust gerado em debug e
release**, Clippy global e formatação. Os testes cobrem ABI, TLS/FS, LEA,
snapshot com chamada pendente, tail call, orçamento e erros. A execução dos
testes gerados usa 69 casos em x86_64; o conjunto varia conforme o host.
**Nessa entrega ainda não havia serviços HLE de produção. O hello completo não rodou.**

- [TLS e startup](crates/xbox-kernel/src/loader/startup.rs).
- [Imports e ABI](crates/xbox-kernel/src/imports.rs) e [catálogo](crates/xbox-kernel/src/imports/catalog.rs).
- [Runtime e snapshots](crates/xbox-recomp/src/runtime.rs).
- [Análise](crates/xbox-recomp/src/analysis.rs), [IR](crates/xbox-recomp/src/ir.rs) e [emissão Rust](crates/xbox-recomp/src/emit.rs).
- [Testes de startup](crates/xbox-kernel/tests/startup.rs), [análise de startup](crates/xbox-recomp/tests/startup.rs) e [testes nativos de ABI](tools/xberecomp/tests/verification_kernel.inc).
- [Build, hashes e fontes públicas](docs/dev/homebrew.md) e [ADR-0021](docs/adr/0021-tls-imports-aot.md).

## Passo 3 — kernel mínimo: próxima entrega

**Objetivo:** dar semântica real aos dados e serviços que a inicialização do alvo
precisa. A infraestrutura do passo 2 deve ser reutilizada.

### Item 1 concluído — dados exportados (ADR-0022)

Os três dados têm representação em memória guest, em duas páginas porque a permissão
da MMU é por página e o título escreve um deles:

| Export | Endereço | Permissão | Forma |
|---|---|---|---|
| `PsThreadObjectType` (259) | `0x7FFF0000` | somente leitura | `OBJECT_TYPE` de 28 bytes, procedimentos **envenenados** |
| `XeImageFileName` (326) | `0x7FFF0100` | somente leitura | `ANSI_STRING` + buffer com caminho NT |
| `LaunchDataPage` (164) | `0x7FFF1000` | leitura e escrita | ponteiro de 4 bytes, nulo |

O slot do thunk de um dado recebe o endereço do símbolo, não um trampolim — o nxdk usa
`__declspec(dllimport)` e o código carrega o slot antes de desreferenciar. Dado que o
catálogo conhece e ninguém modelou continua recusando a imagem, com nome e ordinal.

Dez testes em `crates/xbox-kernel/tests/data.rs` cobrem layout, ponteiros, permissões e
conteúdo, incluindo a recusa de escrita na página somente-leitura e o envenenamento
ficar em página não mapeada.

**Efeito medido:** `translate` do `hello.xbe` real deixou de parar no ordinal 164 e
agora para em `LOCK CMPXCHG` em `0x209C3` — bloqueio de **ISA, em tempo de tradução**.
Isso confirma que, sendo AOT, instrução faltante bloqueia antes de qualquer serviço.
Resolver os dados destravou a próxima investigação; não aproximou a execução por si.

O caminho em `XeImageFileName` é `\Device\CdRom0\default.xbe`: valor **declarado**, não
derivado de mídia. Ligar isso à VFS é pendência registrada.

### Item 2 concluído — dispatcher (ADR-0023)

`xbox_kernel::dispatch::Dispatcher` atende em `(&mut Bus, pc, regs)`, e quem aplica o
resultado na CPU é `Session::atender_kernel`. Fica em `xbox-kernel` porque receber
`Session` fecharia ciclo com `xbox-recomp`.

Contratos que a próxima implementação deve preservar:

| Elemento | Contrato |
|---|---|
| Recusa | erro com nome, ordinal, **endereço do chamador** e argumentos pela ABI |
| Cdecl | mostra as primeiras palavras e avisa que a quantidade não é declarada |
| Falha | CPU **intacta**; o estado pendente continua retomável por EIP/registradores/RAM |
| Fonte única | `dispatch::IMPLEMENTADOS`, consultada pelo registro e pelo relatório |
| Sucesso genérico | proibido: ordinal sem serviço devolve erro, nunca `EAX = 0` |

Primeiro serviço: `RtlInitAnsiString` (289), escolhido por ser função pura sobre a
memória guest — mede a origem e preenche os três campos do destino, sem estado de
kernel e sem nada inventado. Destino nulo é recusado; string sem NUL dentro do limite
de `ANSI_STRING` é erro, não truncamento.

O projeto gerado passou a ter laço de despacho (executa, atende, retoma). Duas
verificações no Rust **gerado** cobrem o caminho completo; o contador de testes gerados
foi de 69 para 71. O relatório mostra `1/69 serviços implementados`.

**`1/69` não mede proximidade de executar o homebrew.** Após o dispatcher, o alvo
parava em `LOCK CMPXCHG` em `0x209C3`. O ADR-0024 implementou essa operação e SETcc;
o alvo passou a parar em RDTSC (`0x2055B`). O ADR-0025 implementou RDTSC com
Clock guest. O ADR-0026 implementou CLI/HLT e o ADR-0027 modelou a fronteira #BP
de INT3. O ADR-0028 implementou SFENCE no bus síncrono exclusivo; o bloqueio
seguinte foi CMOVNE (`0x13B88`). O ADR-0029 implementou CMOVcc e encontrou BSR
(`0x1657C`). O ADR-0030 implementou BSF/BSR e permitiu traduzir o fluxo descoberto
(8725 instruções/blocos). Após corrigir a pilha host e incluir um callback
explícito, a execução real chega a NtCreateMutant (ADR-0031), sem concluir o alpha.

### Avanço — NtCreateMutant e estado de kernel (ADR-0032)

`NtCreateMutant` (192) cria mutexes sem nome com handles guest distintos,
proprietário inicial opcional e recursão inicial 0/1. Aceita atributos nulos ou
struct Xbox de 12 bytes zerada; nomes/flags/diretórios ainda recusam com erro de
variante não implementada. Ponteiros inválidos devolvem NTSTATUS sem objeto ou
store parcial. Limite local: 4096 objetos. Espera, liberação, fechamento, abandono
e scheduler continuam pendentes; a identidade 1 é apenas a thread inicial.

`Session` agora possui `kernel: Dispatcher`; chamar `session.atender_kernel()`
sem argumento externo. O retorno é validado/capturado antes dos efeitos do
serviço. Snapshot integral de `Session` usa `AOTS` v1 e inclui CPU `AOTC` v6,
bus, `KRNL` v1 e `KMUT` v1. **Salvar apenas CPU/bus não preserva objetos HLE.**
Regenerar projetos antigos para a nova API. Carga inválida exige descartar sessão.

O hello real foi traduzido, compilado e executado com `--entry 0x19110` e passou
por NtCreateMutant. O novo bloqueio é **XcSHAInit (335)**, retorno `0x19140`,
argumento `0x28EF0`, no caminho de `init_rand_s` do nxdk. O fonte público usa um
contexto de 116 bytes e depois chama EEPROM/SHAUpdate/contador; implementar o
contrato e estado corretos de SHA antes de avançar, sem zerar contexto como stub.

Registro nessa entrega: **2/69 serviços** (variantes limitadas), 3/3 dados. Foram validados
**271 testes de workspace/doctests** e **115 casos gerados em debug/release**
(99 em outros hosts), Clippy global, fmt, loader do hello e diff. Esse avanço
não fecha kernel mínimo, threads, execução integrada nem framebuffer.
Detalhes e logs: [ADR-0032](docs/adr/0032-mutants-guest-snapshot-sessao.md).

### Avanço — SHA-1 em contexto opaco guest (ADR-0033)

`XcSHAInit`/`XcSHAUpdate`/`XcSHAFinal` (335/336/337) implementam SHA-1 com estado
inteiramente nos 116 bytes da RAM guest. Copiar o contexto cria um fluxo independente;
snapshot parcial funciona sem mapa host. O formato interno é **HLE v1**, com marcador
`XSH1`, não um layout comprovado do kernel físico. Atende código que trata o buffer
como opaco. Final preserva a origem por política local, salvo sobreposição da saída;
não prometer preservação byte a byte pelo hardware. Layout e limites estão no ADR.

Ponteiros/faltas/RO/MMIO/overflow são recusados antes de publicar mudanças.
VOID usa EAX zero por convenção local, não NTSTATUS inventado. Não houve novo
unsafe, dependência ou estado extra a serializar; versões de snapshot permanecem.
Há vetores públicos de SHA-1, referências .NET para padding/páginas, testes de
fragmentação, cópia, sobreposição e três casos no Rust gerado.

O hello real passou por SHAInit e parou em **ExQueryNonVolatileSetting (24)**,
retorno `0x19176`, argumentos `0xFFFF, 0x7000FD78, 0x7000FD80, 0x100, 0x7000FD7C`.
O próximo trabalho é modelar o contrato dessa consulta e o conteúdo/estado da
EEPROM, com origem e política explícitas, sem retornar sucesso com um buffer fictício
ou buscar dados na BIOS. SHAUpdate/Final estão validados em testes; ainda não foram
exercidos pelo hello, que parou antes deles.

Registro atual: **5/69 serviços com variantes limitadas**, 3/3 dados; **288 testes
de workspace/doctests** e **118 casos gerados em debug/release** (102 em outros
hosts), Clippy global e fmt. Logs e detalhes:
[ADR-0033](docs/adr/0033-sha1-contexto-guest.md). Kernel mínimo, threads, orçamento/
interrupção integrada e framebuffer continuam com aceite aberto.

Trabalho restante:

1. ~~Modelar os três dados exportados~~ — concluído acima.
2. ~~Implementar um dispatcher para `Exit::KernelCall`~~ — concluído acima.
3. Levantar a ordem das chamadas de inicialização a partir do CRT e do código
   compilado; implementar cada serviço necessário com teste de comportamento.
4. Criar um modelo explícito de threads guest suficiente para a thread de `main`,
   com TLS próprio, espera/retomada e término conforme o caminho usado pelo nxdk.
   Não confundir o perfil inicial existente com esse escalonamento.
5. Acrescentar memória/objetos, sincronização, tempo, arquivos e outros serviços
   conforme os caminhos realmente necessários. Manter os demais como erros claros.

**Aceite:** dados resolvidos com representação adequada, serviços implementados
com resultados/efeitos verificáveis e chamadas reais atravessando o dispatcher.
Uma tabela de zeros ou funções que sempre retornam sucesso não fecha o passo.
A integração deve também fechar as pendências do passo 2.

## Passo 4 — ISA exigida pelo compilador

**Objetivo:** traduzir os caminhos necessários do XBE real para Rust executável.

Já existem muitas operações inteiras, subregistradores, flags, pilha, branches,
CALL/JMP indiretos, multiplicação/divisão e strings REP retomáveis. A análise
descobre fluxo direto, constantes locais e ponteiros imutáveis; `--entry` permite
adicionar raízes explícitas para callbacks e outros destinos não inferidos.

O [ADR-0024](docs/adr/0024-cmpxchg-setcc.md) adicionou CMPXCHG 8/16/32,
LOCK em destino RAM e as 16 condições SETcc. A indivisibilidade usa posse exclusiva
do bus, sem yield dentro da instrução; MMIO é recusado. As flags e resultados de
CMPXCHG foram comparados com a CPU física em 147456 casos. O projeto gerado tem
agora 79 testes em x86_64, incluindo FS/SIB, aliases, RO, faltas e snapshots.

O [ADR-0025](docs/adr/0025-relogio-aot-rdtsc.md) implementou RDTSC e integrou o
`Clock` já existente no core. A política AOT v1 cobra um ciclo virtual por
instrução ou elemento REP concluído: é sintética, não latência física do Xbox.
Falhas não cobram o passo que falhou; fatias, orçamento zero e polls HLE não
inventam tempo. Snapshot AOTC v4 guarda o perfil e CLK0; v1–v3 recebem Clock zero,
sem derivar ciclos de `retired`. Perfil desconhecido e esgotamento são erros.
O Scheduler do core ainda não está integrado; futuras threads e serviços de tempo
devem compartilhar esse único relógio da sessão, não criar relógios por thread.
Essa entrega levou o projeto gerado a 84 testes em x86_64.

Falta implementar e validar as demais instruções dos caminhos necessários.
O [ADR-0026](docs/adr/0026-controle-cli-hlt.md) implementou CLI/HLT, encontrados
no caminho `_xbox_assert`. CLI limpa IF guest no perfil privilegiado; HLT guarda
o PC da parada, avança EIP e devolve `Exit::Halted` com IF. O main gerado encerra
com diagnóstico, sem bloquear o host nem anunciar retorno. Polls, inclusive com
orçamento zero, preservam estado e tempo; não há entrega de IRQ/NMI nem retomada
automática. AOTC v5 guarda a parada e migra v1–v4 com CPU ativa, preservando as
regras anteriores de tempo. Essa entrega chegou a 88 testes gerados em x86_64.

O [ADR-0027](docs/adr/0027-int3-breakpoint-aot.md) modelou INT3 (`0xCC`) como
`Exit::Breakpoint { pc }`, vetor 3, EIP seguinte e exceção persistente em polls e
snapshots. Não há IDT/SEH nem retomada automática; o main encerra com diagnóstico.
O preenchimento encontrado após CALL em `0x20D67` corresponde ao fim de abort,
confirmado pelo objeto público e suas relocações para raise/exit. Isso não prova
que esse caminho executou. A descoberta termina na exceção; branches e raízes
explícitas continuam descobrindo seus destinos, inclusive depois de INT3.
Retomada futura exige tratamento explícito e continuação traduzida.

AOTC v6 guarda o breakpoint; v1–v5 migram sem essa pendência, preservando as
regras anteriores de FS, Clock e HLT. Essa entrega chegou a 92 casos nativos
em x86_64 e encontrou SFENCE em `0x12A43`.

O [ADR-0028](docs/adr/0028-sfence-ordenacao-do-bus.md) acrescentou IR própria de
SFENCE e chamada a `Bus::ordenar_escritas`. O corpo da barreira é vazio no perfil
atual: RAM é aplicada sincronicamente, a arena é exclusiva e não há store buffer
a publicar. Isso não implementa MMIO ou conclusão de GPU. Filas de stores,
write-combining ou observadores concorrentes futuros exigem ampliar a barreira
e rever os contratos de memória; uma fence host isolada não substitui esse trabalho.
Não há estado novo: AOTC continua v6 e o perfil de tempo continua 1.
Essa entrega chegou a 96 casos gerados em x86_64 e um contrato de visibilidade
por alias/físico/DMA validado também sob Miri; o bloqueio seguinte foi CMOVNE.

O [ADR-0029](docs/adr/0029-cmovcc.md) implementou as 16 condições de CMOVcc,
destino registrador e origem registrador/memória de 16/32 bits. A origem é lida
incondicionalmente antes da condição: condição falsa não evita falta nem efeito
de leitura do bus. Flags são preservadas; word conserva a metade alta. Aliases,
FS/SIB, SP/ESP, RO, leituras cruzando página, snapshots e tempo têm testes no Rust
gerado. O novo unsafe fica apenas no oráculo opcional de xbox-cpu, com assembly
fixo em registradores locais e flags mascaradas; não recebe endereços guest.
São 32768 comparações físicas e 103 testes gerados em x86_64. Não há estado novo:
AOTC permanece v6 e o perfil de tempo permanece 1.

O [ADR-0030](docs/adr/0030-bsf-bsr.md) implementou BSF/BSR de 16/32 bits,
com origem registrador/memória e ZF definido. Para zero, o runtime preserva o
destino; normaliza CF/PF/AF/SF/OF para zero, como política para os indefinidos da
referência clássica. Word usa só os 16 bits baixos e preserva a metade alta do
destino. AOTC permanece v6, perfil de tempo 1. O novo unsafe está só no oráculo
físico opcional; AOT/runtime/bus continuam sem novo unsafe.

São 262144 comparações físicas dos campos definidos (em zero, somente ZF),
mais testes portáveis de flags/zero, RO/faltas, aliases, FS/SIB, SP/ESP, branches,
snapshots e tempo. O projeto gerado tem 110 casos em x86_64 (94 em outros hosts).
A análise real do fluxo descoberto do hello **passa com 8725 instruções e blocos**.
Isso não garante novos destinos de threads/callbacks, todos os serviços nem
execução completa: implementar/descobrir novos caminhos pode revelar mais ISA.

O [ADR-0031](docs/adr/0031-blocos-aot-pilha-host.md) corrigiu um estouro de pilha
host no primeiro executável real: cada bloco agora é função não inlineada, com
até 32 instruções. O dispatcher seleciona o ponteiro de função e faz uma chamada
por passagem; CALL/RET guest não geram recursão nativa. A inferência de destinos
ocorre antes de limitar os blocos. Orçamento de sequências longas fica mais fino,
sem alterar Clock ou AOTC v6. Há regressão em pilha host solicitada de 128 KiB.
O total dessa entrega foi 111 testes gerados em x86_64 (95 em outros hosts).

A execução revelou `UnknownTarget` no CALL `0x209D2 → 0x19110`. Essa raiz de
callback é adicionada explicitamente com `--entry 0x19110`; sem ela o erro deve
continuar explícito. Com ela, 8769 instruções/blocos compilam e executam até
**NtCreateMutant (192)**, retorno reportado `0x18CFA` (CALL em `0x18CF4`),
argumentos `0x28EEC, 0, 0`. O nxdk chama isso em `threads/mtx_init.c`.
O próximo trabalho é modelar handles/objetos e mutant com estado real, incluindo
a escrita do handle guest e planejamento de ownership/espera. Não retornar
sucesso fictício; o dispatcher deve continuar recusando serviços ausentes.
Preservar semântica de flags, acessos, atomicidade aplicável e falhas sem efeitos
parciais; comparar com referências independentes quando apropriado.

**Aceite:** tradução dos caminhos necessários do alvo, incluindo entradas das
threads/callbacks, sem decodificação x86 durante a execução. Erros de fluxo e
instruções ainda faltantes devem continuar explícitos. Os passos 3 e 4 podem
precisar avançar intercalados; corrigir imports não prova que a ISA esteja pronta.

## Passo 5 — execução integrada

**Objetivo:** conectar imagem, Rust gerado, bus, dispatcher e threads em uma
execução observável e limitada pelo host.

Reutilizar `Session`, `State`, `Exit::{Returned, Budget, KernelCall, Halted, Breakpoint}` e os
snapshots. Implementar o ciclo de despacho HLE e retomada, orçamento/interrupção,
logs úteis e relatório do primeiro erro não suportado. Manter separadas as
condições de retorno, orçamento esgotado, espera de thread e término do processo.

O `hello` configura vídeo, imprime texto e chama Sleep num laço: não se deve
exigir que ele retorne espontaneamente. A espera precisa permitir progresso do
host e das threads, e o host deve conseguir encerrar a sessão.

**Aceite:** executar código recompilado do homebrew, observar efeitos reais da
inicialização/`main`, repetir o resultado e interromper de forma controlada.
A demonstração própria com `EAX=62` permanece como regressão, não como prova de
que o homebrew real passou.

## Passo 6 — framebuffer

**Objetivo:** apresentar a imagem produzida pelo homebrew executado.

Identificar o caminho real de `XVideoSetMode` e `debugPrint` na revisão nxdk
fixada e modelar os serviços/acessos de dispositivo necessários. Conectar o
framebuffer guest ao backend de apresentação, respeitando formato, dimensões,
stride e posse da memória. Preservar a segurança de memória Rust ao compartilhar
dados com renderização; a arena atual não pode simplesmente virar `Sync`.

**Aceite:** mostrar uma imagem originada das escritas reais do homebrew, com
evidência visual e execução controlável. Um texto desenhado diretamente pelo host
ou uma janela com imagem estática própria é só um teste de apresentação.
Controle, áudio, GPU completa e compatibilidade comercial seguem fora deste aceite.

## Como começar em outra máquina ou sessão

### Ambiente e referências

- Rust fixado em **1.98.1** por `rust-toolchain.toml`; seguir também
  [docs/dev/build.md](docs/dev/build.md) para linker/dependências do host.
- Último host validado: Windows x86_64 GNU + PowerShell; SDK compilado pelo WSL
  `FedoraLinux-44`. O caminho local era `C:\Users\Altenrath\emulador-xbox`.
  Ajustar caminhos e nome da distribuição em outra máquina.
- SDK, XBEs e logs em `target` são locais e **não chegam por git clone**.
  `--offline` nos comandos Cargo pressupõe dependências já baixadas; quando
  necessário, preparar a cache com `cargo fetch --locked`.
- Ler este guia, [resumo do alpha](docs/dev/alpha.md),
  [homebrew](docs/dev/homebrew.md), [arquitetura AOT](docs/arch/recompilacao.md)
  e [ROADMAP.md](ROADMAP.md). ADRs registram decisões históricas: ADR-0021 amplia
  o suporte a TLS/imports que ainda era recusado na etapa do ADR-0020.

### Reconstruir e inspecionar o alvo

Com as dependências do SDK instaladas conforme `docs/dev/homebrew.md`:

```sh
# Linux, a partir da raiz do projeto:
bash scripts/build-homebrew.sh
```

No Windows usado como referência:

```powershell
wsl -d FedoraLinux-44 -- bash /mnt/c/Users/Altenrath/emulador-xbox/scripts/build-homebrew.sh
cargo run --offline -p ordinal-report -- target/homebrew/hello.xbe
cargo run --offline -p obsidian -- check-memory target/homebrew/hello.xbe
cargo run --offline -p xberecomp -- translate target/homebrew/hello.xbe --entry 0x19110 --out target/alpha-hello
cargo run --offline --manifest-path target/alpha-hello/Cargo.toml
```

Na referência atual, `check-memory` e `translate` passam. Com a entrada explícita,
o binário nativo passa por mutex/SHAInit e para em **ExQueryNonVolatileSetting (24)**,
sem framebuffer. Sem ela,
para em `UnknownTarget` para `0x19110`. Isso ainda não significa execução
completa nem serviços suficientes. A tradução exige
diretório de saída novo/vazio: se o nome já foi
usado, escolher outro sem apagar trabalho anterior.

### Verificar alterações de implementação

```sh
cargo fmt --all --check
cargo test --offline --workspace --features xbox-cpu/isa-oracle
cargo clippy --offline --workspace --all-targets --features xbox-cpu/isa-oracle -- -D warnings
git diff --check
```

O teste `cargo test --offline -p xberecomp --test native` gera um projeto em
`target/recomp-tests/<id>`, compila o Rust emitido e grava seus resultados.
Para repetir também em release, depois de gerar esse projeto, no PowerShell:

```powershell
$generated = Get-ChildItem target/recomp-tests -Directory | Sort-Object LastWriteTime -Descending | Select-Object -First 1
cargo test --offline --release --manifest-path (Join-Path $generated.FullName 'Cargo.toml') --target-dir target/recomp-native-check
```

Os 118 casos atuais em x86_64 não são um número fixo de aceite: adicionar os casos
necessários e atualizar o contador esperado do teste gerador ao ampliar a suíte.
Se alterar memória/unsafe, consultar também os contratos e Miri no guia de memória.
Mídia comercial é opcional; seu teste T14 fica ignorado na execução normal.

## Arquivos locais e cuidados de continuidade

O usuário mantém ISOs comerciais e `Complex_4627v1.03.bin` localmente, ignorados
pelo Git. **A BIOS não foi usada** nas implementações acima e não é necessária
para retomar este plano. Não presumir sua presença em outra máquina nem copiar
BIOS, jogos ou bytes proprietários para código, documentação ou fixtures.
O alvo inicial é o homebrew aberto; fontes públicas fixadas estão no guia nxdk.

Preservar alterações locais do mantenedor. Os commits anteriores foram feitos
pelo usuário após sugestões de mensagem; criar este guia não autoriza publicar,
fazer push ou modificar o remoto. Ao avançar cada entrega, atualizar este arquivo,
o resumo do alpha, o roadmap e a evidência de validação correspondente.

## Texto pronto para iniciar com outra LLM

> Continue o projeto Obsidian a partir de CONTINUIDADE_ALPHA.md. Confira o estado
> atual do Git e leia os arquivos indicados antes de alterar código. A entrega 1
> tem aceite local; a entrega 2 possui infraestrutura TLS/ABI testada, mas seu
> aceite com o homebrew depende do kernel. Os três dados, o dispatcher e
> RtlInitAnsiString, NtCreateMutant sem nome e SHA-1 Init/Update/Final já existem
> (5/69 serviços). SHA usa contexto opaco de 116 bytes na RAM, com formato HLE v1,
> não layout comprovado do hardware. Final preserva a origem por política local.
> Session possui o dispatcher; atender_kernel() não recebe argumento. Snapshot
> integral AOTS v1 inclui CPU, bus e objetos KRNL/KMUT; CPU/bus isolados não bastam.
> CMPXCHG/LOCK em RAM e SETcc passaram nos testes;
> RDTSC também passou, com Clock guest e custo sintético explícito. CLI/HLT têm
> controle/parada guest. INT3 gera #BP com PC/EIP seguinte, polls sem efeitos e
> snapshot AOTC v6; ainda não há IRQ/NMI, IDT/SEH ou retomada automática. A descoberta
> termina o caminho na exceção, preservando branches/raízes explícitas. O bloqueio
> atual é ExQueryNonVolatileSetting (24), retorno 0x19176, índice 0xFFFF,
> buffer 0x7000FD80, tamanho 256, em init_rand_s. SHAInit já foi atendido no hello;
> SHAUpdate/Final estão validados em testes, mas ainda não observados no alvo.
> O hello real traduz/compila/inicia com --entry 0x19110 (callback observado em
> 0x209D2). Funções de bloco limitadas corrigiram a pilha host; são 8769 blocos.
> SFENCE chama a barreira do bus síncrono exclusivo,
> sem fila pendente nem conclusão de GPU; filas futuras exigem ampliar o protocolo.
> CMOVcc já cobre as 16 condições em 16/32 bits e lê a origem mesmo se falsa.
> BSF/BSR também estão implementados, com ZF e política determinística para zero.
> Implemente contrato e estado/origem explícita da EEPROM para essa consulta e
> continue intercalando ISA, serviços e threads conforme os bloqueios reais.
> Reutilize o AOT em Rust, o bus e o estado explícito. Não use
> respostas genéricas de sucesso ou testes próprios como prova de execução do
> homebrew real. Registre o que implementou, como verificou e o que permanece
> pendente nas seis entregas.
