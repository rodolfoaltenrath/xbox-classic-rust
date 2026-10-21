# Recompilação estática em Rust

Estado: subconjunto inteiro ampliado da Fase 3-AOT e kernel HLE parcial, conforme
ADRs 0013–0037. O hello nxdk inicia nativamente com TLS/imports e 12 serviços;
o próximo construtor exige RDMSR. Execução completa, renderização e áudio pendem.

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
RDMSR, x87/MMX/SSE (exceto SFENCE) e
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
continua em blocos; sequências longas passam a ceder mais cedo. AOTC permanece v6
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
Snapshot AOTC v6 guarda perfil, Clock, parada por HLT e breakpoint pendente.
Versões v1–v5 migram sem breakpoint, v1–v4 com CPU ativa; v1–v3 recebem Clock zero.
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
([ADR-0028](../adr/0028-sfence-ordenacao-do-bus.md)). AOTC continua v6.
CMOVcc usa as 16 condições existentes e origem de 16/32 bits capturada antes
da condição, mesmo se falsa. Flags e o destino em condição falsa são preservados;
leituras faltantes continuam falhando. Word preserva a metade alta do registrador.
Não há estado novo ([ADR-0029](../adr/0029-cmovcc.md)).
BSF/BSR calculam índices em origem de 16/32 bits e definem ZF. O perfil preserva
o destino em origem zero e normaliza CF/PF/AF/SF/OF para zero, pois são indefinidos
na referência clássica. IF/DF são preservados; uma leitura faltante não altera
CPU. AOTC continua v6 ([ADR-0030](../adr/0030-bsf-bsr.md)).
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

`Session` possui CPU, bus e dispatcher HLE. O snapshot integral `AOTS` v1
inclui `AOTC` v6, memória e estado `KRNL` v2 / `KMUT` v2 / `EEPR` v1, com handles, propriedade e
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

## Evidência e limites

A fixture contém código x86 escrito pelo projeto: soma 1..=10, chama uma função que
soma 7, grava o resultado em `0x12020` e retorna. O resultado esperado é EAX=62,
memória=62 e 37 instruções concluídas. Não é código de jogo nem XBE nxdk.

O teste de integração executa a CLI, compila o código gerado em um projeto Cargo
separado e executa 125 testes nativos em x86_64. Além da primeira prova, cobre
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
