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
| 3. Kernel mínimo | memória contígua, modo, filtros, janela MMIO e espera por intervalo (ADR-0050, ADR-0051); 37/69 serviços | funções efetivamente usadas pelo homebrew; erro claro nas demais |
| 4. ISA necessária | nove raízes traduzem 10676 blocos; execução chega ao vídeo | traduzir o código emitido pelo compilador e validar instruções adicionais |
| 5. Execução integrada | executa main do hello; Session::conduzir, --fatias e interrupção pelo host; IRQ/vblank testados com fixtures | carregar módulo, rodar com orçamento, logs e encerramento controlado |
| 6. Framebuffer | apresentado em janela Win32 (ADR-0052), imagem idêntica à RAM guest; só Windows, bytes crus (sem gamma/filtros — provisório, ROADMAP §16.1) | primeira imagem apresentada; controle/áudio ficam para o próximo marco |

As seis entregas são uma divisão de trabalho, não garantia de seis commits ou prazo.
Dependências reais de kernel/ISA só serão conhecidas após compilar o alvo.

A retomada de 2026-10-22 usa triangle/pbkit como segundo alvo. O recorte PRAMDAC
do [ADR-0057](../adr/0057-pramdac-perfil-clock.md) acrescenta NVPLL programável,
perfil público explícito e BUS0 v3/NPLL v1. Bus novo informa 0x00011C01 e
233333324 Hz nominais. Snapshots BUS0 v1/v2 preservam o perfil antigo sem modelo;
não passam a ter PLL válido por migração implícita. O
[ADR-0058](../adr/0058-pfb-perfil-memoria.md) acrescenta CFG0/CFG1 com perfil
Cromwell conforme a arena de 64/128 MiB. BUS0 v4/PFBM v1 preserva os valores;
snapshots BUS0 v1–v3 continuam sem PFB modelado. Readback não implementa geometria
elétrica nem altera o tamanho da RAM quando o guest reprograma o registro.
O [ADR-0059](../adr/0059-consulta-memoria-instancia-gpu.md) atende a consulta
retail 64 MiB de MmClaimGpuInstanceMemory com reserva física real, fronteira
0x83FF0000/padding 0x10000 e KRNL v13/GINS v1. A reserva na primeira consulta é
provisória (P13); resize/devkit recusam. O
[ADR-0060](../adr/0060-pramin-armazenamento-instancia.md) acrescenta armazenamento
PRAMIN independente de 1 MiB (P14), BUS0 v5/PRAM v1 e migração v1–v4 sem modelo.
O [ADR-0061](../adr/0061-pagina-boot-dma.md) reserva a página D3D antes dos frames
XBE: PA zero/VA=0x80000000, RW sem execução, headers em PA=0x1000/RO. BUS0 v6/BTPG
v1 guarda a posse; v1–v5 migram sem reserva e preservam o layout/replay anterior.
TLS e imagem não podem ocupar a nova VA. Triangle preserva RAMHT/contextos,
passa a escrita/WBINVD e chega ao polling DMA_GET sem modelo. Com 12 raízes,
ISR/DPC executam; timeout leva a pb_kill e à falta do ordinal 171 na limpeza.
Próximo: PFIFO/processamento real do pushbuffer e liberação CMEM, depois PGRAPH. P13/P14 e renderização continuam
pendentes. Registros por ADR abaixo são históricos; a passagem de bastão é vigente.

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
representação guest. Os primeiros 17 serviços são RtlInitAnsiString, criação/espera/liberação
de mutant, SHA-1 Init/Update/Final, consulta integral de EEPROM e a família
Init/Enter/Leave/TryEnter de seção crítica, leitura PCI de clock, InterlockedExchange, KeQuerySystemTime, KeGetCurrentIrql e NtAllocateVirtualMemory. Há variantes recusadas explicitamente;
a contagem de serviços não mede compatibilidade.

As seções críticas usam a estrutura pública Xbox de 28 bytes na RAM guest, com
recursão, contagens e identidade guest ativa (pai 1 ou filha 2). O prefixo KTHREAD
da filha é limitado ao bootstrap; não há escalonamento geral. TryEnter retorna FALSE em contenção; Enter
bloqueante e filas de espera recusam sem bloquear o host. O snapshot integral
preserva RAM, CPU, kernel e chamada pendente sem formato novo
([ADR-0037](../adr/0037-secoes-criticas-guest.md)).

O hello real traduz **10676 instruções/blocos** com as raízes `0x19110`, `0x145E0`,
`0x205E0`, `0x20889`, `0x13C70`, `0x18FC0`, `0x18C90`, `0x139B0` e `0x14F14`. Passa pelos
serviços anteriores e cria a filha com pilha de 64 KiB e TLS de 0x114 bytes.
Executa SystemRoutine, inicializa TLS/FLS no guest e entra em StartRoutine e
main_wrapper. A última raiz destrava um case do formatador do CRT. O namespace
guest começa sem D:; a consulta retorna ausência, a criação guarda o diretório
e open/close operam sobre handles reais. Isso não conecta mídia à VFS.
AvSendTVEncoderOption (2) agora responde GET_SETTINGS com padrão/flags da EEPROM
e cabo padrão, resultado 0x00480101 no perfil inicial. VIDEOENABLE guarda o
blanking; AvGetSavedDataAddress retorna NULL no cold boot sem imagem herdada.
MmAllocateContiguousMemoryEx (166) agora entrega **1.228.800 bytes/300 páginas
fisicamente contíguas para 640×480×32**. O hello recebeu VA 0x80050000 e PA
0x50000; o teste real sujou a região e comprovou que o memset guest a zerou.
AvSetDisplayMode (3) agora configura o modo 0x04010101 (NTSC-M 640×480/60),
formato 0x12/pitch 2560 e PA do recurso. O runtime lê as linhas BGRA por DMA,
sem compartilhar a arena. AvSendTVEncoderOption já registra os pedidos
flicker=5 e soften=false. O bloqueio atual é a escrita ao DAC gamma por MMIO,
**0xFD6813C8**, ainda sem mapeamento/modelo. A saída permanece desabilitada;
rampa gamma e apresentação ainda faltam
([ADR-0050](../adr/0050-pedidos-filtros-encoder.md)).

O [ADR-0045](../adr/0045-bootstrap-thread-guest.md) guarda a continuação do pai
sem copiar Clock/retired; término da filha restaura esse contexto. A filha roda
primeiro, sem preempção ou yield. Espera na filha terminada funciona; espera viva,
segunda criação, Sleep e escalonamento geral continuam pendentes. Não há garantia
de progresso do pai se a filha entrar em laço. O prefixo KTHREAD não é ETHREAD
completo. Snapshot AOTS v2/KRNL v7/KMUT v3 acrescenta THRD v1/TCXT v1, migrando
formatos anteriores sem threads. Recursos são retidos; NtClose de thread/mutant
e liberação faltam. KRNL v8 acrescenta SYML v1 para namespace/handles de link;
KRNL v1–v7 migram sem D:. NtClose só fecha links nesta versão. KRNL v9/AVST v1
preserva o encoder; v1–v8 migram com saída desabilitada. A EEPROM continua
autoritativa para o padrão/flags; não há estado duplicado ou tempo novo.

KRNL v10/CMEM v1 acrescenta os recursos contíguos. A janela 0x80000000+PA só
mapeia suas páginas RW, sem abrir acesso geral à RAM/código. O snapshot cruza
tradução linear, posse, bitmap e aliases; v1–v9 migram sem recursos. Há suporte
a limites/alinhamento e proteção RW com WRITECOMBINE opcional sob coerência
síncrona do Bus. Liberação e persistência ainda faltam.

AVST v2 guarda o modo, conferindo geometria, EEPROM e recurso CMEM antes de
publicar o estado restaurado; AVST v1 migra sem modo, preservando enabled.
KRNL continua v10. A visão empresta a RAM do Bus e expõe apenas pixels visíveis
de cada linha, excluindo o padding. Ela permite inspeção com saída desabilitada;
não implica scanout, vblank ou uma imagem já apresentada.

AVST v3 acrescenta os pedidos de flicker (0..7) e soften (0/1), com None no cold
boot e na migração de AVST v1/v2. Valores fora do perfil e Result não nulo
recusam antes do efeito. O estado registra intenção do guest; não aplica um
filtro analógico aos pixels nem altera a visão BGRA da RAM.

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

A validação atual no Linux tem **513 testes workspace/doctests** e **221 casos de
Rust gerado em debug/release**, incluindo PRAMIN/PFB/NVPLL, memória e snapshots. O Rust
fixado é 1.98.1. Hello executa main/apresenta em Win32; o segundo alvo triangle
passa do PLL/PFB e da consulta de instância GPU, mas ainda não renderiza.
Scripts não mudaram; a apresentação Win32 não foi revalidada aqui.
O aceite não cobre GPU 3D ou scheduler geral. A primeira
entrega de memória usou mídia comercial apenas como dado do loader; a BIOS não
foi usada. A EEPROM padrão continua sintética, com bytes/origem explícitos.
