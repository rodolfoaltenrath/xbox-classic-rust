# ADR-0072 — DIRQL, máscara IF e preempção entre instruções AOT

Data: 2026-10-23. Estado: aceito. PLANO 3.4; encerra P10/P11 para novas sessões.

## Evidência e recorte

O pbkit fixado nos ADRs anteriores conecta GPU_IRQ=3 via HalGetInterruptVector e
KeInitializeInterrupt; o dispatcher e a sessão já entregam seu ISR/DPC real.
Para retirar P10/P11, usar as seguintes fontes públicas, sem copiar implementação:

- [Intel SDM Vol. 2B, 253667-060US, setembro/2016, STI, pp. 4-645–646](https://www.intel.com/content/dam/www/public/us/en/documents/manuals/64-ia-32-architectures-software-developer-vol-2b-manual.pdf):
  IF habilita INTR; STI que encontra IF limpa inibe reconhecimento até completar
  a instrução seguinte. STI com IF já ligada não reinicia essa janela; CLI a cancela.
  Este perfil permanece privilegiado, sem VME/PVI/CPL dinâmicos.
- Cxbx-Reloaded fixado em **585c49a50af1255ab155099e06f24505f9c5a800**:
  [CxbxKrnl.h](https://github.com/Cxbx-Reloaded/Cxbx-Reloaded/blob/585c49a50af1255ab155099e06f24505f9c5a800/src/core/kernel/init/CxbxKrnl.h)
  documenta IRQ_BASE=0x30 e conversão de prioridade 27−IRQ;
  [HalGetInterruptVector](https://github.com/Cxbx-Reloaded/Cxbx-Reloaded/blob/585c49a50af1255ab155099e06f24505f9c5a800/src/core/kernel/exports/EmuKrnlHal.cpp)
  consome essas conversões. Isso fundamenta o perfil público existente: GPU IRQ3
  tem DIRQL24. Não é uma medição física do console.
- [EmuKrnl.h](https://github.com/Cxbx-Reloaded/Cxbx-Reloaded/blob/585c49a50af1255ab155099e06f24505f9c5a800/src/core/kernel/exports/EmuKrnl.h)
  inicializa habilitada a entrega global emulada;
  [EmuX86.cpp](https://github.com/Cxbx-Reloaded/Cxbx-Reloaded/blob/585c49a50af1255ab155099e06f24505f9c5a800/src/devices/x86/EmuX86.cpp)
  liga CLI/STI a esse estado. Adotar IF habilitada no bootstrap **HLE** de Session,
  como já ocorria na filha, explicitamente neste novo perfil. State::new isolado
  conserva flags 2; não afirmar que isso descreve o reset elétrico de uma CPU.
- [types.h](https://github.com/Cxbx-Reloaded/Cxbx-Reloaded/blob/585c49a50af1255ab155099e06f24505f9c5a800/src/core/kernel/common/types.h)
  permite representar níveis até HIGH_LEVEL=31. O KIRQL é byte no nxdk.

- [xemu target/i386/tcg/translate.c](https://github.com/xemu-project/xemu/blob/03b988c9d76553b33ed712ba00822ffdf49f0e7d/target/i386/tcg/translate.c),
  mesmo pin gráfico: do_gen_rep desabilita o loop interno sob HF_INHIBIT_IRQ;
  gen_eob remove a inibição ao sair após essa iteração. O perfil público permite
  reconhecer IRQ **depois da primeira iteração REP**, mesmo com ECX restante.

Revisão de método: a primeira versão local segurava a shadow até REP terminar,
por interpretar apenas a descrição geral de STI. Conferir o consumidor REP do
pin acima revelou que isso era uma suposição indevida. A implementação foi
corrigida antes do commit: o teste agora observa ECX=2 no ISR após uma de três
iterações e verifica os bytes finais/replay, em vez de confirmar aquela suposição.
Nenhuma implementação do projeto de referência foi copiada.

As cópias locais e o pin estão em target/interrupt-evidence (não versionado).
As URLs fixadas acima, os consumidores e testes versionados são a referência.

## Decisão

Irql representa 0–31, com Passive/APC/Dispatch e níveis de dispositivo explícitos.
O ISR MaskedV1 só é entregue com IF ligada, shadow zero e prioridade maior que
a atual. A prioridade vem da KINTERRUPT guest; valores fora de DIRQL recusam.
Durante o callback, IF é limpa e o kernel entra no DIRQL registrado; o retorno
restaura flags/contexto/prioridade anteriores. DPC roda em Dispatch com IF
habilitada, somente se o contexto interrompido permite a entrega. A preempção
por quantum de outra thread também respeita IF/shadow no perfil novo.

STI tem IR própria, validação de prefixos e shadow serializável. O primeiro STI
com IF limpa cria a janela; ela termina após o próximo passo bem sucedido.
Na REP, a primeira iteração consome a shadow mesmo com ECX restante. CLI torna a máscara efetiva antes
do próximo ponto de reconhecimento. Nenhuma instrução RMW é partida.

Session calcula um poll efêmero a partir do Clock/vblanks, IRQ conectada e IRQL.
O código emitido verifica linha/deadline entre instruções e elementos REP. Se
precisar entregar, retorna Budget com EIP exato, sem repetir store ou cobrar
ciclos. O próximo passo da sessão sincroniza vblank e entrega o callback.
Assim um run de grande orçamento cede no evento, sem esperar terminar a fatia.

Blocos de até 32 instruções permanecem limitados. O dispatcher aceita seus
inícios internos e o corpo pula o prefixo anterior ao EIP de entrada. Isso
permite retornar do ISR/snapshot no meio do bloco sem duplicar funções/sufixos
nem executar novamente efeitos anteriores. Guardas de targets dinâmicos e
recusa de endereço no meio de instrução permanecem.

## Persistência e compatibilidade

AOTC v10 acrescenta InterruptProfile e shadow; v1–v9 migram para LegacyBoundary,
shadow zero, preservando IF serializada. Regravar não habilita entrega nova.
LegacyBoundary conserva o ISR com IRQL inalterado e entrega só em fronteiras,
independente de IF, como antes. TickPublication continua independente.

IRQL v2 guarda 0–31; v1 conserva validação restrita a 0–2. TCTX v2 guarda a
shadow de thread inativa; v1 migra para zero. Flags desligadas/shadow ativa,
shadow >1 e shadow fora de MaskedV1 recusam. Poll não é estado oculto: existe
somente durante run e é rederivado antes da execução após restore. Não tem
relógio host, fila não serializada ou tarefa nativa concorrente.

## Limites / P20

Apenas INTR da GPU conectada neste perfil HLE, ISR/DPC ainda atômicos para o host,
sem IRQ aninhada durante os callbacks, PIC/APIC geral, NMI ou retomada de HLT.
KfRaiseIrql/KfLowerIrql continuam recusados: representar DIRQL não implementa
APIs ausentes. ISR usa a ABI callback do kernel, não um IRET/IDT de kernel guest.
A revisão também tornou explícito P19: Events/KEVT conserva um único registro
de waiter. MaskedV1 recusa uma segunda chamada NtWait enquanto há outra
continuação nesse trampolim, sem sobrescrever o waiter, consumir pulso ou trocar
contextos. A recusa contém serviço/ordinal/chamador/argumentos; replay reproduz
a recusa. LegacyBoundary conserva o comportamento do perfil anterior.

Condição de saída de P20: consumidor real exigindo IRQ aninhada, controlador
adicional ou retomada de HLT, com nova evidência, frames persistentes e replay.
Não anunciar preempção completa de um kernel NT nem timing físico.

## Validação

Três testes de CPU: STI/prefixos, migração v9 sem promover política/IF, corrupção
e persistência da shadow. Teste IRQL cobre v1 e roundtrip de todos os 32 níveis.
Testes negativos que usavam STI como opcode pendente passam a usar UD2: mantêm
as mesmas verificações de descoberta terminal/continuação, pois STI foi implementado.
A prova de carry passa a conferir 0x202/0x203, preservando IF do bootstrap novo.

Onze casos nativos novos: interrupção dentro de bloco com instrução protegida e
snapshot/replay, CLI e STI/CLI, STI/STI, REP, máscara DIRQL/retorno a Passive,
vblank preemptando loop sem HLE, política legada, KeGetCurrentIrql guest em
ISR=24/DPC=2 e ISR legado=0, TCTX v1/v2 com corrupção e recusa persistente de duas esperas pendentes. Resultados de gate,
debug/release e triangle retraduzido ficam no PLANO e passagem de bastão.
