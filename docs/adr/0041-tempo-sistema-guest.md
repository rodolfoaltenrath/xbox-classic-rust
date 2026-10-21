# ADR-0041 — KeQuerySystemTime e referência UTC guest

Data: 2026-10-20. Estado: aceito para o perfil temporal determinístico do alpha.

## Contexto e fontes

O hello real parava em `KeQuerySystemTime` (128), retorno `0x20D8B`, saída
`0x7000FDD8`. O transporte ABI já conhecia stdcall/4, mas o dispatcher ainda
não recebia o Clock da sessão e não havia referência de data absoluta.

Referências públicas, consultadas sem copiar implementações:

- [Header nxdk fixado, CC0](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/xboxkrnl/xboxkrnl.h):
  assinatura `VOID NTAPI KeQuerySystemTime(PLARGE_INTEGER CurrentTime)` e saída
  em intervalos de 100 ns desde 1601-01-01 GMT. NTAPI é stdcall no Xbox.
- [Microsoft KeQuerySystemTime](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/nf-wdm-kequerysystemtime-r1):
  unidade, época e ausência de valor de retorno. A frequência de atualização de
  um Windows físico não determina a política de temporização deste perfil HLE.
- [ADR-0025](0025-relogio-aot-rdtsc.md): Clock único guest, sem tempo
  de parede host e separado de `retired`.

## Decisão

`SystemTimeReference` pertence ao dispatcher e guarda somente a data UTC do
power-on. O padrão sintético é **2000-01-01 00:00:00 UTC**, equivalente a
`125911584000000000` unidades de 100 ns desde 1601. É uma escolha explícita do
perfil, não a leitura de um RTC físico ou um campo da EEPROM. A API
`from_boot_ticks(i64)` permite outra referência não negativa, e
`Dispatcher::with_system_time` configura uma sessão nova.

O tempo consultado é:

```text
boot_ticks + floor(State.clock.cycles × 10000000 / CPU_HZ)
```

A conversão usa u128 antes da divisão e verifica o intervalo positivo de
LARGE_INTEGER antes de escrever. Não usa `Clock::nanos()`, cujo intervalo pode
ser menor que o do Clock em ciclos; não usa `retired`, SystemTime/Instant host
ou um segundo contador de tempo. RDTSC, RDMSR/TSC e consultas de sistema
compartilham a mesma autoridade temporal. A precisão de 100 ns expressa a unidade
da saída, não uma simulação de precisão física ou de interrupções do RTC.

`Session::atender_kernel` fornece seu Clock real a
`Dispatcher::atender_com_clock`. O método anterior `atender` continua disponível
para serviços sem dependência temporal; tentar consultar tempo por ele dá erro
explícito de contexto, sem presumir Clock zero. O HLE não avança ciclos por si:
a cobrança continua sendo a política sintética das instruções AOT.

O serviço captura o argumento por `Call::argument`, valida **todos os oito bytes**
como RAM gravável e só então publica a saída little-endian. Desalinhamento e
cruzamento de páginas são aceitos quando a faixa inteira é válida. Nulo, wrap,
RO/RX, página ausente e MMIO recusam. Contexto ausente e soma fora de
LARGE_INTEGER dão erros do runtime, não NTSTATUS inventado para uma função VOID.
CPU, conteúdo RAM e referência UTC não mudam nessas falhas; traduções de endereço
podem marcar Accessed/Dirty na MMU, como nos demais preflights do Bus.

O retorno é validado/capturado pela sessão antes dos efeitos. Uma saída sobre a
pilha pode sobrescrever retorno/argumento e ainda assim retomar a continuação
capturada corretamente. stdcall remove retorno e argumento; flags, outros
registradores, Clock e `retired` permanecem intactos durante o atendimento.

## Snapshot

**AOTS v1 / AOTC v7 / KRNL v4 / KMUT v2 / EEPR v1 / PCIC v1 / STIM v1**.
STIM contém apenas a referência UTC de 64 bits; o Clock continua salvo em AOTC/CLK0.
Não há contador temporal duplicado. KRNL v1–v3 migram para a referência padrão,
independentemente da configuração prévia no destino. O carregamento de kernel
continua transacional: módulos são preparados antes da atribuição final. Versões
desconhecidas, referência negativa codificada em u64 e truncamento recusam.

## Validação e execução real

- Seis testes de kernel cobrem época por calendário independente, segundo exato,
  fronteira de 100 ns, Clock u64 máximo, soma fora de LARGE_INTEGER, guardas,
  MMIO/permissões/páginas e round-trip/migração/corrupção de snapshot.
- Cinco testes de x86 → Rust compilado cobrem stdcall/64 bits, Clock diferente
  de `retired`, orçamento/polls, snapshot com referência configurada e consulta
  pendente, falhas/retry, retorno inválido e alias da pilha.
- **342 testes workspace/doctests e 145 nativos debug/release** em x86_64
  (129 nos demais hosts), Rust 1.98.1; Clippy global, fmt e diff aprovados.
- O hello mantém **quatro raízes / 9027 blocos**. Na consulta real, Clock=704
  ciclos, saída `0x7000FDD8`, valor **125911584000000009**, sem avanço do Clock
  no atendimento. Teste local adicional verifica os oito bytes e a continuação.
- O próximo bloqueio é **KeGetCurrentIrql (103)**, retorno **0x20E09**, sem
  argumentos. Registro: **15/69 serviços**, com variantes limitadas. Ainda não
  há execução completa ou framebuffer.

Próximo passo: modelar o IRQL guest do perfil inicial e sua relação com CPU,
snapshot e futuras transições/interrupções. Não retornar um literal apenas para
passar pelo assert de `lib/winapi/memory.c`; nível de IRQL e EFLAGS.IF têm
contratos distintos. Threads e serviços seguintes dependem da execução real.

Reprodução em diretório novo/vazio:

```sh
cargo run --offline -p xberecomp -- translate target/homebrew/hello.xbe --entry 0x19110 --entry 0x145E0 --entry 0x205E0 --entry 0x20889 --out target/alpha-hello-irql
cargo run --offline --manifest-path target/alpha-hello-irql/Cargo.toml
```

Logs: `target/system-time-{kernel-tests,workspace-tests,doctests,clippy}.log`,
`target/system-time-native-release-tests.log`,
`target/system-time-hello-{run,ordinals,effect-test}.log`.
O teste local está no artefato ignorado
`target/alpha-hello-msr-ratio-20261020/tests/system_time_effect.rs` e usa o XBE
real de SHA-256 `baaa88110014743864d0f38606e18eb423b0bf0da5bb4083a3e55b89a0a2af7f`.
