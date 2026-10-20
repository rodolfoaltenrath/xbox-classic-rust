# ADR-0024 — CMPXCHG, LOCK em RAM e SETcc no AOT

Data: 2026-10-20. Estado: aceito para o runtime de acesso exclusivo ao bus.

## Contexto

Após a representação dos dados e o dispatcher dos ADRs 0022/0023, o nxdk hello
ainda falhava durante a tradução em `LOCK CMPXCHG`, PC `0x209C3`, seguido de
`SETE` em `0x209C9`. O dispatcher não pode atender serviços antes de existir
um módulo que a análise consiga traduzir.

## Decisão

Adicionar operações explícitas de compare/exchange e condição materializada na IR.
CMPXCHG suporta operandos de 8/16/32 bits, registradores parciais e memória.
Capturar acumulador, origem, destino e endereço antes de qualquer alteração
preserva aliases como AL/AH, EAX e bases/índices do endereço.

As flags aritméticas correspondem a acumulador menos destino. Se forem iguais,
o destino recebe a origem; caso contrário, o acumulador recebe o destino original.
O destino recebe uma escrita mesmo na comparação desigual. Portanto, uma página
RO falha também nesse caso. Todas as permissões/páginas são verificadas antes
de ler o destino e antes de comprometer RAM, registradores ou flags.

LOCK é aceito somente para CMPXCHG com destino de memória neste perfil. O acesso
inteiro à RAM ocorre com posse exclusiva do `Bus`, sem yield no meio da instrução.
Isso implementa a indivisibilidade no modelo atual de execução guest serializada;
não habilita acesso concorrente à arena nem é uma implementação de CAS entre
threads host. Um scheduler futuro deve preservar essa fronteira. MMIO é recusado
antes da leitura porque precisa de um protocolo próprio de read-modify-write.
CMPXCHG8B, LOCK em registrador e LOCK nas demais instruções continuam recusados.

SETcc implementa as 16 condições com resultado byte 0/1, em registrador baixo,
alto ou memória. Não altera flags. Uma falha de store preserva o estado da CPU.
FS e endereços de 32 bits reutilizam o caminho existente; address16 é recusado.

Não há mudança de snapshot: as instruções concluem ou falham como uma unidade,
sem continuação nova. O estado atual `AOTC` v3 continua suficiente.

## Validação

O Rust realmente emitido é compilado e executado nos testes. Cobertura inclui
sucesso/falha, aliases de acumulador/origem/destino, SIB com EAX, FS, RO,
acesso cruzando página, MMIO recusado, dirty bit, orçamento e snapshot/reexecução.
As 16 condições são verificadas para todas as 32 combinações de CF/PF/ZF/SF/OF
em AL, AH e memória, com preservação dos demais bits e flags.

Um oráculo x86_64 adicional executa CMPXCHG físico apenas sobre memória local
válida. Com e sem LOCK, confere 147456 casos de resultado/acumulador/flags com
o código recompilado, incluindo todas as combinações de acumulador/destino byte.
Asm é limitado ao crate de oráculo; o runtime AOT continua em Rust seguro.

O projeto gerado passa de 71 para 79 testes no host x86_64. A tradução do hello
real avançou para `RDTSC` em `0x2055B`, ainda sem suporte. O homebrew não executou;
o dispatcher mantém um serviço implementado. Endereços citados são da build
nxdk fixada em `docs/dev/homebrew.md`.

Aceite local: 241 testes somando workspace e doctests, mais 79 testes do projeto
gerado em debug e release (release com `-D warnings`). Clippy global, fmt e
diff passaram. Logs locais, ignorados pelo Git: `target/exchange-workspace.log`,
`target/exchange-release.log` e `target/exchange-clippy.log`.

## Referência

[Intel SDM volume 2, CMPXCHG e SETcc](https://cdrdv2-public.intel.com/774492/325383-sdm-vol-2abcd.pdf).
A implementação usa a operação de comparação, flags e escrita do destino
descritas pelo manual. Não estima ciclos do Pentium III nem implementa um TSC.

## Continuação

Definir um relógio guest determinístico e preservável em snapshot para RDTSC,
com contrato de avanço compatível com o futuro scheduler/esperas. Resolver essa
instrução permitirá descobrir os próximos bloqueios; não garante tradução
completa. Entradas de threads e callbacks também precisarão ser recompiladas.
