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
| 3. Kernel mínimo | serviços anteriores e bootstrap de uma filha guest com pilha/TLS próprios, identificação e término (ADR-0045); 20/69 serviços | funções efetivamente usadas pelo homebrew; erro claro nas demais |
| 4. ISA necessária | oito raízes traduzem 10444 blocos; próximo destino indireto 0x14F14 | traduzir o código emitido pelo compilador e validar instruções adicionais |
| 5. Execução integrada | executa bootstrap e StartRoutine; para no JMP 0x14E86 → 0x14F14; controle/logs pendentes | carregar módulo, rodar com orçamento, logs e encerramento controlado |
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
representação guest. Os 17 serviços são RtlInitAnsiString, criação/espera/liberação
de mutant, SHA-1 Init/Update/Final, consulta integral de EEPROM e a família
Init/Enter/Leave/TryEnter de seção crítica, leitura PCI de clock, InterlockedExchange, KeQuerySystemTime, KeGetCurrentIrql e NtAllocateVirtualMemory. Há variantes recusadas explicitamente;
a contagem de serviços não mede compatibilidade.

As seções críticas usam a estrutura pública Xbox de 28 bytes na RAM guest, com
recursão, contagens e identidade guest ativa (pai 1 ou filha 2). O prefixo KTHREAD
da filha é limitado ao bootstrap; não há escalonamento geral. TryEnter retorna FALSE em contenção; Enter
bloqueante e filas de espera recusam sem bloquear o host. O snapshot integral
preserva RAM, CPU, kernel e chamada pendente sem formato novo
([ADR-0037](../adr/0037-secoes-criticas-guest.md)).

O hello real traduz **10444 instruções/blocos** com as raízes `0x19110`, `0x145E0`,
`0x205E0`, `0x20889`, `0x13C70`, `0x18FC0`, `0x18C90` e `0x139B0`. Passa pelos
serviços anteriores e cria a filha com pilha de 64 KiB e TLS de 0x114 bytes.
Executa SystemRoutine, inicializa TLS/FLS no guest e entra em StartRoutine e
main_wrapper. O bloqueio atual é **UnknownTarget: JMP 0x14E86 → 0x14F14**, no CRT.
As novas raízes são main_wrapper e um inicializador descobertos pela execução.

O [ADR-0045](../adr/0045-bootstrap-thread-guest.md) guarda a continuação do pai
sem copiar Clock/retired; término da filha restaura esse contexto. A filha roda
primeiro, sem preempção ou yield. Espera na filha terminada funciona; espera viva,
segunda criação, Sleep e escalonamento geral continuam pendentes. Não há garantia
de progresso do pai se a filha entrar em laço. O prefixo KTHREAD não é ETHREAD
completo. Snapshot AOTS v2/KRNL v7/KMUT v3 acrescenta THRD v1/TCXT v1, migrando
formatos anteriores sem threads. Recursos são retidos; NtClose/liberação faltam.

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

KeQuerySystemTime escreve oito bytes em unidades de 100 ns desde 1601/UTC,
com referência sintética de 2000-01-01 no power-on, configurável e somada ao Clock
único. Session passa Clock ao dispatcher; não há tempo host ou contador paralelo.
KRNL v4/STIM v1 preservam a referência e migram v1–v3 para o padrão. A saída real
foi conferida em RAM: 125911584000000009 aos 704 ciclos. Erros de ponteiro/overflow
recusam antes de escrever ([ADR-0041](../adr/0041-tempo-sistema-guest.md)).

KeGetCurrentIrql consulta o nível explícito guardado no kernel para a única CPU
guest. O padrão é Passive; CLI/IF não alteram IRQL. KRNL v5/IRQL v1 preserva o
nível e migra v1–v4 para Passive. Raise/Lower, IRQ/APC/DPC, DIRQL e layout físico
de KPCR/APIC seguem ausentes. O hello real devolveu Passive com IF=0 e avançou
até a alocação ([ADR-0042](../adr/0042-irql-guest.md)).

NtAllocateVirtualMemory atende reserva+commit privado, base escolhida/ZeroBits=0,
PAGE_READWRITE, em Passive. Base alinhada a 64 KiB, tamanho a 4 KiB, first-fit
após a imagem inteira, frames próprios zerados e saídas validadas antes do efeito.
KRNL v6/VMEM v1 salva regiões e confere Bus/MMU/bitmap no snapshot. O hello recebeu
0x30000 e usou essa RAM. Reserva isolada, commit posterior e liberação faltam;
RW sem execução é política AOT ([ADR-0043](../adr/0043-alocacao-virtual-guest.md)).

A ISA já inclui CMPXCHG/LOCK, SETcc, RDTSC, CLI/HLT, INT3/#BP, SFENCE, CMOVcc,
BSF/BSR e CLC/STC/CMC. Blocos em funções separadas corrigiram o estouro de pilha
host. O controle guest e as exceções pendentes sobrevivem a snapshot, mas ainda
não há IRQ/NMI, IDT/SEH, scheduler geral ou retomada automática dessas exceções.

A validação atual tem **365 testes workspace/doctests** e **162 casos de
Rust gerado em debug/release**, incluindo bootstrap, propriedade e snapshots de threads. O Rust
fixado é 1.98.1. O aceite integrado permanece aberto: o CRT cria a thread de
`main` e ainda faltam serviços, execução completa e framebuffer. A primeira
entrega de memória usou mídia comercial apenas como dado do loader; a BIOS não
foi usada. A EEPROM padrão continua sintética, com bytes/origem explícitos.
