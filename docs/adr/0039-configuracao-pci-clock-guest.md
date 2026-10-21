# ADR-0039 — Configuração PCI de clock guest

Data: 2026-10-20. Estado: aceito para a leitura parcial do perfil inicial.

## Contexto e fontes

Após RDMSR, o hello parava em `HalReadWritePCISpace` (46), retorno `0x2064D`,
argumentos `0, 0x60, 0x6C, 0x7000FE9C, 4, 0`. O construtor de frequência consulta
o PLL para calcular o clock da CPU. Retornar zero apenas escolheria o fallback;
inventar um registro para avançar perderia a correspondência CPU/chipset.

Fontes públicas consultadas como fatos técnicos, sem incorporar código externo:

- [nxdk xboxkrnl.h, revisão fixada (CC0)](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/xboxkrnl/xboxkrnl.h):
  `VOID HalReadWritePCISpace(ULONG, ULONG, ULONG, PVOID, ULONG, BOOLEAN)`, stdcall/24.
- [nxdk profiling.c](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/winapi/profiling.c)
  e [pbkit.c](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/pbkit/pbkit.c):
  consumidor do registro; byte baixo é divisor e byte seguinte é multiplicador.
- [XboxDevWiki PCI, controlador DRAM 00.00:3](https://xboxdevwiki.net/PCI#00.00:3_-_DRAM_Controller):
  inventário público apresenta `0x01230801` no offset 0x6C. Trata-se de um fato
  técnico de configuração, não de um dump proprietário incorporado ao projeto.
- [Microsoft PCI_SLOT_NUMBER](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/ns-wdm-_pci_slot_number):
  formato NT com device em 5 bits e function nos 3 seguintes. O slot 0x60
  corresponde a device 0, function 3, consistente com o inventário e o consumidor nxdk.

## Decisão

Adicionar `PciConfig` ao dispatcher, com perfil imutável `PciProfile::RetailClockV1`
(ID 1). Representa **somente os quatro bytes do registro PLL** do bus 0, slot NT
0x60, offsets 0x6C..0x70. Leitura integral e subleituras de byte/word/desalinhadas
são fatias little-endian dessa configuração. Não há uma imagem genérica cheia de
zeros nem enumeração fictícia de dispositivos ausentes.

`xbox_core::hardware::RetailClockV1` centraliza EBL_CR_POWERON e FSB_PLL_CONFIG.
O runtime passa a obter o mesmo MSR do ADR-0038 dessa autoridade, sem mudar seus
bits ou o ID de CPU. O PLL publicado tem divisor 1 e multiplicador 8. Cristal
nominal de 16⅔ MHz × 8 e razão de CPU 5,5 correspondem a 733⅓ MHz; os
arredondamentos da equação nxdk dão **733333333 Hz**, iguais ao CPU_HZ do Clock.
Uma asserção de compilação mantém essa relação. Os outros campos do registro
têm readback fixo do perfil, sem efeitos de programação/modelo elétrico.

O perfil é uma configuração funcional declarada, não compatibilidade prometida
com todas as revisões físicas. Seus valores/IDs não podem mudar silenciosamente.
Escritas de PLL exigirão rever tempo, MSRs e efeitos do dispositivo em conjunto.

Contrato HLE desta entrega:

| Entrada | Resultado |
|---|---|
| leitura contida no registro PLL conhecido | copia os bytes para RAM guest; EAX=0 por convenção VOID |
| comprimento zero dentro da faixa, incluindo 0x70 | não acessa Buffer; EAX=0 |
| escrita, outra função ou intervalo ainda não modelado | erro de variante, com nome/ordinal/chamador/argumentos |
| bus/slot fora de 8 bits ou intervalo fora dos 256 bytes convencionais | erro de argumento, sem mascarar bits/wrap |
| Buffer nulo, ausente, RO/RX, MMIO, wrap ou página parcial | erro de argumento, sem store parcial |

`WritePCISpace` usa só o byte baixo de BOOLEAN. Todos os bytes da saída são
validados como RAM gravável antes do primeiro store. A saída é cópia: alterações
do guest não modificam o PLL. Não há port I/O ou consulta ao PCI host.
Não inventar NTSTATUS para a função VOID; recusas mantêm a chamada pendente.

O modelo fica inicialmente em kernel, dono do perfil da sessão. Portas PCI e
dispositivos completos futuros devem usar a mesma autoridade de configuração,
sem criar uma segunda tabela HLE/MMIO divergente.

## Snapshot

`KRNL` passa a **v3**, acrescentando `PCIC` v1 com ID do perfil. Os bytes vêm do
perfil versionado, não de estado oculto. KRNL v1/v2 migram para o perfil PCI
inicial, preservando mutants/EEPROM/contadores conforme seus contratos anteriores.
Versão/ID desconhecidos e truncamentos recusam; a carga do dispatcher publica
o estado apenas depois de validar todas as seções. AOTS v1, AOTC v7, KMUT v2 e
EEPR v1 permanecem. Só CPU/RAM continua insuficiente para salvar uma sessão.

## Evidência e próximo passo

- Nove testes de kernel cobrem bytes/guardas, frequência, todas as subleituras,
  BOOLEAN, comprimento zero, erros/variantes, páginas, MMIO e migração/snapshot.
- Quatro casos compilam x86 → Rust, verificando stdcall/24 e preservação da CPU,
  cálculo de frequência pelo código guest com MUL/DIV, subleitura, recusa/retry
  e snapshot integral com chamada PCI pendente.
- **331 testes workspace/doctests e 135 casos nativos debug/release** (119 fora
  de x86_64), no Rust 1.98.1; Clippy global, fmt e diff aprovados.
- O hello canônico mantém as quatro raízes e **9027 blocos**. A leitura PCI é
  atendida, e o programa avança até **InterlockedExchange (54)**, retorno
  `0x19DC7`, argumentos `0x29158, 1`. O kernel tem **13/69 serviços**, com variantes
  limitadas; isso não mede compatibilidade.
- Um teste local adicional sobre o XBE real, após essa execução, leu o `tsc_freq`
  do nxdk em 0x27D10/0x27D14: **733333333 Hz**, parte alta zero. Não é fixture.
  O teste e o XBE ficam ignorados em `target/alpha-hello-msr-ratio-20261020`;
  log `target/pci-hello-clock-test.log`.

O próximo passo é modelar InterlockedExchange e os efeitos atômicos da família
efetivamente usados, respeitando a ABI e RAM guest. Não devolver sucesso sem
trocar o valor e retornar o anterior. Scheduler, threads, execução completa e
framebuffer permanecem pendentes.

Reprodução, escolhendo diretório novo/vazio:

```sh
cargo run --offline -p xberecomp -- translate target/homebrew/hello.xbe --entry 0x19110 --entry 0x145E0 --entry 0x205E0 --entry 0x20889 --out target/alpha-hello-atomics
cargo run --offline --manifest-path target/alpha-hello-atomics/Cargo.toml
```

Logs locais: `target/pci-{kernel-tests,workspace-tests,doctests,clippy}.log`,
`target/pci-native-release-tests.log`, `target/pci-hello-{run,ordinals,clock-test}.log`.
O XBE usado tem SHA-256 `baaa88110014743864d0f38606e18eb423b0bf0da5bb4083a3e55b89a0a2af7f`.

## Como reverter

Remover o ordinal 46 de IMPLEMENTADOS/dispatcher recusa novamente o serviço.
Remover PciConfig exige tratar explicitamente snapshots KRNL v3; não reduzir a
versão mantendo payload distinto. As constantes compartilhadas não mudaram os
resultados do MSR 0x2A ou do Clock existente.
