# ADR-0043 — Reserva e commit privado de memória virtual guest

Data: 2026-10-20. Estado: aceito para o caminho de alocação usado pelo alpha.

## Contexto e fontes

O hello parava em `NtAllocateVirtualMemory` (184), retorno `0x20E68`.
`VirtualAlloc` pedia BaseAddress=0, RegionSize=64 KiB, ZeroBits=0,
AllocationType=0x3000 e Protect=4. O Bus já possuía RAM, MMU e bitmap de frames;
faltava representar a alocação virtual e executar o serviço.

Referências públicas, sem copiar implementações:

- [Header nxdk fixado, CC0](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/xboxkrnl/xboxkrnl.h):
  ABI Xbox com cinco argumentos, sem ProcessHandle; constantes MEM_COMMIT,
  MEM_RESERVE e PAGE_READWRITE. O catálogo já define stdcall/20.
- [Microsoft NtAllocateVirtualMemory](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-ntallocatevirtualmemory):
  parâmetros de entrada/saída, reserva/commit e arredondamento por página.
  A assinatura Xbox vem do nxdk, não da variante Windows com seis argumentos.
- [VirtualAlloc](https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-virtualalloc):
  alinhamento da base pela granularidade de alocação e conteúdo inicial zero.
- [Contrato de memória do projeto](../arch/memoria.md): espaços guest/host
  distintos, arena exclusiva, permissões e acesso físico verificado.

## Perfil implementado

Atender apenas **reserva+commit privado em uma chamada**, com endereço escolhido
pelo kernel (`*BaseAddress=0`), ZeroBits=0, MEM_RESERVE|MEM_COMMIT e PAGE_READWRITE,
em contexto Passive. Outras combinações, bases solicitadas, proteções e níveis
de IRQL recusam como variantes não implementadas. Reserva isolada, commit sobre
reserva prévia, reset, guard, top-down, decommit/release e NtFreeVirtualMemory
continuam pendentes. Não anunciar a família inteira pelo primeiro caminho real.

O perfil usa páginas de **4 KiB**, bases alinhadas a **64 KiB**, busca first-fit
determinística a partir do fim do intervalo completo da imagem XBE e limite
superior exclusivo `0x70000000` (a pilha inicial). Assim não ocupa headers,
seções nem buracos internos da imagem; pula páginas previamente mapeadas,
inclusive MMIO. São escolhas explícitas do perfil, não endereços físicos do Xbox
ou ponteiros host. Há limite de 4096 regiões e de frames efetivamente disponíveis.

As páginas recebem `Perm::RW`, sem execução no perfil AOT. Essa política protege
o código recompilado e não pretende reproduzir NX físico no Pentium III.
Pedido de proteção executável não é silenciosamente convertido. Cada página
committed usa um frame próprio; contiguidade física não é requisito. Todo o
conteúdo é zerado antes da publicação, inclusive frames anteriormente sujos.

## Validação e publicação

BaseAddress e RegionSize precisam cobrir quatro bytes de RAM legível/gravável.
Antes de alocar, o serviço captura os argumentos, valida as duas saídas, lê os
valores e verifica tamanho/arredondamento. Sobreposição entre saídas é recusada,
inclusive desalinhamento parcial e aliases físicos distintos no espaço virtual.

| Situação | Resultado do perfil |
|---|---|
| pedido suportado, RAM/VA disponíveis | STATUS_SUCCESS, base e tamanho arredondado nas saídas |
| saída nula, parcial, RO/RX, MMIO ou wrap | STATUS_ACCESS_VIOLATION, sem alocação/saídas parciais |
| tamanho zero, overflow de arredondamento ou saídas sobrepostas | STATUS_INVALID_PARAMETER |
| falta de frames ou de intervalo virtual | STATUS_NO_MEMORY |
| limite de regiões do perfil | STATUS_INSUFFICIENT_RESOURCES |
| variante válida ainda ausente | erro identificável com ordinal/chamador/argumentos |
| registro, bitmap e MMU incoerentes | erro explícito de estado, sem sucesso fictício |

O planejamento usa cópias independentes da MMU e do bitmap; não copia a arena.
Falha nessa fase não publica frames/mapeamentos. A inspeção dos mapeamentos
confere que páginas RAM existentes têm posse no bitmap e que regiões privadas
não compartilham frames ou aliases externos. Após validação, o serviço zera os
frames, publica MMU/bitmap, escreve as saídas e insere o registro ordenado.
Não há yield, MMIO ou concorrência host no protocolo atual.

Session já captura/valida a continuação antes do efeito. RegionSize pode apontar
à palavra de retorno: a escrita não provoca nova leitura do retorno modificado.
stdcall/20 remove retorno e cinco argumentos. HLE preserva flags, outros
registradores, Clock e retired. Preflight pode marcar Accessed/Dirty, como nas
demais operações do Bus; uma falha não altera conteúdo RAM nem a posse de frames.

## Estado e snapshot

**AOTS v1 / AOTC v7 / KRNL v6 / KMUT v2 / EEPR v1 / PCIC v1 / STIM v1 / IRQL v1 / VMEM v1**.
VMEM guarda o intervalo da imagem e pares ordenados base/tamanho das regiões
integralmente committed/RW. RAM, permissões e bitmap continuam exclusivamente no
Bus, sem lista física duplicada no kernel. Session restaura e confere os registros
contra MMU/frames, inclusive permissões, páginas ausentes, compartilhamento
indevido e headers da imagem. A inspeção não marca Accessed/Dirty.

KRNL v1–v5 migram sem alocações. A restauração de Session recupera o intervalo
do XBE usado para abrir a sessão e confere seus headers na RAM restaurada.
Restaurar sobre o XBE correspondente continua sendo requisito; o snapshot não
fornece código nativo de outra imagem. Dispatcher isolado sem contexto da imagem
recusa alocações; `Dispatcher::for_image` configura esse contexto. Snapshots atuais
sem contexto não herdam configuração arbitrária do destino.

A carga de kernel continua transacional para erros estruturais. Falha na carga
integral de Session exige descartar a sessão, como no contrato anterior. VMEM
recusa versão, contagem, intervalos, alinhamentos, sobreposição e limites inválidos.
FRM0 permanece v1, mas agora valida contagem, bitmap/padding e truncamento antes
de alterar o alocador. MMU0 v1 também recusa frames fora do espaço de 32 bits
e bases físicas de páginas grandes sem alinhamento de 4 MiB, evitando transbordo
na enumeração de mapeamentos. Snapshots antigos válidos continuam aceitos.

## Evidência e próximo bloqueio

- Sete testes de kernel: arredondamento, RAM suja/frames fragmentados, primeira
  faixa livre/imagem inteira, permissões, ponteiros/aliases, variantes, OOM,
  snapshot/corrupção e migração KRNL v1–v5. Um teste adicional fortalece FRM0.
- Cinco casos nativos: stdcall/20, escrita guest entre páginas, replay de duas
  alocações, falta de frames, retorno inválido, saída sobre o retorno, validação
  cruzada de snapshot e migração integral de KRNL v5.
- **354 testes workspace/doctests e 153 nativos debug/release** em x86_64
  (137 em outros hosts), Rust 1.98.1; Clippy global, fmt e diff aprovados.
- Teste local do XBE real confere **base=0x30000, tamanho=65536, 16 frames RW
  próprios e zerados**, STATUS_SUCCESS e CPU/Clock preservados. O CRT escreve
  nessa RAM e passa **0x30008 (heap+8)** como contexto da thread.
- Quatro raízes/9027 blocos chegam a **PsCreateSystemThreadEx (255)**, stdcall/40,
  retorno **0x13BF1**. Argumentos: `0x7000FE88, 0, 0x10000, 0x114, 0,
  0x18FC0, 0x30008, 0, 0, 0x13C70`. Registro: **17/69 serviços**.

Próximo passo: modelo explícito de threads guest, com handle/identidade, pilha,
TLS de 0x114 bytes e início pelo SystemRoutine 0x13C70/StartRoutine 0x18FC0.
Investigar o CRT e as raízes AOT necessárias antes de implementar. Não trocar
EIP diretamente para main, simular criação bem-sucedida ou iniciar thread host.
Espera/término e snapshot das continuações precisam fazer parte desse contrato.
Execução completa e framebuffer continuam pendentes.

Reprodução em diretório novo/vazio:

```sh
cargo run --offline -p xberecomp -- translate target/homebrew/hello.xbe --entry 0x19110 --entry 0x145E0 --entry 0x205E0 --entry 0x20889 --out target/alpha-hello-threads
cargo run --offline --manifest-path target/alpha-hello-threads/Cargo.toml
```

Logs: `target/virtual-memory-{kernel-tests,workspace-tests,doctests,clippy}.log`,
`target/virtual-memory-native-release-tests.log`,
`target/virtual-memory-hello-{run,ordinals,effect-test}.log`.
Teste local ignorado: `target/alpha-hello-msr-ratio-20261020/tests/virtual_memory_effect.rs`,
sobre o XBE de SHA-256 `baaa88110014743864d0f38606e18eb423b0bf0da5bb4083a3e55b89a0a2af7f`.
