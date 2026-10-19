# Recompilação estática em Rust

Estado: subconjunto inteiro ampliado da Fase 3-AOT, conforme ADRs 0013 e 0014. Não executa jogos
comerciais nem implementa o kernel, renderização ou áudio.

## Pipeline implementado

`Xbe::parse` → validação de layout → descoberta de instruções alcançáveis pelo entry
point, branches e calls diretas → IR → blocos Rust → projeto Cargo → rustc → programa
nativo. A biblioteca é `crates/xbox-recomp`; a ferramenta é `tools/xberecomp`.

O decoder `iced-x86` é escrito em Rust e usado somente na análise. Não há C++, FFI
nova ou blocos `unsafe` neste compilador/runtime. O bus existente mantém seus próprios
limites de `unsafe`, já documentados pelo subsistema de memória.

## Conjunto suportado

- Operações inteiras de 8/16/32 bits: MOV, ADD, SUB, XOR, AND, OR, CMP, TEST,
  INC, DEC, NEG e NOT; NOP de 16/32 bits.
- Registradores completos e partes AL/CL/DL/BL, AH/CH/DH/BH, AX/CX/DX/BX/SP/BP/SI/DI.
  Escrever uma parte preserva os demais bits. `ir::Width` determina leitura,
  escrita, máscara e flags; endereços continuam com 32 bits.
- Memória de 8/16/32 bits com base, índice, escala e deslocamento. MOVZX/MOVSX
  estendem bytes/words; LEA calcula o endereço sem acessar memória ou mudar flags.
- JMP e as 16 condições Jcc curtas/near, CALL/RET de 32 bits, RET com limpeza.
- PUSH/POP de 16/32 bits, incluindo imediatos estendidos com sinal, e LEAVE de 32 bits.
  PUSH ESP lê ESP antes do decremento; POP ESP substitui o incremento; POP [ESP+...]
  usa ESP incrementado no endereço. Escrita com falha não compromete ESP.
- Flags conforme largura efetiva. INC/DEC preservam CF; NOT e instruções de
  transferência preservam flags. Operações lógicas limpam CF/OF e normalizam AF
  indefinido para zero.

LOCK/REP, overrides de segmento, endereçamento de 16 bits, JECXZ/LOOP, CALL/RET de
16 bits, chamadas indiretas, ADC/SBB, shifts, multiplicação/divisão, x87/MMX/SSE e
outras instruções são recusados com PC e diagnóstico.
Jumps para o meio de uma instrução, sobreposição de instruções/seções, alvos fora
de bytes executáveis, código truncado e código gravável também são erros. A análise
tem limite de 65536 instruções alcançáveis, sem promessa de que esse limite sirva a
um título comercial.

## Execução e estado

Cada bloco gerado contém operações Rust explícitas. Um `match` de EIP escolhe o bloco
compilado; não há dispatcher de opcodes nem leitura do XBE para decodificar durante
a execução. O ponto de suspensão é uma fronteira de bloco.

`State` contém os oito registradores, EIP, EFLAGS, contador de instruções concluídas
e indicador de retorno. Não tem ponteiros ou handles do host. O CALL escreve o
endereço de retorno na pilha do guest; o RET o lê. A sentença Rust de chamada não
representa a pilha do jogo, permitindo salvar a continuação com uma call pendente.

`run(state, bus, blocks)` devolve `Exit::Budget`, `Exit::Returned` ou erro recuperável.
Orçamento zero preserva o estado. Falhas de memória deixam EIP na instrução que
falhou; a retomada após erro no meio de bloco ainda não é suportada, e a sessão deve
ser descartada. Pausa/retomada com orçamento acontece só em fronteiras de bloco.

O runtime não consulta relógio de parede. O contador `retired` não alimenta o relógio
virtual como se fosse contagem de ciclos: essa integração precisa de um modelo de
tempo explícito em etapa posterior.

## Loader experimental

Valida que headers/seções não se sobreponham, que as seções caibam na imagem e que
não ocupem a pilha de teste. Mapeia páginas do XBE sobre frames alocados pelo bus,
copia headers e dados e aplica permissões RX/RO/RW. Páginas compartilhadas entre
código e dados graváveis são recusadas. Pilha: 64 KiB em `0x70000000`, com um retorno
sentinela inserido pelo chamador do entry point.

O loader exige ausência de TLS e de imports do kernel. Não substitui o loader da
Fase 4: faltam TLS, resolução de ordinais, seções sob demanda, ambiente XAPI, objetos
e escalonamento. `raw_size > virtual_size` também é recusado nesta versão.

## Evidência e limites

A fixture contém código x86 escrito pelo projeto: soma 1..=10, chama uma função que
soma 7, grava o resultado em `0x12020` e retorna. O resultado esperado é EAX=62,
memória=62 e 37 instruções concluídas. Não é código de jogo nem XBE nxdk.

O teste de integração executa a CLI, compila o código gerado em um projeto Cargo
separado e executa 13 testes nativos em x86_64. Além da primeira prova, cobre
subregistradores, extensões, LEA, frames e casos especiais de ESP, pilha de 16 bits e
read-modify-write de byte na memória.

`xbox-cpu/isa-oracle` executa a ALU física sobre registradores inicializados e captura
suas flags. A comparação cobre 11 operações × 3 larguras × 11 valores de cada
operando × 2 carries: 7986 casos. Outros 3872 casos cobrem os 16 Jcc curtos e near por
relações independentes. Os testes de análise cobrem truncamentos, mutações, fluxo
ambíguo e rejeição de variantes ainda sem suporte. Limites da referência: ADR-0014.

O snapshot mantém o formato anterior: partes de registrador usam os mesmos oito
registradores de 32 bits. Helpers de flags de 32 bits continuam disponíveis para os
fontes gerados pela primeira demonstração; o emissor atual chama helpers por largura.

Essa prova não mede FPS ou ganho de performance, não testa BIOS ou jogos comerciais,
não oferece cache identificado por hash e não integra o módulo ao `obsidian run`.
Os fontes e o XBE de saída são artefatos locais. Antes de distribuir módulos de
jogos, é necessário resolver identificação de imagem/perfil/runtime e o processo de
geração local do usuário.
