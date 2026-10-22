# ADR-0058 — Perfil de memória PFB: CFG0 e CFG1

Data: 2026-10-22. Base de código: `0784976`.
Estado: aceito para dois registradores; não representa o controlador PFB completo.

## Evidência e decisão

O triangle/pbkit passou do PLL (ADR-0057) e leu PFB_CFG0 em 0x18271.
O placeholder zero falhava na comparação `(CFG0 & 3) == 3`. A limpeza pb_kill
falhava antes de pb_init retornar -6. Esta entrega modela CFG0 e CFG1, ambos
lidos pelo guest, com perfil inicial de firmware público e armazenamento Rust
próprio. Não se consultou BIOS proprietária nem se copiou código de dispositivo.

Fontes públicas fixadas:

- [Cromwell, BootVgaInitializationKernelNG](https://github.com/XboxDev/cromwell/blob/86f547387184d1001d377cce97d3756c8acf91cc/drivers/video/BootVgaInitialization.c):
  escreve CFG0=0x03070103 quando xbox_ram==128 e 0x03070003 caso contrário.
- [Cromwell, BootStartBiosLoader](https://github.com/XboxDev/cromwell/blob/86f547387184d1001d377cce97d3756c8acf91cc/boot_rom/2bBootStartBios.c#L107):
  inicializa CFG1=0x11448000 junto de CFG0; a etapa posterior de vídeo ajusta
  CFG0 para o tamanho detectado. É evidência de programação de firmware,
  **não de valor físico de reset ou medição de um console**.
- [xemu, pfb.c](https://github.com/xemu-project/xemu/blob/03b988c9d76553b33ed712ba00822ffdf49f0e7d/hw/xbox/nv2a/pfb.c):
  CFG0/CFG1 têm armazenamento e readback. Não fornece o perfil inicial acima.
- [nxdk, pbkit.c](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/pbkit/pbkit.c):
  pb_init salva ambos, exige os dois bits baixos de CFG0 iguais a 3 e depois
  chama MmClaimGpuInstanceMemory. pb_set_memory copia CFG0/CFG1 para PGRAPH/RDI.

O número anteriormente lembrado 0x03070103 tem agora procedência, mas corresponde
ao ramo **128 MiB** dessa referência. Usá-lo indistintamente no console de 64 MiB
seria incorreto para o perfil escolhido. Não se inferiu capacidade de memória
apenas dos dois bits baixos nem se adotaram campos de outra geração NVIDIA.

## Contrato do recorte

`Nv2aPfb` guarda perfil, CFG0 e CFG1. `Bus::com_arena` escolhe
`FirmwareMemoryV1` somente com 64 ou 128 MiB reais na arena:

| RAM da arena | CFG0 inicial | CFG1 inicial |
|---|---|---|
| 64 MiB | 0x03070003 | 0x11448000 |
| 128 MiB | 0x03070103 | 0x11448000 |
| outro tamanho (testes) | sem modelo/telemetria | sem modelo/telemetria |

PAs 0xFD100200 e 0xFD100204, offsets NV2A 0x00100200 e 0x00100204.
Acesso modelado exige exatamente 32 bits alinhados. Leituras e escritas
preservam o valor bruto integral; aliases VA e acesso físico observam a mesma
fonte. Acesso que se sobrepõe com outra largura/desalinhamento falha explicitamente.
O preflight valida todas as páginas antes de qualquer store em RAM ou MMIO.
Offsets vizinhos continuam na telemetria, inclusive CSTATUS e NVM.

Reprogramar CFG0/CFG1 **não redimensiona a arena** nem muda MMU/DMA ou geometria
física. Readback não equivale a implementar efeitos elétricos de configuração,
tiling, proteção de PRAMIN ou PGRAPH. Esses limites permanecem em P12. A RAM
continua única para CPU/kernel/DMA, sem VRAM duplicada, unsafe novo, worker ou
mudança em Send/!Sync. Acesso aos registros não cobra ciclos extras nem cria
outro relógio; PTIMER continua sem modelo apesar das escritas observadas.

## Snapshot e migração

**BUS0 v4** acrescenta **PFBM v1** depois de NPLL v1:
tag/versão, u32 perfil, u32 CFG0, u32 CFG1 (20 bytes).
Perfil 1=FirmwareMemoryV1, 0=LegacyUnmodeled. Legado exige ambos os valores zero;
outros IDs/versões e truncamentos recusam. Valores programados no perfil 1
podem ser quaisquer u32; preservá-los não afirma que toda configuração seja
operacional no hardware.

BUS0 v1/v2/v3 migram para PFB LegacyUnmodeled: zero com telemetria e escritas
descartadas, preservando o caminho antigo do guest. BUS0 v3 continua preservando
seu PLL, v1/v2 mantêm o PLL legado do ADR-0057. IRQ segue a migração existente.
Salvar de novo mantém o perfil legado. Para adotar o perfil PFB, sessão nova.
AOTS/KRNL e demais formatos permanecem iguais; carga que falhar exige descartar
a sessão, conforme contrato do Bus.

## Verificação

Um teste do modelo confere os perfis 64/128 MiB e arenas fora do contrato.
Seis testes do Bus cobrem aliases/físico, readback, larguras/sobreposições,
ausência de store parcial RAM/MMIO, replay, metadados inválidos, truncamentos
e migrações BUS0 v1–v3. A migração PRAMDAC foi ajustada ao novo sufixo.
Três casos no Rust efetivamente emitido verificam instruções MMIO, flags/Clock,
snapshot integral, migração BUS0 v3, falta sem efeito na CPU e retomada após reparo.

O registro do hello segue **37/69 serviços**; esta entrega não implementa ordinal.
PRAMIN, MmClaimGpuInstanceMemory, PFIFO/pushbuffer e PGRAPH continuam pendentes.

## Alvo real: novo bloqueio observado

Mesmas dez raízes/17883 blocos do triangle-raw.xbe, sem modificar bytes guest:

```text
serviço de kernel não implementado: MmClaimGpuInstanceMemory (ordinal 168),
chamado de 0x000182AB; argumentos: 0xFFFFFFFF, 0x0005FE14
EIP=0x7FFE0540 (trampolim), retired=24701909, cycles=24701909, chamadas=52
```

A sessão nova leu CFG0=0x03070003 e CFG1=0x11448000, passou do guard de PFB e
parou antes de atender o ordinal 168. Nova tentativa da chamada preservou a CPU;
snapshot integral foi restaurado e salvo com bytes idênticos nesse ponto.
O teste comparativo migrou uma sessão inicial para BUS0 v3: manteve PLL válido,
PFB sem modelo e reproduziu a falta antiga em VA=0x40/PC=0x16EA2, com cadeia EBP
pb_start → pb_kill → retorno 0x18283 em pb_init, 24701938 retired e 53 chamadas.
São dois testes locais não versionados em
`target/homebrew2/roots-review-20261022/attempt-01/tests/pfb_probe.rs`;
log `target/pfb-triangle.log`. O comando completo e raízes para reproduzir a
fronteira em sessão nova estão no CONTINUIDADE_ALPHA.md.

Gate: **499 testes workspace/doctests**, **212 casos do Rust emitido em debug/release**
(196 fora de x86_64), fmt e Clippy all-targets com `-D warnings` limpos.
Release compilou também com `RUSTFLAGS="-D warnings"`.
Logs: `target/pfb-workspace.log`, `target/pfb-clippy.log`,
`target/pfb-native-release.log`.
