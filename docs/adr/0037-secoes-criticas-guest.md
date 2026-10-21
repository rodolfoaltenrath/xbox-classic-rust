# ADR-0037 — Seções críticas na RAM guest

Data: 2026-10-20. Estado: aceito para o perfil de uma thread guest.

## Contexto e fontes

Após os ADRs 0035/0036, o hello real parava em `RtlInitializeCriticalSection`
(291), retorno `0x11063`, objeto `0x2810C`, no construtor global `0x145E0`.

As declarações públicas CC0 do nxdk fixado definem uma estrutura Xbox de **28
bytes**, diferente da estrutura desktop Windows, e a família stdcall/4:

- [xboxdef.h](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/xboxkrnl/xboxdef.h): layout de `RTL_CRITICAL_SECTION`.
- [xboxkrnl.h](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/xboxkrnl/xboxkrnl.h): Init/Enter/Leave retornam VOID; TryEnter retorna BOOLEAN e admite aquisição recursiva.
- [sync.c](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/winapi/sync.c): wrappers WinAPI usam os serviços de kernel.

São referências de interface/comportamento; não foi copiada implementação de
outro emulador nem usado binário de kernel proprietário.

## Decisão

Implementar `RtlInitializeCriticalSection` (291), `RtlEnterCriticalSection` (277),
`RtlLeaveCriticalSection` (294) e `RtlTryEnterCriticalSection` (306) no dispatcher,
com todo o estado na RAM guest e sem tabela host por endereço.

| Offset | Campo | Estado inicial do perfil HLE |
|---|---|---|
| 0–15 | Evento embutido | tipo 1, tamanho 4 palavras, não sinalizado, lista vazia autorreferente em objeto + 8 |
| 16 | LockCount (LONG) | -1 |
| 20 | RecursionCount (LONG) | 0 |
| 24 | OwningThread (PVOID guest) | 0 |

Os offsets são públicos. Os bytes iniciais do evento e a identidade de proprietário
são decisões explícitas deste perfil HLE, sem alegação de equivalência byte a byte
com um kernel físico. `OwningThread = INITIAL_THREAD` (1) é uma identidade sintética,
como nos mutants; **não é endereço de KTHREAD**. APIs que dependem de KTHREAD ainda
precisam de um modelo próprio. Não armazenar ponteiro host nesse campo.

Sem esperadores, a seção livre tem `LockCount=-1`, recursão/proprietário zero.
Ao adquirir, o proprietário passa a 1, a recursão sobe e `LockCount=recursão-1`.
Entradas recursivas mantêm esse invariante. Cada saída reduz ambas as contagens;
a última zera o proprietário. TryEnter retorna 1 quando adquire/recursa e 0
quando há outro proprietário, sem modificar o objeto nesse último caso.

Enter em seção de outra thread, filas/contagens com esperadores, evento sinalizado
e estouro de recursão (`i32::MAX`) são erros de variante não implementada, sem
efeitos. Exigem scheduler ou entrega de exceção guest; não bloquear a thread do
host nem inventar NTSTATUS para funções VOID. Leave sem propriedade, estrutura
não inicializada/incoerente e ponteiros inválidos são erros de argumento.
As variantes AndRegion permanecem ausentes: envolvem estado de região/APCs.

Todos os 28 bytes são validados antes do commit, inclusive através de páginas;
RAM sem permissão, MMIO e wrap de endereço são recusados. Não há alterações
parciais em recusas. Init reconstrói o objeto no perfil local; o chamador continua
responsável por não reinicializar uma seção em uso.

Sem estado host novo, **AOTS v1 / AOTC v6 / KRNL v2 / KMUT v2 / EEPR v1**
permanecem. O snapshot integral da sessão preserva seção, CPU e chamada pendente.
Uma cópia arbitrária de uma seção para outro endereço não é suportada: os
ponteiros da lista interna continuam associados ao endereço original.

## Evidência e próximo bloqueio

- Nove testes de kernel cobrem layout/guardas, recursão, TryEnter, propriedade,
  contagens/eventos inválidos, esperadores, overflow, MMIO, permissões e páginas.
- Três testes compilam x86 → Rust e verificam ABI stdcall, BOOLEAN observado pelo
  guest, recursão/snapshot integral, recusa sem alterar CPU e retry da chamada.
- O hello foi reconstruído no Linux/Fedora 44 com o mesmo SHA-256 normalizado da
  referência: `baaa88110014743864d0f38606e18eb423b0bf0da5bb4083a3e55b89a0a2af7f`.
- Com `--entry 0x19110 --entry 0x145E0`, os 8791 blocos compilam e executam além
  de Init, até `UnknownTarget { pc: 0x18E23, target: 0x205E0 }`.
- Ao acrescentar `--entry 0x205E0`, a tradução recusa **RDMSR em `0x207C6`**.
  O disassembly carrega `ECX=0x2A`. A origem pública é `get_cpu_multiplier`, chamada
  pelo construtor `tsc_freq_init` em
  [profiling.c](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/winapi/profiling.c),
  que consulta `EBL_CR_POWERON` e depois pode ler PCI para calcular a frequência.
  Isso requer contrato explícito de CPU/MSRs guest e relação com o Clock; não
  executar RDMSR do host nem devolver valor escolhido apenas para evitar o caminho.

Logs locais: `target/critical-kernel-tests.log`,
`target/critical-{workspace-tests,doctests}-rust198.log`,
`target/critical-native-release-tests.log`, `target/critical-clippy-rust198.log`,
`target/critical-hello-{translate,constructors-translate}.log`,
`target/critical-hello-run-rust198.log` e manifesto do
SDK em `target/homebrew/`. Registro: **12/69 serviços**, com variantes limitadas;
317 testes workspace/doctests e 125 casos nativos em debug/release.
Kernel mínimo completo, threads, execução integrada e framebuffer seguem abertos.

## Como reverter

Remover as quatro entradas do dispatcher/IMPLEMENTADOS e o módulo `critical`.
Nenhum formato de snapshot foi alterado; RAM contendo seções continua opaca ao bus.
