# ADR-0032 — NtCreateMutant, objetos guest e snapshot da sessão

Data: 2026-10-20. Estado: aceito para o perfil inicial de uma thread.

## Necessidade observada

A execução nativa do hello, com `--entry 0x19110`, chegou a `NtCreateMutant`
(192), chamado por `mtx_init` do pdclib: saída `0x28EEC`, atributos nulos e
`InitialOwner = FALSE`. O retorno reportado é `0x18CFA`; o CALL é `0x18CF4`.
O ADR-0031 documenta essa primeira execução. A criação precisa publicar um
objeto consultável e um handle guest, sem sucesso fictício ou mutex do host.

## Contrato e subconjunto implementado

Usar as declarações públicas do nxdk fixado, sem consultar BIOS. A assinatura
Xbox é stdcall/12: `PHANDLE`, `POBJECT_ATTRIBUTES`, `BOOLEAN`. Não existe argumento
`DesiredAccess`. `OBJECT_ATTRIBUTES` ocupa **12 bytes**, com `RootDirectory`,
`ObjectName` e `Attributes`; não usar o layout NT de desktop.

- Atributos nulos ou struct com os três campos zero criam mutex sem nome.
- Nome, diretório ou flags não modelados causam erro de variante não implementada,
  com nome/ordinal/retorno/argumentos. Não converter falta de suporte em NTSTATUS.
- `BOOLEAN` usa os oito bits baixos da palavra ABI. Sem proprietário inicial:
  owner ausente, recursão zero. Com proprietário: identidade guest **1**, recursão
  um. Essa identidade representa somente a thread inicial, não um scheduler.
- Handles são palavras guest positivas, alinhadas a quatro, começando em 4,
  monotonamente crescentes e sem ponteiros host. A tabela usa `BTreeMap` ordenado.
- Limite local de **4096 objetos**. Esgotamento devolve
  `STATUS_INSUFFICIENT_RESOURCES` (`0xC000009A`), sem modificar saída.
- Saída e atributos precisam estar em RAM guest com permissões corretas.
  Ponteiro nulo, wrap, página ausente, proteção ou MMIO devolvem
  `STATUS_ACCESS_VIOLATION` (`0xC0000005`), sem criar objeto ou consumir handle.
- A escrita dos quatro bytes é validada integralmente pelo bus antes de publicar
  o objeto. Acesso desalinhado válido é permitido. Sucesso devolve status zero.

`NtWaitForSingleObject`, `NtReleaseMutant`, `NtClose`, nomes, referências externas,
abandono e escalonamento continuam pendentes. Criar o objeto não implementa a
sincronização completa. O futuro scheduler deverá fornecer a identidade guest
em execução e ampliar o estado/versão para aquisição recursiva e espera.

## Estado, retorno e snapshots

`Session` passa a possuir `kernel: Dispatcher`. A API é `session.atender_kernel()`,
sem dispatcher externo que pudesse ser esquecido ao salvar. O main e os testes
gerados usam essa propriedade; projetos anteriores precisam ser regenerados.

O runtime captura e valida a continuação **antes** de executar serviço com
efeitos. Após sucesso, aplica a continuação capturada sem outra leitura falível
da pilha. Um ponteiro de saída que sobrepõe o endereço de retorno não transforma
o handle recém-escrito em destino de execução. Retorno inválido falha antes de
criar objeto ou escrever saída. Erros do dispatcher preservam a CPU pendente;
NTSTATUS de falha é uma chamada atendida e retorna normalmente ao guest.

`Snapshot for Session` escreve `AOTS` v1, CPU `AOTC` v6, bus, `KRNL` v1 e
`KMUT` v1. O estado de kernel inclui contadores, próximo handle e mutexes com
proprietário/recursão. A ordem é determinística; carga valida limites, handles
alinhados e ordenados, duplicatas, proprietário, recursão, versões e truncamento.
Nenhuma alocação de quantidade arbitrária é feita a partir de snapshot.

`AOTC` v1–v6 mantém sua compatibilidade anterior como snapshot de **CPU**.
Salvar CPU e bus separadamente não salva objetos HLE. Para retomar uma sessão
com serviços de estado, usar o snapshot integral; uma sessão nova não adivinha
objetos de snapshots antigos. Erro de carga exige descartar a sessão, conforme
o contrato geral de `Snapshot`.

## Validação

Testes do dispatcher verificam criação, unicidade, estado de propriedade,
largura de BOOLEAN, atributos, erros de memória, MMIO, stores cruzando páginas,
desalinhamento, esgotamento e restauração/rejeição de snapshots.

Quatro verificações adicionais compilam Rust emitido e executam criação/retorno,
snapshot integral com um mutex existente e outro pendente, replay determinístico,
retorno inválido sem efeitos, sobreposição da saída com retorno e NTSTATUS sem
objeto fantasma. O conjunto gerado passa a **115 casos em x86_64** (99 nos demais
hosts). O relatório usa a fonte única e passa a **2/69 serviços**; isso não mede
percentual de compatibilidade nem proximidade do alpha.

Aceite local: **271 testes do workspace/doctests**, **115 casos gerados em debug
e release** (release com `-D warnings`), Clippy global com `-D warnings`, fmt,
diff e loader do hello. O hello real foi traduzido novamente (8769
instruções/blocos), compilado e executado em debug. Passou por NtCreateMutant
e parou em **XcSHAInit (335)**, retorno reportado `0x19140`, argumento `0x28EF0`.
O fonte público `lib/xboxrt/libc_extensions/stdlib_ext_.c` confirma a ordem
`mtx_init` → `XcSHAInit` em `init_rand_s`. SHA, EEPROM e contador surgem nesse
fonte; só a chamada observada é evidência de execução até aqui.

Próximo passo: implementar o contrato e o estado SHA guest, preservando o
contexto de 116 bytes usado pelo nxdk; não apenas zerar o contexto ou retornar.
Não há framebuffer nem encerramento completo do homebrew ainda.

Logs locais ignorados: `target/mutant-workspace.log`, `target/mutant-release.log`,
`target/mutant-clippy.log`, `target/mutant-ordinal.log`, `target/mutant-memory.log`,
`target/mutant-hello-translate.log` e `target/mutant-hello-run.log`. Projeto real:
`target/alpha-hello-mutant-20261020`; dependências por caminho exigem regenerar
projetos antigos quando a API mudar. A BIOS não foi consultada.

## Referências

- [Declarações públicas do kernel no nxdk fixado](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/xboxkrnl/xboxkrnl.h).
- [NTSTATUS públicos](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/xboxkrnl/ntstatus.h).
- [mtx_init do pdclib](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/pdclib/platform/xbox/functions/threads/mtx_init.c).
- [Dispatcher](../../crates/xbox-kernel/src/dispatch.rs) e [objetos](../../crates/xbox-kernel/src/dispatch/mutants.rs).
- [Sessão e snapshot](../../crates/xbox-recomp/src/runtime.rs).
- [Testes de serviços](../../crates/xbox-kernel/tests/dispatch.rs) e [testes nativos](../../tools/xberecomp/tests/verification_kernel.inc).
