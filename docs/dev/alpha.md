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
| 3. Kernel mínimo | dados, dispatcher, mutants, SHA-1, EEPROM, seções críticas, PCI de clock e InterlockedExchange (até ADR-0040); 14/69 serviços | funções efetivamente usadas pelo homebrew; erro claro nas demais |
| 4. ISA necessária | CLI/HLT, RDTSC, CMPXCHG/LOCK, SETcc, CMOVcc, BSF/BSR e CLC/STC/CMC; RDMSR e perfil de CPU/MSRs disponíveis | traduzir o código emitido pelo compilador e validar instruções adicionais |
| 5. Execução integrada | passa pela troca e para em KeQuerySystemTime (128); controle/logs pendentes | carregar módulo, rodar com orçamento, logs e encerramento controlado |
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
representação guest. Os 14 serviços são RtlInitAnsiString, criação/espera/liberação
de mutant, SHA-1 Init/Update/Final, consulta integral de EEPROM e a família
Init/Enter/Leave/TryEnter de seção crítica, leitura PCI de clock e InterlockedExchange. Há variantes recusadas explicitamente;
a contagem de serviços não mede compatibilidade.

As seções críticas usam a estrutura pública Xbox de 28 bytes na RAM guest, com
recursão, contagens e proprietário sintético da thread inicial. Nenhum KTHREAD real
ou scheduler está implícito nisso. TryEnter retorna FALSE em contenção; Enter
bloqueante e filas de espera recusam sem bloquear o host. O snapshot integral
preserva RAM, CPU, kernel e chamada pendente sem formato novo
([ADR-0037](../adr/0037-secoes-criticas-guest.md)).

O hello real traduz **9027 instruções/blocos** com as raízes `0x19110`, `0x145E0`,
`0x205E0` e `0x20889`. Passa por mutex, SHA, EEPROM, Init da seção crítica e
RDMSR, leitura PCI e troca de LONG; para em **KeQuerySystemTime (128)**,
retorno `0x20D8B`, argumento `0x7000FDD8`. O próximo passo é definir época e tempo
de sistema guest derivados do Clock da sessão. A quarta raiz é o case do
multiplicador da CPU, ainda não inferido por passar pela pilha.

O perfil `CpuProfile::XboxRetailV1` define EBL_CR_POWERON (0x2A) nominal para
bus 133⅓ MHz × razão 5,5; demais bits são sintéticos, não dump físico. A leitura
TSC (0x10) e RDTSC usam o mesmo Clock. Índices sem modelo recusam com PC/índice,
sem efeitos. AOTC v7 salva o perfil e migra v1–v6; não há RDMSR host ou relógio
adicional ([ADR-0038](../adr/0038-rdmsr-perfil-cpu-guest.md)).

A consulta PCI atende o PLL do controlador DRAM e suas subleituras, com valor
público 0x01230801 e configuração compartilhada com CPU/MSRs no core. Escritas
ou áreas sem modelo recusam. KRNL v3/PCIC v1 guarda o perfil, com migração de
KRNL v1/v2; AOTC permanece v7. O hello real calcula **733333333 Hz**, conferidos
na RAM após o construtor ([ADR-0039](../adr/0039-configuracao-pci-clock-guest.md)).

InterlockedExchange usa ECX/EDX (FASTCALL/8), atualiza a RAM e devolve o valor
anterior em EAX. Valida toda a faixa antes do efeito; não há yield entre load/store
sob Bus exclusivo. Preserva snapshot/retorno capturado sem novo formato; outros
serviços Interlocked continuam ausentes
([ADR-0040](../adr/0040-interlocked-exchange-guest.md)).

A ISA já inclui CMPXCHG/LOCK, SETcc, RDTSC, CLI/HLT, INT3/#BP, SFENCE, CMOVcc,
BSF/BSR e CLC/STC/CMC. Blocos em funções separadas corrigiram o estouro de pilha
host. O controle guest e as exceções pendentes sobrevivem a snapshot, mas ainda
não há IRQ/NMI, IDT/SEH, scheduler ou retomada automática.

A validação desta entrega tem **336 testes workspace/doctests** e **140 casos de
Rust gerado em debug/release**, incluindo cinco novos para troca/fastcall/snapshot. O Rust
fixado é 1.98.1. O aceite integrado permanece aberto: o CRT cria a thread de
`main` e ainda faltam serviços, execução completa e framebuffer. A primeira
entrega de memória usou mídia comercial apenas como dado do loader; a BIOS não
foi usada. A EEPROM padrão continua sintética, com bytes/origem explícitos.
