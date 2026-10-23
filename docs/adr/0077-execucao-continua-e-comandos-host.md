# ADR-0077 — Execução contínua, pausa e comandos do host

Data: 2026-10-23. Estado: aceito para PLANO 5.2. Base `d7c07a4`.

## Decisão

`--continuo --janela` repete trechos finitos de Session::conduzir na mesma
sessão. Só BudgetExhausted significa próximo trecho. Serviço recusado, ISA,
memória, HLT/INT3, tempo esgotado e demais erros terminam o laço com diagnóstico.
O orçamento finito padrão e os contratos das sondas continuam disponíveis.
--fatias define o tamanho do trecho, não um limite total no modo contínuo.
Não há throttle/sincronização com relógio real: o Clock guest continua sintético.

Espaço alterna pausa; F5 grava geração nova em `--estados diretório`; F9 restaura
a última geração publicada com sucesso nesta execução. F9 não busca arquivos
arbitrários do diretório. `--carregar` seleciona explicitamente um estado no
início de outro processo. Diretório/gerações existentes são preservados. Erro de
arquivo num atalho é recuperável, relatado no log; falha ao restaurar não troca
a sessão. O estado de pausa do host é preservado ao restaurar.

Control::advance não toca Session nem chama scheduler/run/apresentação quando
pausado. O main continua bombeando eventos/repintando e dorme 16 ms entre polls.
Não avança Clock/RAM/kernel para dar aparência de responsividade. A janela
mostra PAUSADO no título. Não se serializa janela, pausa ou fila de teclas: não
são estado do Xbox; versões internas de snapshot permanecem inalteradas.

Eventos de teclado vivem em xbox-platform/Win32, sem dependências de SO no
runtime. WM_KEYDOWN bit 30 ignora autorrepetição; uma fila por janela conserva
ordem de até 32 comandos, descartando novos comandos se cheia. WM_CLOSE usa
sinal independente, persistente e prioritário. Não injeta input USB/XID nem
simula um controle do console. Não modela hardware novo.
Fonte: [Microsoft WM_KEYDOWN](https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-keydown).
Novo FFI SetWindowTextW usa UTF-16 terminado em NUL, vivo durante a chamada,
e a janela/estado continuam pertencendo exclusivamente à thread criadora.

## Verificação

Testes de Control comparam snapshot completo antes/depois de pausa, checam
propagação de erro diferente de orçamento, gerações preservadas e restauração
falhada mantendo a sessão. Código realmente recompilado pausa e retoma o demo
até EAX=62/37 instruções; parser recusa modo contínuo sem janela e --estados sem
modo contínuo. Mensagens Win32 reais checam ordem, autorrepetição e prioridade
do fechamento. Gate e contadores na evidência da tarefa 5.2 em PLANO.md.
Teste visual ponta a ponta dos samples e launcher pertence à tarefa 5.3.
