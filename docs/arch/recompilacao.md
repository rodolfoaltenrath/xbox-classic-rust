# Recompilação estática em Rust

Estado: subconjunto inteiro ampliado da Fase 3-AOT e kernel HLE parcial, conforme
ADRs 0013–0061. O hello nxdk inicia nativamente com TLS/imports, 37 serviços
e perfil de CPU/PCI guest; executa o bootstrap de uma filha e configura o modo
de vídeo sobre memória física contígua. Main e apresentação Win32 funcionam com
bytes crus, sem gamma/filtros. Há infraestrutura de eventos/vblank/ISR/DPC;
o segundo alvo triangle passa do NVPLL/PFB e da consulta de instância GPU
(ordinal 168, retail 64 MiB, reserva física P13). KRNL v13/GINS v1 guarda o
bootstrap e migra v1–v12 sem estado ativo. PRAMIN preserva tabelas/contextos
no perfil independente P14, BUS0 v5/PRAM v1 e migração v1–v4 sem modelo PRAMIN.
AOT novo reserva PA zero para D3D antes da imagem, mantendo headers em PA=0x1000/RO.
BUS0 v6/BTPG v1 persiste a posse; v1–v5 preservam o layout anterior sem reserva.
Triangle passa escrita/WBINVD e chega ao polling DMA_GET sem modelo.
GPU 3D, scheduler geral e áudio permanecem pendentes.

## Pipeline implementado

`Xbe::parse` → validação de layout → descoberta de instruções alcançáveis pelo entry
point, branches, calls e raízes indiretas descobertas/declaradas → IR → blocos Rust → projeto Cargo → rustc → programa
nativo. A biblioteca é `crates/xbox-recomp`; a ferramenta é `tools/xberecomp`.

O decoder `iced-x86` é escrito em Rust e usado somente na análise. Não há C++, FFI
nova ou blocos `unsafe` neste compilador/runtime. O bus existente mantém seus próprios
limites de `unsafe`, já documentados pelo subsistema de memória.

## Conjunto suportado

- Operações inteiras de 8/16/32 bits: MOV, ADD, ADC, SUB, SBB, XOR, AND, OR, CMP, TEST,
  INC, DEC, NEG e NOT; NOP de 16/32 bits.
- Registradores completos e partes AL/CL/DL/BL, AH/CH/DH/BH, AX/CX/DX/BX/SP/BP/SI/DI.
  Escrever uma parte preserva os demais bits. `ir::Width` determina leitura,
  escrita, máscara e flags; endereços gerais continuam com 32 bits, com address16
  disponível especificamente nas strings e branches por contador.
- Memória de 8/16/32 bits com base, índice, escala e deslocamento. MOVZX/MOVSX
  estendem bytes/words; LEA calcula o endereço sem acessar memória ou mudar flags.
- JMP e as 16 condições Jcc curtas/near, CALL/RET de 32 bits, RET com limpeza.
- CALL/JMP near por registrador ou memória de 32 bits. Destinos são descobertos
  por constantes locais, ponteiros de seções imutáveis ou entradas adicionais
  declaradas com `--entry`. CALL/JMP e RET validam destinos antes de comprometer
  CPU/pilha; apenas RET aceita a sentinela de retorno ao host (ADR-0019).
- PUSH/POP de 16/32 bits, incluindo imediatos estendidos com sinal, e LEAVE de 32 bits.
  PUSH ESP lê ESP antes do decremento; POP ESP substitui o incremento; POP [ESP+...]
  usa ESP incrementado no endereço. Escrita com falha não compromete ESP.
- Flags conforme largura efetiva. INC/DEC preservam CF; NOT e instruções de
  transferência preservam flags. Operações lógicas limpam CF/OF e normalizam AF
  indefinido para zero.
- ADC/SBB consomem CF antes da operação. CF é calculado com operandos estendidos
  para 64 bits, sem perder carry/borrow quando `src + CF` excede a largura; OF e AF
  usam os operandos originais e o resultado final. FLAGS só muda após escrita bem
  sucedida. Pausa e snapshot preservam CF entre blocos de aritmética multipalavra.
- SHL/SHR/SAR de 8/16/32 bits com contagem um implícita, imediato ou CL. A contagem
  é mascarada por 31; SAR estende o sinal antes de deslocar. CL é capturado antes
  da escrita, incluindo quando CL/CX/ECX é o destino. Contagem efetiva zero preserva
  valor e FLAGS, mantendo a verificação de leitura/escrita da memória. AF indefinido
  é zero; OF com contagem maior que um e CF lógico com contagem maior ou igual à
  largura também são normalizados para zero (ADR-0015).
- ROL/ROR/RCL/RCR de 8/16/32 bits. O emissor rotaciona um anel em `u64`, com um bit
  extra para CF em RCL/RCR. SF/ZF/AF/PF são preservados; OF indefinido é zero. Uma
  volta completa em ROL/ROR byte/word preserva o valor mas atualiza CF.
- SHLD/SHRD de 16/32 bits com fonte em registrador e contagem imediata ou CL.
  Contagem efetiva maior que a largura retorna `UndefinedDoubleShift` com PC antes
  de qualquer acesso à memória; o resultado inteiro é indefinido na ISA. Zero
  preserva FLAGS e valor, mantendo a validação de memória; count=16 em word
  transfere a fonte inteira. Aliases com contagem/fonte são capturados antes da
  escrita (ADR-0016).
- MUL/IMUL implícitos e DIV/IDIV de 8/16/32 bits, com AX/DX:AX/EDX:EAX e partes
  preservadas. IMUL de dois/três operandos cobre 16/32 bits, com fonte de memória e
  imediato com sinal. Produtos são calculados em 64 bits, divisões em i128; o
  emissor compromete resultados só depois de validar a aritmética. CF/OF de
  produto são definidos por truncamento; outras flags aritméticas indefinidas
  são zero. Divisão bem sucedida normaliza todas as flags aritméticas para zero.
- CBW/CWDE/CWD/CDQ, sem alterar flags. Erros `Divide` com PC e motivo distinguem
  divisor zero de quociente fora da faixa. Não entregam #DE via IDT/kernel; a CPU
  permanece na instrução, sem contabilizá-la como concluída (ADR-0017).

- JCXZ/JECXZ, LOOP/LOOPE/LOOPNE com contador CX/ECX, sem alterar flags.
- MOVS/STOS/LODS/CMPS/SCAS de 8/16/32 bits; CLD/STD e DF. REP para cópia,
  preenchimento e carga; REPE/REPNE para comparações. Endereços de 16/32 bits
  determinam índices e contador, preservando metades altas no modo de 16 bits.
  Cada elemento repetido é uma passagem limitada de um bloco próprio (ADR-0018).

LOCK fora de CMPXCHG, overrides de segmento fora do perfil FS, endereçamento geral
de 16 bits, CALL/RET de 16 bits, CALL/JMP far, INS/OUTS, REPNE fora de CMPS/SCAS,
WRMSR, MSRs fora do perfil, x87/MMX/SSE (exceto SFENCE) e
outras instruções são recusados com PC e diagnóstico.
Jumps para o meio de uma instrução, sobreposição de instruções/seções, alvos fora
de bytes executáveis, código truncado e código gravável também são erros. A análise
tem limite de 65536 instruções alcançáveis e 256 rodadas de descoberta indireta,
sem promessa de que esses limites sirvam a
um título comercial.

## Execução e estado

Cada bloco gerado contém operações Rust explícitas. Um `match` de EIP escolhe o bloco
compilado; não há dispatcher de opcodes nem leitura do XBE para decodificar durante
a execução. O ponto de suspensão é uma fronteira de bloco, incluindo cada elemento
de uma instrução REP, isolada de seu setup e de sua continuação.

Com CALL/JMP indireto, todos os inícios de instrução decodificados viram blocos
para admitir destinos dinâmicos internos. A granularidade do orçamento passa a
uma instrução por bloco nesse programa, ou um elemento por passagem de REP.
Sem CALL/JMP indireto, o agrupamento por fluxo é preservado até o limite final
descrito abaixo. A inferência não
varre dados, não cruza blocos/calls e não presume valores da memória gravável.

Desde o ADR-0031, o particionamento final limita blocos a 32 instruções, depois
da inferência de destinos. Cada bloco é uma função não inlineada; o dispatcher
seleciona um ponteiro de função e faz uma chamada, que retorna antes da próxima.
Isso corrige o estouro de pilha host do emissor monolítico. CALL/RET guest
continuam explícitos em EIP/ESP/RAM, sem recursão de chamadas nativas. Orçamento
continua em blocos; sequências longas passam a ceder mais cedo. Essa entrega manteve AOTC v6
([ADR-0031](../adr/0031-blocos-aot-pilha-host.md)).

`State` contém os oito registradores, EIP, EFLAGS, contador de instruções concluídas
e indicador de retorno. Guarda também FS, o Clock único da sessão AOT atual e
o PC de HLT quando a CPU está suspensa ou de INT3 quando #BP está pendente.
Durante comparação REP, guarda PC e flags originais
para restaurá-las em caso de falta. Não tem ponteiros ou handles do host. O CALL escreve o
endereço de retorno na pilha do guest; o RET o lê. A sentença Rust de chamada não
representa a pilha do jogo, permitindo salvar a continuação com uma call pendente.

`run(state, bus, blocks)` devolve `Exit::Budget`, `Exit::Returned`, `Exit::KernelCall`,
`Exit::Halted`, `Exit::Breakpoint` ou erro recuperável. RDTSC lê o Clock guest; o perfil AOT cobra um ciclo virtual
por instrução/elemento REP concluído, sem alegar precisão de ciclos físicos.
Snapshot AOTC v7 guarda perfis de CPU/tempo, Clock, HLT e breakpoint pendente.
Versões v1–v6 recebem o perfil de CPU inicial, v1–v5 migram sem breakpoint, v1–v4 com CPU ativa; v1–v3 recebem Clock zero.
O agendador do core ainda precisa ser integrado a esse relógio único
([ADR-0025](../adr/0025-relogio-aot-rdtsc.md)).
CLI limpa IF do guest no perfil privilegiado. HLT preserva flags, avança EIP e
entrega a parada ao host. Polls não executam a continuação nem avançam tempo;
não há entrega de interrupções/retomada automática ainda
([ADR-0026](../adr/0026-controle-cli-hlt.md)).
INT3 conclui com EIP seguinte e #BP pendente. O host recebe diagnóstico, sem
entrega automática via IDT/SEH. A descoberta termina esse caminho excepcional,
mantendo destinos de branches e raízes explícitas. Polls não executam código nem
acrescentam tempo ([ADR-0027](../adr/0027-int3-breakpoint-aot.md)).
SFENCE chama a barreira do bus. Stores síncronos na arena exclusiva já têm a
ordenação exigida; não há fila a publicar nem fence de GPU nesta etapa. Introduzir
observadores concorrentes ou escrita diferida exige ampliar esse contrato
([ADR-0028](../adr/0028-sfence-ordenacao-do-bus.md)). Essa entrega manteve AOTC v6.
CMOVcc usa as 16 condições existentes e origem de 16/32 bits capturada antes
da condição, mesmo se falsa. Flags e o destino em condição falsa são preservados;
leituras faltantes continuam falhando. Word preserva a metade alta do registrador.
Não há estado novo ([ADR-0029](../adr/0029-cmovcc.md)).
BSF/BSR calculam índices em origem de 16/32 bits e definem ZF. O perfil preserva
o destino em origem zero e normaliza CF/PF/AF/SF/OF para zero, pois são indefinidos
na referência clássica. IF/DF são preservados; uma leitura faltante não altera
CPU. Essa entrega manteve AOTC v6 ([ADR-0030](../adr/0030-bsf-bsr.md)).
Orçamento zero preserva o estado e informa uma parada já existente. Falhas de memória deixam EIP na instrução que
falhou. REP pode retomar no elemento pendente depois que o host resolver a falta;
não há entrega automática ao kernel. Nas outras instruções, a retomada após erro
no meio de bloco ainda não é suportada, e a sessão deve ser descartada.
Cada elemento REP consome uma unidade de orçamento; `retired` conta a instrução
apenas na conclusão. `UnknownTarget` informa PC e destino sem desempilhar nem
empilhar; o runtime também confere permissão de execução do destino conhecido.
Zero repetições não acessa memória. O bus valida todas as
páginas de uma escrita antes de alterar RAM, evitando store parcial na falta.

O runtime não consulta relógio de parede. Instruções/elementos REP concluídos
avançam o Clock pelo custo sintético do perfil, independentemente de `retired`.
Integrar o scheduler do core ao mesmo Clock continua pendente.

RDMSR tem operação própria e consulta ECX antes de escrever EDX:EAX. O perfil
`CpuProfile::XboxRetailV1` (ID 1) está em State e no AOTC v7: 0x10 lê o mesmo
Clock de RDTSC; 0x2A lê a configuração nominal 0x01040000 (bus 133⅓ MHz × 5,5).
Demais campos desse registro são sintéticos, sem efeitos de APIC/energia/erros.
Outros índices dão UnsupportedMsr com PC/índice, sem efeitos ou cobrança do passo;
WRMSR/CPUID/CPL/CR4/#GP por IDT seguem pendentes. v1–v6 migram para o perfil 1;
o perfil de tempo continua 1. Não há contador MSR próprio ou RDMSR do host
([ADR-0038](../adr/0038-rdmsr-perfil-cpu-guest.md)).

## Loader experimental

Desde o ADR-0020, a sessão AOT reutiliza `xbox-kernel::loader::Image` em `Mode::Aot`.
O modo `Inspect` separado aceita permissões da imagem real e mapeia alias físico
para o comando `obsidian check-memory`; nunca é usado para rodar código AOT.

Valida que headers/seções não se sobreponham, que as seções caibam na imagem e que
não ocupem a pilha de teste. Mapeia páginas do XBE sobre frames alocados pelo bus,
copia headers e dados e aplica permissões RX/RO/RW. Páginas compartilhadas entre
código e dados graváveis são recusadas. Pilha: 64 KiB em `0x70000000`, com um retorno
sentinela inserido pelo chamador do entry point.

O perfil inicial prepara TLS nxdk e resolve imports de funções para trampolins
guest RX. FS tem base guest explícita, LEA usa só offset e chamadas HLE devolvem
`Exit::KernelCall` com ABI stdcall/cdecl/fastcall. Dados exportados e dispatcher
foram acrescentados nos ADRs 0022/0023. O ADR-0032 acrescenta criação de mutexes
guest sem nome; seções sob demanda, ambiente XAPI e escalonamento continuam
pendentes. `raw_size > virtual_size` é recusado. Limites e fontes no
[ADR-0021](../adr/0021-tls-imports-aot.md) e [guia homebrew](../dev/homebrew.md).

`Session` possui CPU, bus, dispatcher HLE e continuação do pai. O snapshot integral `AOTS` v2
inclui `AOTC` v7, memória e estado `KRNL` v10 / `KMUT` v3 / `EEPR` v1 / `PCIC` v1 /
`STIM` v1 / `IRQL` v1 / `VMEM` v1 / `THRD` v1 / `TCXT` v1 / `SYML` v1 / `AVST` v3 / `CMEM` v1, com regiões virtuais, handles, propriedade, prioridade lógica e
contador de alocação determinísticos. Salvar CPU/RAM isoladamente não preserva
objetos. O retorno HLE é capturado e validado antes de executar efeitos; depois
do serviço, o runtime aplica essa continuação sem reler a pilha já modificada.
Detalhes e limites: [ADR-0032](../adr/0032-mutants-guest-snapshot-sessao.md).

SHA-1 Init/Update/Final usa um contexto **opaco HLE v1** de 116 bytes na RAM,
com marcador e estado explícito. Cópia de bytes funciona sem registro host, e
o snapshot da memória preserva hashes parciais. O formato não pretende reproduzir
campos privados de um kernel físico; Final preserva a origem por política local.
Versões de snapshot continuam as mesmas. Limites, fontes e testes independentes:
[ADR-0033](../adr/0033-sha1-contexto-guest.md).

O dispatcher também possui imagem autoritativa de EEPROM com bytes/origem
explícitos. `ExQueryNonVolatileSetting` atende somente consulta integral;
o padrão é sintético NTSC-M/português, gerado pelo projeto. A consulta retorna
cópia em RAM, validando todas as saídas antes de publicar. `KRNL` v2 acrescenta
`EEPR` v1 e migra KRNL v1 para a imagem padrão. Persistência/escrita e índices
individuais seguem pendentes. [ADR-0034](../adr/0034-consulta-eeprom-sessao.md).

Mutants admitem aquisição/recursão/liberação imediatas, sem scheduler; espera
bloqueante/APCs/timeout e `PreviousCount` não nulo são variantes recusadas
([ADR-0035](../adr/0035-espera-liberacao-mutant.md)). As seções críticas
Init/Enter/Leave/TryEnter operam sobre os 28 bytes públicos Xbox em RAM guest:
evento embutido, LockCount, RecursionCount e OwningThread. O proprietário 1 é
identidade sintética do perfil, sem KTHREAD físico. TryEnter de outro proprietário
retorna FALSE; Enter bloqueante, esperadores e overflow recusam sem efeitos.
Toda a faixa é validada como RAM antes de escrever. Seu estado não precisa de
tabela host nem seção nova no snapshot. Contrato e limites:
[ADR-0037](../adr/0037-secoes-criticas-guest.md).

O dispatcher possui também `PciConfig`, perfil somente leitura do registro PLL
0:0:3 (slot NT 0x60), offsets 0x6C..0x70 e subfaixas. CPU/MSRs e PCI compartilham
as constantes de `RetailClockV1` no core, com frequência derivada conferida contra
Clock. Áreas desconhecidas/escritas recusam; saída passa por validação integral
como RAM antes do commit. Não há acesso a portas host. KRNL v3 acrescenta PCIC v1
com perfil ID 1; v1/v2 migram explicitamente, preservando estado anterior.
Portas/dispositivos futuros devem usar essa mesma autoridade de configuração
([ADR-0039](../adr/0039-configuracao-pci-clock-guest.md)).

InterlockedExchange (54) captura Target/Value de ECX/EDX e valida RAM R/W antes
de ler/trocar LONG. EAX recebe os bits anteriores, sem alterar flags. O serviço
não cede no meio do RMW e usa a ordenação do Bus síncrono/exclusivo; não permite
observadores host concorrentes ou MMIO. Nenhum estado host novo ou formato de
snapshot é necessário. A continuação HLE capturada permite Target sobre o retorno;
fastcall limpa somente as palavras efetivamente na pilha
([ADR-0040](../adr/0040-interlocked-exchange-guest.md)).

KeQuerySystemTime recebe o Clock autoritativo da sessão por
Dispatcher::atender_com_clock. SystemTimeReference guarda a data UTC do power-on:
padrão sintético 2000-01-01, configurável sem relógio host. A consulta soma a essa
referência os ciclos convertidos a 100 ns com u128 e limite de LARGE_INTEGER.
Valida a saída inteira como RAM gravável antes dos oito bytes little-endian;
contexto ausente, overflow, MMIO e ponteiros inválidos recusam explicitamente.
KRNL v4/STIM v1 salva somente a referência; Clock continua em AOTC/CLK0, sem
contador duplicado. KRNL v1–v3 migram para o padrão. Formato dessa entrega:
AOTS v1/AOTC v7/KRNL v4/KMUT v2/EEPR v1/PCIC v1/STIM v1
([ADR-0041](../adr/0041-tempo-sistema-guest.md)).

O dispatcher guarda também Irql, prioridade lógica da única CPU guest,
inicialmente Passive. KeGetCurrentIrql lê esse estado, com KIRQL estendido a u32
em EAX, sem argumentos ou efeitos sobre flags/Clock/RAM. CLI modifica apenas IF,
sem alterar IRQL. Apc/Dispatch são contextos iniciais representáveis; DIRQL e
transições Raise/Lower ainda não têm modelo. Não há IRQ/APC/DPC, KPCR/APIC físico
ou cópia de prioridade por thread. KRNL v5/IRQL v1 salva o nível e migra KRNL
v1–v4 para Passive independentemente de IF e do contexto de destino.
Formato dessa entrega: AOTS v1/AOTC v7/KRNL v5/KMUT v2/EEPR v1/PCIC v1/STIM v1/IRQL v1
([ADR-0042](../adr/0042-irql-guest.md)).

NtAllocateVirtualMemory atende reserva+commit privado/RW, base escolhida pelo
kernel e ZeroBits=0, em Passive. O registro lógico guarda intervalo XBE completo e
regiões committed, com bases 64 KiB/tamanhos 4 KiB e first-fit abaixo da pilha.
MMU/frames são planejados em cópias independentes; frames são zerados antes de
publicar mapeamentos e parâmetros. Sobreposição física entre saídas é recusada.
Outras variantes e NtFreeVirtualMemory faltam; RW sem execução é política AOT.
KRNL v6/VMEM v1 salva regiões sem duplicar os frames do Bus. Session confere
headers, posse de RAM, permissões e ausência de aliases privados indevidos.
KRNL v1–v5 migra sem regiões, recuperando o intervalo do XBE correspondente na
carga de Session. FRM0 v1 valida bitmap/contagem/padding antes do commit.
Formato nessa entrega: AOTS v1/AOTC v7/KRNL v6/KMUT v2/EEPR v1/PCIC v1/STIM v1/IRQL v1/VMEM v1

O [ADR-0045](../adr/0045-bootstrap-thread-guest.md) acrescenta uma filha guest
cooperativa: o pai fica estacionado, a filha tem pilha/TLS/TIB próprios e entra
em SystemRoutine(StartRoutine, Context). KeGetCurrentThread expõe só o prefixo
KTHREAD requerido pelo bootstrap. O término restaura registradores/flags/FS do
pai; Clock/retired/perfil de CPU/IRQL permanecem únicos. Sem threads host ou Bus
compartilhado. Espera imediata em filha terminada é suportada; preempção, yield,
espera bloqueante e múltiplas filhas não. AOTS v2/KRNL v7/KMUT v3 acrescentam
THRD/TCXT v1 e migram versões anteriores sem filha/pai suspenso.

O [ADR-0046](../adr/0046-namespace-drive-d.md) acrescenta um namespace mínimo de
D: para diretório de CdRom0, independente de mounts da VFS. Open/create/close de
links usam estado e handles guest, sem acesso ao host. KRNL v8/SYML v1 salva
destino e handles, valida colisões entre tipos e migra v1–v7 para namespace vazio.
NtClose de mutant/thread e ligação do namespace aos serviços de arquivo faltam.

O [ADR-0047](../adr/0047-encoder-video-cold-boot.md) acrescenta o perfil de encoder
cabo padrão/cold boot: consulta deriva os settings da EEPROM e blanking guarda
enabled. Sem framebuffer herdado, AvGetSavedDataAddress retorna NULL. Não há
GPU, apresentação, scanout ou vblank por essa entrega. KRNL v9/AVST v1 salva
perfil/estado; v1–v8 migram com saída desabilitada. Nenhum Clock ou configuração
EEPROM é duplicado. Memória fisicamente contígua e programação do modo faltam.

O [ADR-0048](../adr/0048-memoria-fisica-contigua.md) acrescenta o alocador físico
contíguo, separado de VMEM e usando o mesmo bitmap. Mapeia apenas páginas
possuídas em 0x80000000|PA, RW sem execução, mantendo RAM virtual/física/DMA
coerente. Limites físicos inclusivos, alinhamento e espaço livre são conferidos
antes do commit planejado em cópias de MMU/bitmap. KRNL v10/CMEM v1 registra os
recursos; v1–v9 migram sem recursos. Restauração rejeita posse/tradução/proteção
inconsistente, aliases externos e páginas órfãs na janela, sem marcar A/D.
Liberação/persistência e programação/apresentação do framebuffer ainda faltam.
([ADR-0043](../adr/0043-alocacao-virtual-guest.md)).

## Evidência e limites

O ADR-0044 acrescenta NOP multibyte como a IR Nop já existente, sem ler memória
ou calcular o operando ModR/M. As entradas SystemRoutine/StartRoutine da thread
passam a ser raízes explícitas adicionais: seis raízes traduzem 9454 blocos.
Isso prepara a tradução do startup; a execução ainda recusa PsCreateSystemThreadEx.
[Contrato e limites](../adr/0044-nop-multibyte-raizes-thread.md).

A fixture contém código x86 escrito pelo projeto: soma 1..=10, chama uma função que
soma 7, grava o resultado em `0x12020` e retorna. O resultado esperado é EAX=62,
memória=62 e 37 instruções concluídas. Não é código de jogo nem XBE nxdk.

O teste de integração executa a CLI, compila o código gerado em um projeto Cargo
separado e executa 153 testes nativos em x86_64. Além da primeira prova, cobre
subregistradores, extensões, LEA, frames e casos especiais de ESP, pilha de 16 bits e
read-modify-write de byte na memória.

`xbox-cpu/isa-oracle` executa a ALU física sobre registradores inicializados e captura
suas flags. A comparação cobre 13 operações × 3 larguras × 11 valores de cada
operando × 2 carries: 9438 casos. ADC/SBB de byte também são verificados nas 262144
combinações de operação, operandos e carry. Outros 3872 casos cobrem os 16 Jcc curtos
e near por relações independentes. Testes estruturais verificam AH, imediatos com
sinal, memória, falha de escrita e retomada com carry entre blocos. Os testes de
análise cobrem truncamentos, mutações, fluxo ambíguo e rejeição de variantes ainda
sem suporte. Os shifts acrescentam 52470 casos de CL/imediato/um implícito e 393216
casos exaustivos de byte contra a CPU física; só flags definidas são comparadas,
enquanto a normalização das demais é verificada separadamente. Limites da referência:
ADRs 0014 e 0015. Rotações e shifts duplos acrescentam mais 791764 comparações
físicas (ADR-0016). Contagens de word com resultado indefinido são excluídas do
oráculo e verificadas separadamente como erros recuperáveis; não há alegação de
compatibilidade com o resultado indefinido de um Pentium III.

Produtos/divisões acrescentam 483916 comparações físicas e 1000580 diagnósticos
classificados (ADR-0017). A referência divide em 64 bits para provar segurança e
só então executa a divisão estreita exata; não provoca #DE no host. IMUL truncado
é comparado com o produto físico implícito equivalente. Os testes verificam
separadamente flags indefinidas, fontes read-only, aliases, erros e snapshot.

Strings acrescentam 3456 comparações físicas e testes de address16, faltas,
sobreposição e snapshots parciais (ADR-0018). O oráculo usa buffers host com limites
validados; o assembly limpa DF antes de devolver o controle ao Rust.

Controle indireto acrescenta nove testes nativos de callbacks, tabelas, aliases de
ESP, faltas, retornos, orçamento, recursão e snapshot (ADR-0019). As verificações
de semântica usam resultados esperados independentes; não há execução de endereços
guest na CPU host nem novo assembly para essas instruções.

O snapshot `AOTC` v2 acrescenta a continuação de comparação REP e lê v1 com esse
campo vazio. Partes de registrador usam os mesmos oito registradores de 32 bits.
Helpers de flags de 32 bits continuam disponíveis para os
fontes gerados pela primeira demonstração; o emissor atual chama helpers por largura.

Essa prova não mede FPS ou ganho de performance, não testa BIOS ou jogos comerciais,
não oferece cache identificado por hash e não integra o módulo ao `obsidian run`.
Os fontes e o XBE de saída são artefatos locais. Antes de distribuir módulos de
jogos, é necessário resolver identificação de imagem/perfil/runtime e o processo de
geração local do usuário.

O [ADR-0049](../adr/0049-modo-video-framebuffer.md) acrescenta modo HLE NTSC-M
640×480/60 A8R8G8B8 e leitura por DMA do framebuffer CMEM. AvSetDisplayMode valida
EEPROM, geometria e posse antes de publicar DisplayMode; Step=0 encerra a
transação HLE. O descritor não permite construção externa sem validação.
Dispatcher::framebuffer empresta a RAM e expõe linhas BGRA sem padding;
o empréstimo impede retomar a CPU com Bus mutável enquanto usado. Não há
compartilhamento da arena ou pixels gerados pelo host. AVST v2 persiste o modo,
migra v1 sem modo e cruza EEPROM/CMEM; KRNL continua v10. Scanout, vblank e
apresentação ficam fora dessa entrega. O hello para no filtro de cintilação
(opção 11 do encoder), com saída ainda desabilitada.

O [ADR-0050](../adr/0050-pedidos-filtros-encoder.md) registra os pedidos de
flicker (11, 0..7) e soften (14, 0/1) no VideoState. Valida base/Param/Result
antes do efeito, preservando modo/blanking e a RAM original. São configurações
lógicas, sem simulação dos filtros analógicos. AVST v3 acrescenta dois bytes
opcionais, migrando v1/v2 sem pedidos; KRNL permanece v10. O próximo bloqueio é
o DAC gamma por MMIO, ainda sem mapeamento/modelo. A visão do framebuffer
continua emprestando os pixels guest sem transformá-los.
