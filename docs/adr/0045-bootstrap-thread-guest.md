# ADR-0045 — Bootstrap cooperativo de uma thread guest

Data: 2026-10-21. Estado: aceito para o perfil limitado de uma filha por sessão.

## Contrato e implementação

O hello real parava em PsCreateSystemThreadEx (255), stdcall/40. O CRT do nxdk
não chama main diretamente: cria WinapiThreadStartup como SystemRoutine, que
consulta KeGetCurrentThread (104), inicializa TLS/FLS e chama StartRoutine com
o contexto. Este chama o wrapper guardado no heap. Foram usados apenas fontes
públicas do nxdk, fixadas em `14d5ee97e73347c973f1f57b68b79ec08c9e77f2`:

- [thread.c](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/winapi/thread.c);
- [crt0.c](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/pdclib/platform/xbox/crt0.c);
- [xboxkrnl.h](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/xboxkrnl/xboxkrnl.h).

O kernel implementa criação, identificação e término (PsTerminateSystemThread,
258). O catálogo passa de **17/69 para 20/69 serviços**, sempre com variantes
limitadas. O contador não mede conclusão do homebrew.

Uma única filha, identidade 2, executa enquanto o pai, identidade 1, fica
estacionado. São contextos guest sobre a mesma CPU e Bus exclusivo; nenhuma
thread host é criada. Session conclui a ABI da criação e guarda a continuação
do pai antes de entrar em SystemRoutine(StartRoutine, StartContext).
Registradores, flags, FS e continuação pertencem ao contexto; Clock, retired,
perfil de CPU e IRQL permanecem únicos. A entrada da filha tem IF=1.

O recurso usa os frames zerados e o planejamento de VMEM: pilha RW sem execução,
arredondada a 64 KiB (mínimo 64 KiB, máximo 1 MiB), seguida de uma página de
metadados. O TLS pode ser ausente ou ocupar 4+16*n bytes até meia pilha.
O slot fica no topo da pilha; os dados começam quatro bytes depois, alinhados a
16 bytes. O kernel disponibiliza RAM zerada; **o guest copia o template e
inicializa o slot**, sem pular o bootstrap.

FS aponta ao TIB próprio. O prefixo KTHREAD representado termina em TlsData,
offset 0x28; StackBase/StackLimit/KernelStack também são preenchidos. Não é um
ETHREAD completo. O prefixo fica no fim da página de metadados; a página seguinte
permanece ausente, fazendo acesso além do prefixo falhar. O TIB/prefixo é RAM
guest RW, sem ponteiros host.

Handles de thread e mutants compartilham o mesmo contador e não colidem.
Mutants e seções críticas usam a identidade ativa, incluindo recursão e recusa
de liberação pelo outro proprietário. TryEnter em contenção devolve FALSE;
espera bloqueante continua recusada. Esperar na filha terminada retorna WAIT_0.
Término com mutant adquirido recusa antes dos efeitos: abandono não é modelado.

PsTerminateSystemThread não retorna ao código seguinte ao CALL. Session restaura
o pai após validar o término. Um RET ao sentinela da filha também é conclusão
local: o main gerado consulta complete_thread_return antes de anunciar retorno
do processo. O status fica no objeto, e os recursos são retidos nesta versão.

## Falhas e limites

Antes de alocar, são validados todos os argumentos/saídas, aliases físicos das
duas saídas e código RX imutável dentro da imagem para ambas as rotinas. O
retorno do pai é capturado/validado antes do serviço, inclusive se a saída
sobrepuser sua pilha. OOM e parâmetros inválidos não publicam recurso ou handle.

O perfil exige Passive, extensão zero, SystemRoutine explícita, sem suspensão ou
flag de debugger. Só os bytes baixos dos BOOLEANs são significativos. Segunda
criação, espera em thread viva, preempção, Sleep/yield, timeout, NtClose,
liberação/reutilização de recursos, KTHREAD da thread inicial e campos adicionais
de ETHREAD não estão implementados. Isso é bootstrap cooperativo, ainda sem
escalonamento geral nem garantia de progresso do pai se a filha entrar em laço.
Os limites de fatias existentes impedem que a CLI rode indefinidamente.

## Snapshot

Formato atual: **AOTS v2 / AOTC v7 / KRNL v7 / KMUT v3 / THRD v1 / TCXT v1**,
além de EEPR/PCIC/STIM/IRQL/VMEM v1 já existentes. THRD guarda o recurso,
entradas, handle e fase/status. TCXT guarda o pai sem duplicar Clock/retired.
KMUT v3 permite proprietário 2; KRNL confere que a filha existe.

A restauração cruza handle/objetos, regiões VMEM, RAM/MMU/bitmap, TIB/KTHREAD,
guarda, código imutável, CPU ativa e continuação do pai. Não marca Accessed/Dirty
durante a inspeção. Ready é estado transitório do kernel e não é aceito como
snapshot de Session. AOTS v1/KRNL v1–v6 migram sem filha/pai; estados de mutant
legados continuam migrando. Como antes, erro de carga exige descartar a sessão.

## Validação

Windows/Rust 1.98.1: **365 testes workspace/doctests**, **162 casos nativos
debug/release** (146 nos demais hosts; release com `-D warnings`). Oito testes
de kernel verificam recursos/ABI, falhas/OOM, propriedade, handles e snapshots.
Seis casos de Rust realmente emitido cobrem TLS pelo guest, contexto de início,
término noreturn/RET, retomada do pai, relógio global, replay integral, migração,
recusa sem efeitos e saída sobre a pilha do pai. O teste da CLI gerada também
confere que o RET da filha não seja anunciado como retorno do processo.

Clippy global com `-D warnings`, fmt e diff devem acompanhar o aceite.
Logs locais: `target/threads-workspace.log`, `target/threads-native-release.log`,
`target/threads-native-main.log`, `target/threads-clippy.log`.

## Reprodução do homebrew

As seis raízes anteriores continuam traduzindo 9454 blocos, mas agora a execução
passa pela criação, KeGetCurrentThread, TLS/FLS e StartRoutine. O primeiro destino
dinâmico novo é **main_wrapper, 0x18C90**, chamado de 0x18FF4. Incluí-lo produz
10170 blocos e revela **0x139B0**, chamado da tabela de inicializadores em
0x18E84. Não há decodificação x86 ou fallback interpretado em execução.

Com ambas as raízes adicionais, são **10444 instruções/blocos**:

```powershell
cargo run --offline -p xberecomp -- translate target/homebrew/hello.xbe --entry 0x19110 --entry 0x145E0 --entry 0x205E0 --entry 0x20889 --entry 0x13C70 --entry 0x18FC0 --entry 0x18C90 --entry 0x139B0 --out target/alpha-hello-thread-crt
cargo run --offline --manifest-path target/alpha-hello-thread-crt/Cargo.toml --target-dir target/recomp-hello-check
```

O módulo foi compilado/executado: o bloqueio observado é **UnknownTarget no JMP
0x14E86 → 0x14F14**, um case de tabela dentro do CRT. O disassembly público
confirma o JMP via EAX após leitura de 0x25168 + índice*4. O próximo trabalho é
incluir/inferir esse case e seguir o diagnóstico, acrescentando ISA/serviços
somente quando necessários. Log: `target/threads-hello-crt.log`.

Usar diretório de saída novo/vazio. A entrada de main não foi pulada nem
substituída por sucesso: ainda faltam serviços/ISA descobertos pelo CRT, controle
integrado de execução e framebuffer. BIOS e mídia comercial não foram usadas.
