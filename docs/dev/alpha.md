# Primeiro alpha: homebrew real

Para retomar o projeto em outra sessão ou LLM, consulte
[CONTINUIDADE_ALPHA.md](../../CONTINUIDADE_ALPHA.md), com as seis entregas,
o estado de implementação, seus limites e os próximos passos.

Meta inicial: executar um XBE homebrew compilado por um SDK real, produzir saída
verificável e depois apresentar um framebuffer. Fixtures de bytes do projeto
continuam sendo testes; não substituem esse marco.

| Entrega | Estado | Aceite |
|---|---|---|
| 1. Memória/loader | aceite local concluído | T14 real + fixture, contrato sob Miri e benchmark seguro |
| 2. TLS/imports | infraestrutura validada; integração com homebrew pendente | TLS nxdk, FS e ABI disponíveis; dados exportados dependem da entrega 3 |
| 3. Kernel mínimo | dados, dispatcher, mutants, SHA-1, EEPROM e seções críticas (até ADR-0037); 12/69 serviços | funções efetivamente usadas pelo homebrew; erro claro nas demais |
| 4. ISA necessária | CLI/HLT, RDTSC, CMPXCHG/LOCK, SETcc, CMOVcc, BSF/BSR e CLC/STC/CMC; RDMSR e perfil de CPU/MSRs disponíveis | traduzir o código emitido pelo compilador e validar instruções adicionais |
| 5. Execução integrada | passa por RDMSR e para em HalReadWritePCISpace (46); controle/logs pendentes | carregar módulo, rodar com orçamento, logs e encerramento controlado |
| 6. Framebuffer | pendente | primeira imagem apresentada; controle/áudio ficam para o próximo marco |

As seis entregas são uma divisão de trabalho, não garantia de seis commits ou prazo.
Dependências reais de kernel/ISA só serão conhecidas após compilar o alvo.

## Alvo escolhido

[nxdk samples/hello](https://github.com/XboxDev/nxdk/tree/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/samples/hello),
revisão `14d5ee97e73347c973f1f57b68b79ec08c9e77f2`, consultada em 2026-10-20.
O sample configura vídeo 640×480, imprime texto e usa Sleep no laço. Isso oferece
um objetivo observável para vídeo e temporização; seu laço termina pelo controle
de execução do host, não por um retorno espontâneo do programa.

O alvo foi compilado no WSL/Fedora e reconstruído neste Linux/Fedora 44, com
SDK/submódulos fixados e os mesmos hashes após normalização de metadata. O loader
verificou 4 seções, 27 páginas e 72 imports: 69 funções e 3 dados. Build,
inventário, hashes e limites estão em [`homebrew.md`](homebrew.md).

A infraestrutura de uma thread inicial prepara TLS nxdk, aplica FS e transporta
chamadas stdcall/cdecl/fastcall até o dispatcher. Os três dados exportados já têm
representação guest. Os 12 serviços são RtlInitAnsiString, criação/espera/liberação
de mutant, SHA-1 Init/Update/Final, consulta integral de EEPROM e a família
Init/Enter/Leave/TryEnter de seção crítica. Há variantes recusadas explicitamente;
a contagem de serviços não mede compatibilidade.

As seções críticas usam a estrutura pública Xbox de 28 bytes na RAM guest, com
recursão, contagens e proprietário sintético da thread inicial. Nenhum KTHREAD real
ou scheduler está implícito nisso. TryEnter retorna FALSE em contenção; Enter
bloqueante e filas de espera recusam sem bloquear o host. O snapshot integral
preserva RAM, CPU, kernel e chamada pendente sem formato novo
([ADR-0037](../adr/0037-secoes-criticas-guest.md)).

O hello real traduz **9027 instruções/blocos** com as raízes `0x19110`, `0x145E0`,
`0x205E0` e `0x20889`. Passa por mutex, SHA, EEPROM, Init da seção crítica e
RDMSR; para em **HalReadWritePCISpace (46)**, retorno `0x2064D`, na leitura de
quatro bytes PCI do offset `0x6C` (bus 0, slot `0x60`). O próximo passo é modelar
a configuração PCI guest/clock do chipset exigida por essa leitura. A quarta
raiz é o case do multiplicador da CPU, ainda não inferido por passar pela pilha.

O perfil `CpuProfile::XboxRetailV1` define EBL_CR_POWERON (0x2A) nominal para
bus 133⅓ MHz × razão 5,5; demais bits são sintéticos, não dump físico. A leitura
TSC (0x10) e RDTSC usam o mesmo Clock. Índices sem modelo recusam com PC/índice,
sem efeitos. AOTC v7 salva o perfil e migra v1–v6; não há RDMSR host ou relógio
adicional ([ADR-0038](../adr/0038-rdmsr-perfil-cpu-guest.md)).

A ISA já inclui CMPXCHG/LOCK, SETcc, RDTSC, CLI/HLT, INT3/#BP, SFENCE, CMOVcc,
BSF/BSR e CLC/STC/CMC. Blocos em funções separadas corrigiram o estouro de pilha
host. O controle guest e as exceções pendentes sobrevivem a snapshot, mas ainda
não há IRQ/NMI, IDT/SEH, scheduler ou retomada automática.

A validação desta entrega tem **322 testes workspace/doctests** e **131 casos de
Rust gerado em debug/release**, incluindo seis novos para RDMSR/perfil/Clock. O Rust
fixado é 1.98.1. O aceite integrado permanece aberto: o CRT cria a thread de
`main` e ainda faltam serviços, execução completa e framebuffer. A primeira
entrega de memória usou mídia comercial apenas como dado do loader; a BIOS não
foi usada. A EEPROM padrão continua sintética, com bytes/origem explícitos.
