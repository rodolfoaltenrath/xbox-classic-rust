# ADR-0026 — CLI e parada explícita por HLT no AOT

Data: 2026-10-20. Estado: aceito no perfil Xbox flat/privilegiado atual.

## Contexto

Após RDTSC (ADR-0025), a tradução do nxdk hello parava em CLI (`0x1409E`),
seguido de HLT (`0x1409F`). O SDK fixado usa esse caminho em `_xbox_assert`.
A descoberta estática dessa rotina não prova que um assert tenha ocorrido.

## Decisão

Acrescentar operações próprias na IR e emitir apenas operações Rust sobre o
estado guest. CLI limpa IF (bit 9 de EFLAGS) e preserva os demais bits. O perfil
atual assume código guest privilegiado, equivalente a CPL 0; não há modelo de
anéis, VM86, CR4.PVI/VME ou virtualização de IF. STI continua recusado, inclusive
se aparecer depois de HLT na continuação estática.

HLT conclui a instrução, guarda seu PC em `State::halted`, coloca EIP na instrução
seguinte e devolve `Exit::Halted { pc, interrupts_enabled }`. Preserva flags e
registradores. O bloco termina em HLT; a continuação vira uma raiz explícita da
análise. Não interromper a descoberta nessa instrução nem descartar o código
posterior: ele é a continuação de uma possível retomada futura.

CLI e HLT custam um passo sintético do perfil de tempo 1. O limite do Clock é
verificado antes dos efeitos, conforme o ADR-0025. Polls de uma CPU suspensa,
inclusive com orçamento zero, devolvem a mesma parada sem executar instruções,
alterar memória ou acrescentar tempo. O estado de retorno do título continua
distinto de parada, orçamento e chamada HLE.

Não executar CLI/HLT físicos, bloquear uma thread host, usar espera ativa ou
transformar a instrução em NOP. O main gerado encerra com diagnóstico contendo
PC de HLT, próximo EIP e IF; não anuncia sucesso nem gira consumindo fatias.

## Limites e snapshots

Não há entrega de IRQ/NMI, IDT ou API de retomada automática nesta entrega.
IF informa a máscara de interrupções externas mascaráveis; IF=0 não significa
que a arquitetura física impossibilite NMI/reset. O runtime permanece suspenso
porque esses eventos ainda não estão modelados. Futuro scheduler deve entregar
eventos com semântica explícita; não basta limpar `halted` ao chamar `run`.

Snapshot **AOTC v5** acrescenta, após o Clock, presença de parada e PC de HLT.
EIP e IF já pertencem ao estado serializado. Versões v1–v4 migram com CPU ativa;
v4 conserva o Clock e perfil, e v1–v3 conservam a migração para tempo zero.
O perfil de tempo permanece 1; RAM/MMU não mudam de formato. Regenerar projetos
antigos para usar a nova fronteira e o tratamento de `Exit::Halted`.

## Validação e próximo bloqueio

Testes da análise verificam IR, fronteira do bloco, continuação descoberta e
recusa de LOCK/REP e de instruções ainda faltantes após HLT. O Rust gerado testa:

- CLI em 4096 combinações de flags, com registradores preservados;
- HLT com IF ligado/desligado e store posterior que nunca executa durante polls;
- CLI/HLT em laço de assert, snapshot/restauração e truncamentos;
- esgotamento de tempo antes de CLI ou HLT, preservando passos já concluídos.

O teste de integração também executa o main realmente emitido para a fixture
CLI/HLT e confere saída com erro e diagnóstico. São **88 casos nativos em x86_64**,
incluindo os anteriores. Não usar oráculo físico para instruções privilegiadas
no processo host; a referência de semântica é o manual Intel abaixo.

O relatório real avança para **INT3 em `0x20D6C`**, ainda durante a tradução.
O desassembly do PE correspondente mostra CALL em `0x20D67`, seguido de bytes
`0xCC` até `0x20D6F`. Investigar o fluxo da chamada e a representação de trap/
término sem presumir execução desse preenchimento nem removê-lo para avançar.
Kernel permanece em 1/69 serviços; threads e framebuffer continuam pendentes.
BIOS e mídias comerciais não foram usadas. Nenhum unsafe foi adicionado.

Aceite local: **249 testes do workspace e doctests**, incluindo o teste que
compila/executa os **88 casos do Rust gerado em debug** e verifica o diagnóstico
do main. Os mesmos 88 casos passaram em **release com `-D warnings`**. Clippy
global com `-D warnings`, formatação e `git diff --check` passaram.
Logs ignorados: `target/control-workspace.log`, `target/control-release.log`,
`target/control-clippy.log` e `target/control-ordinal.log`.

## Referências

- [Intel SDM, volume 2A, CLI e HLT](https://www.intel.com/content/dam/support/us/en/documents/processors/pentium4/sb/25366621.pdf),
  seções 3-109 e 3-439: IF, flags preservadas, privilégio e continuação após HLT.
- [Runtime e snapshot](../../crates/xbox-recomp/src/runtime.rs).
- [Análise de controle](../../crates/xbox-recomp/tests/control.rs).
- [Verificações do Rust gerado](../../tools/xberecomp/tests/verification_control.inc).
