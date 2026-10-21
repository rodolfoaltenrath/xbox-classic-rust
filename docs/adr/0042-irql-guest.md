# ADR-0042 — KeGetCurrentIrql e prioridade explícita da CPU guest

Data: 2026-10-20. Estado: aceito para consulta no perfil HLE de uma CPU.

## Contexto e fontes

Depois do tempo de sistema, o hello real parava em `KeGetCurrentIrql` (103),
retorno `0x20E09`, sem argumentos. A chamada vem do assert de `VirtualAlloc` em
`lib/winapi/memory.c`. Responder zero sem representar a prioridade de execução
ocultaria o contrato requerido por futuros serviços de sincronização/interrupção.

Referências públicas, sem copiar implementações:

- [nxdk xboxkrnl.h fixado, CC0](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/xboxkrnl/xboxkrnl.h):
  `KIRQL NTAPI KeGetCurrentIrql(void)`; KIRQL é UCHAR e os níveis públicos
  PASSIVE/APC/DISPATCH valem 0/1/2.
- [nxdk exports fixados](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/xboxkrnl/xboxkrnl.exe.def):
  ordinal 103, stdcall/0; Raise/Lower são serviços distintos, 160/161.
- [Microsoft KeGetCurrentIrql](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/nf-wdm-kegetcurrentirql):
  consulta o IRQL corrente e não recebe argumentos.
- [Microsoft Managing hardware priorities](https://learn.microsoft.com/en-us/windows-hardware/drivers/kernel/managing-hardware-priorities):
  prioridade lógica e contextos de execução. Esta referência não define o layout
  físico de KPCR/APIC do Xbox nem amplia o perfil de interrupções do projeto.

## Decisão e limites

O dispatcher possui um único `Irql`, autoridade de prioridade lógica da única
CPU guest. Não é estado de uma thread host nem uma cópia por thread guest.
`Irql::Passive` é o contexto inicial explícito: este runtime inicia o título e
ainda não entrega IRQ/APC/DPC. Os níveis representáveis são Passive, Apc e
Dispatch, correspondentes a 0/1/2. O construtor `Dispatcher::with_irql` permite
definir o contexto inicial de uma sessão nova; não executa uma elevação/redução
de prioridade ou entrega eventos pendentes.

`KeGetCurrentIrql` retorna o nível armazenado, com extensão para u32 em EAX
pela convenção HLE. O contrato público exige o byte KIRQL; o perfil escolhe
zerar os bits altos. A consulta não lê argumentos, altera prioridade, escreve
RAM, cede execução ou cobra ciclos. Session aplica somente EAX/ESP/EIP e a
indicação de retorno ao host quando a continuação é a sentinela. stdcall/0 remove
apenas a palavra de retorno. Flags, demais registradores, Clock e retired são
preservados. Retorno inválido recusa antes do atendimento e permite retry.

IRQL e EFLAGS.IF são independentes. CLI continua alterando somente IF guest;
não muda o nível lógico. Na chamada real, **IRQL=Passive e IF=0**. Os testes
também consultam níveis não passivos antes/depois de CLI para demonstrar que o
resultado vem do estado, sem uma regra derivada de IF ou um literal de sucesso.

O perfil ainda não representa DIRQLs, máscaras de APIC/PIC, campos físicos de
KPCR, filas/entrega de IRQ/APC/DPC ou restrições de IRQL de todas as outras APIs.
Níveis desconhecidos recusam na construção a partir de byte ou no snapshot;
não são normalizados para Passive. `KfRaiseIrql` (160), `KfLowerIrql` (161) e
outras variantes de transição continuam ausentes. Implementá-las exige modelar
ordem de transições, máscaras e entrega de eventos antes de alterar o nível.
O estado inicial configurável e o snapshot não anunciam suporte a essas APIs.

## Snapshot

**AOTS v1 / AOTC v7 / KRNL v5 / KMUT v2 / EEPR v1 / PCIC v1 / STIM v1 / IRQL v1**.
IRQL contém um byte com o nível explícito. AOTC continua preservando IF; não há
prioridade duplicada na CPU ou em RAM. KRNL v1–v4 migram para Passive, sem herdar
o estado do destino ou inferir IRQL de IF. Os demais campos e migrações continuam
preservados. A carga de kernel prepara todos os módulos antes do commit, recusando
versões desconhecidas, níveis sem modelo e truncamento sem alteração parcial.

## Validação e execução real

- Quatro testes de kernel cobrem níveis configurados, stdcall/0 com somente a
  palavra de retorno mapeada, RAM intacta, Raise/Lower ausentes, round-trip,
  corrupção/truncamento e migração KRNL v1–v4 preservando EEPROM/tempo/contadores.
- Três testes de x86 → Rust compilado cobrem independência de CLI/IF, preservação
  de CPU/tempo, polls sem efeito, tail-call, chamada pendente com nível não passivo
  no snapshot, retorno inválido e retry. O teste de diagnóstico sem argumentos
  usa agora KeGetCurrentThread (104), que continua ausente.
- **346 testes workspace/doctests e 148 nativos debug/release** em x86_64
  (132 nos demais hosts), Rust 1.98.1; Clippy global, fmt e diff aprovados.
- O hello mantém **quatro raízes / 9027 blocos**, consulta Passive e passa pelo
  assert do SDK. Teste local confere EAX, continuação, flags e Clock preservados.
- O próximo bloqueio é **NtAllocateVirtualMemory (184)**, stdcall/20, retorno
  **0x20E68**. Argumentos: `0x7000FD94, 0, 0x7000FD98, 0x3000, 4`.
  Os ponteiros contêm **BaseAddress=0 e RegionSize=0x10000 (64 KiB)**. Registro:
  **16/69 serviços**. Execução completa e framebuffer continuam pendentes.

Próximo passo: alocação virtual guest no caminho real, com reserva/commit,
alinhamento, permissões, frames, zero inicial, validação das saídas e snapshot
do estado de alocação. Reutilizar Bus/MMU/alocador existentes e conferir os
contratos antes de implementar; não fornecer endereços host ou ponteiros fixos.

Reprodução em diretório novo/vazio:

```sh
cargo run --offline -p xberecomp -- translate target/homebrew/hello.xbe --entry 0x19110 --entry 0x145E0 --entry 0x205E0 --entry 0x20889 --out target/alpha-hello-virtual-memory
cargo run --offline --manifest-path target/alpha-hello-virtual-memory/Cargo.toml
```

Logs: `target/irql-{kernel-tests,workspace-tests,doctests,clippy}.log`,
`target/irql-native-release-tests.log`, `target/irql-hello-{run,ordinals,effect-test}.log`.
Teste local ignorado: `target/alpha-hello-msr-ratio-20261020/tests/irql_effect.rs`,
sobre o XBE de SHA-256 `baaa88110014743864d0f38606e18eb423b0bf0da5bb4083a3e55b89a0a2af7f`.
