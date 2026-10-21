# ADR-0038 — RDMSR e perfil de CPU guest

Data: 2026-10-20. Estado: aceito como perfil funcional sintético, sem precisão de ciclos.

## Contexto e fontes

O ADR-0037 levou o hello até o construtor `tsc_freq_init` (`0x205E0`). A tradução
recusava RDMSR em `0x207C6`, com ECX=0x2A. Saltar o construtor ou devolver um valor
para forçar o fallback ocultaria uma dependência real do ambiente de CPU/chipset.

Referências públicas, consultadas como fatos técnicos, sem copiar implementação:

- [Intel SDM vol. 2B, RDMSR](https://www.intel.com/content/dam/www/public/us/en/documents/manuals/64-ia-32-architectures-software-developer-vol-2b-manual.pdf),
  seção 4-532: ECX seleciona o registro, EDX:EAX recebe 64 bits, flags preservadas.
- [Intel SDM vol. 4, tabela 2-53 (P6)](https://www.intel.com/content/dam/develop/external/us/en/documents/335592-sdm-vol-4.pdf),
  seção 2-369 a 2-371: TSC em 0x10, configuração EBL_CR_POWERON em 0x2A.
- [Definições P6 publicadas pela Intel no EDK2](https://github.com/tianocore/edk2/blob/master/MdePkg/Include/Register/Intel/Msr/P6Msr.h):
  campos de frequência de bus e razão de clock. Nenhum código incorporado.
- [nxdk profiling.c na revisão fixada](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/winapi/profiling.c):
  decodificação do multiplicador, construtor e leitura PCI subsequente.

## Decisão

Traduzir RDMSR para `Operation::ReadMsr`. A execução consulta `State::read_msr`
com o ECX completo antes de alterar EAX/EDX. A leitura concluída consome o mesmo
passo sintético das demais instruções; falha não aposenta a instrução nem cobra
o passo. Flags e demais registradores permanecem. Não executar RDMSR do host.

O perfil `CpuProfile::XboxRetailV1`, identificador 1, faz parte de State:

| MSR | Resultado guest |
|---|---|
| 0x10 (TSC) | `State::clock.tsc()`, mesma autoridade de RDTSC, antes do custo da instrução |
| 0x2A (EBL_CR_POWERON) | `0x0000000001040000`: frequência de bus codificada como 133 MHz e multiplicador 5,5 |
| qualquer outro índice | `UnsupportedMsr { pc, index }`, sem efeitos de CPU/RAM |

O domínio nominal é bus 133⅓ MHz × 5,5 = CPU 733⅓ MHz; uma asserção de compilação
liga essa relação ao `CPU_HZ` já usado pelo Clock, sem arredondar o bus antes da
razão. Isso não estima latência física: continua um ciclo virtual por passo AOT.

Os outros bits de 0x2A são zero por **política sintética versionada**. Não são um
dump de configuração de Xbox, nem promessa de implementar os efeitos dos campos
de erro, APIC ou modos de energia. WRMSR, CPUID e demais MSRs continuam ausentes.
O perfil é flat/privilegiado, como CLI/RDTSC existentes; CPL, CR4 e entrega de #GP
via IDT não foram introduzidos. Um índice sem modelo não é automaticamente um
índice fisicamente inválido: o diagnóstico de suporte é deliberadamente distinto.

RDMSR não recebe tratamento especial na inferência de ponteiros: o rastreamento
continua conservador e não usa valores de EAX/EDX anteriores à instrução.

## Snapshot

`AOTC` vai de v6 para **v7**, acrescentando o ID do perfil após o breakpoint.
v1–v6 migram explicitamente para `XboxRetailV1`; v1–v3 continuam com Clock zero,
v1–v4 com CPU ativa e v1–v5 sem breakpoint. ID desconhecido ou versão zero é erro.
Preservam-se as validações/truncamentos existentes; em erro de carga, descartar a
sessão conforme o contrato Snapshot. AOTS v1, KRNL v2, KMUT v2 e EEPR v1 permanecem.

O MSR de TSC não tem contador próprio. Salvar o perfil e o Clock preserva leituras
depois de retry, fatias de execução e snapshot integral da sessão. Projetos antigos
podem ser regenerados para incorporar RDMSR e suas novas raízes de análise.

## Validação e execução real

Cinco testes no workspace verificam IR/prefixos, MSRs sem modelo, leitura sem
efeitos, snapshot v7, IDs/versões inválidos e migração independente de v1 a v6.
Seis casos compilam x86 → Rust e verificam preservação de flags/registradores,
ECX completo, carry EDX:EAX, TSC compartilhado com RDTSC, erro/retry, orçamento,
paradas, esgotamento de tempo, snapshot integral e falta posterior à leitura.
Não há oráculo RDMSR físico: seria privilegiado e dependeria do modelo host.

Resultado: **322 testes workspace/doctests**, **131 casos nativos em debug/release**
(115 fora de x86_64), Clippy global, fmt e diff aprovados no Rust 1.98.1.
Kernel continua com 12/69 serviços, não um percentual de compatibilidade.

No hello de hash normalizado `baaa88110014743864d0f38606e18eb423b0bf0da5bb4083a3e55b89a0a2af7f`:

1. Raízes `0x19110`, `0x145E0`, `0x205E0` traduzem **9024 blocos**. A execução
   passa por RDMSR e chega a `UnknownTarget`, JMP `0x2081E` → `0x20889`.
2. O destino é o case que devolve multiplicador 55 (5,5 × 10); o compilador carrega
   a tabela por um slot de pilha, ainda não inferido. Incluir `--entry 0x20889`
   declara o destino observado, sem alterar os bytes ou pular instruções.
3. Com as quatro raízes, **9027 blocos** executam até `HalReadWritePCISpace` (46),
   retorno `0x2064D`, argumentos `0, 0x60, 0x6C, 0x7000FE9C, 4, 0`.
   A próxima entrega deve representar configuração PCI guest/clock do chipset
   antes de atender essa leitura; não escrever valor escolhido só para contornar
   o cálculo. Threads, execução completa e framebuffer permanecem pendentes.

Reprodução em diretório novo/vazio:

```sh
cargo run --offline -p xberecomp -- translate target/homebrew/hello.xbe --entry 0x19110 --entry 0x145E0 --entry 0x205E0 --entry 0x20889 --out target/alpha-hello-pci
cargo run --offline --manifest-path target/alpha-hello-pci/Cargo.toml
```

Logs locais ignorados: `target/msr-{contract-tests,workspace-tests,doctests,clippy}.log`,
`target/msr-native-release-tests.log`, `target/msr-hello-{translate,run}.log` e
`target/msr-hello-ratio-{translate,run}.log`.

## Como reverter

Remover a operação/decoder/emissão de RDMSR mantém os demais caminhos de ISA.
Remover também o perfil exige decidir como tratar snapshots AOTC v7; não reduzir
a versão silenciosamente. O Clock não ganhou nova autoridade ou estado oculto.
