# ADR-0090 — HalWriteSMBusValue para o LED do SMC

Data: 2026-10-26. Estado: aceito. PLANO 10.1.

## Contexto

O sample `led` do nxdk fixado chama `XSetCustomLED(XLED_GREEN, XLED_RED,
XLED_OFF, XLED_OFF)` e fica em `Sleep(1000)` para sempre. A sonda recusava no
ordinal 50: `HalWriteSMBusValue(0x20, 0x07, FALSE, 1)`, chamado de 0x12799.
`ordinal-report`: 73 imports, 41/70 serviços; a análise AOT traduz as 11834
instruções.

## Fontes

- `lib/hal/led.c` (nxdk 14d5ee9, CC0): `XSetCustomLED` escreve no SMC
  (`0x20`) o modo manual no registrador 0x07 e depois a sequência no 0x08,
  `((t1 & 0x11) << 3) | ((t2 & 0x11) << 2) | ((t3 & 0x11) << 1) | (t4 & 0x11)`;
  `XResetLED` escreve modo 0 (automático). `lib/hal/led.h`: `XLED_GREEN = 0x01`,
  `XLED_RED = 0x10`. Logo, vermelho nos bits 7–4 e verde nos bits 3–0, com o
  primeiro tempo no bit mais alto de cada metade.
- `lib/xboxkrnl/xboxkrnl.h`: `NTSTATUS HalWriteSMBusValue(UCHAR SlaveAddress,
  UCHAR CommandCode, BOOLEAN WriteWordValue, ULONG DataValue)`, stdcall/16.

## Decisão

`dispatch/smbus.rs` atende somente escrita de byte no SMC (0x20) com o comando
0x07 (valor 0 ou 1) ou 0x08 (um byte) e devolve `STATUS_SUCCESS`. O estado
(último modo, última sequência e número de escritas) é observável por
`Dispatcher::smbus()` e vai na seção `SMBC` v1, no fim do `KRNL` v15. KRNL
v1–v14 migram sem escrita de LED. O carregamento recusa estado incoerente: o
contador é zero se e somente se nada foi escrito.

Outros endereços (encoder de vídeo, sensor de temperatura), outros comandos do
SMC, escrita de palavra e valores fora do byte (ou modo ≠ 0/1) são recusados com
`VarianteNaoImplementada`, sem alterar o estado. Não há LED físico nem
temporização dos quatro tempos da sequência.

O `main` gerado imprime `SMC LED: modo=… sequência=0x.. (tempo1,…,tempo4)
escritas=N` quando houve escrita, e a sonda expõe a chave `led:`. Para samples
sem modo de vídeo, como o `led`, a sonda não exige a linha do framebuffer.

## Evidência

- Sonda led/64 (`target/sonda/led-20261026-233444-RXii1Eef.log`): 62 chamadas,
  **zero recusas**, `led: SMC LED: modo=manual sequência=0x48
  (verde,vermelho,apagado,apagado) escritas=2`, nenhum MMIO sem modelo. A
  execução termina por orçamento dentro do `Sleep` infinito do sample.
- `crates/xbox-kernel/tests/smbus.rs`: sequência do sample e `XResetLED`;
  cinco variantes recusadas sem mudar o estado; ida e volta de snapshot;
  migração do KRNL v14 sem a seção; recusa de contador incoerente. O teste
  existente de corrupção do fim do snapshot do kernel (`links.rs`) passou a
  exercitar essa recusa.

## Consequências

Primeiro serviço HAL do SMBus, restrito ao LED. Ler o SMBus
(`HalReadSMBusValue`) ou outros dispositivos exige consumidor real e outra ADR.
