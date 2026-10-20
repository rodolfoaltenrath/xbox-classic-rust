# ADR-0027 — INT3 e fronteira explícita de breakpoint guest

Data: 2026-10-20. Estado: aceito no perfil AOT atual, sem entrega via IDT/SEH.

## Contexto e investigação do alvo

Depois de CLI/HLT (ADR-0026), a tradução do nxdk hello parava em INT3 no PC
`0x20D6C`. O desassembly do PE correspondente mostra uma rotina em `0x20D50`
com argumento 6, CALL para `0x21220`, argumento -1, CALL para `0x19050` e
preenchimento `0xCC` até `0x20D6F`.

O objeto público `lib/pdclib/functions/stdlib/abort.obj` da build fixada identifica
a rotina como `_abort`: a mesma sequência termina no offset `0x1C`, com
relocações para `_raise` e `_exit`. O fonte `abort.c` contém `raise(SIGABRT)` e
`exit(EXIT_FAILURE)`. Isso explica a descoberta do preenchimento após a chamada
sem retorno esperado; não prova que o homebrew executou esse caminho.

Não inferir `noreturn` a partir de padrões ou nomes, nem apagar os bytes do XBE.
Se o controle chegar ao opcode `0xCC`, produzir um breakpoint guest verificável.

## Decisão

Adicionar `Operation::Breakpoint` para **INT3 curto (`0xCC`)**. Ao executar:

- Guardar o PC de origem em `State::breakpoint` e avançar EIP à instrução seguinte.
- Preservar registradores, memória e flags guest na fronteira anterior à entrega.
- Concluir um passo sintético de tempo e aposentadoria, verificando antes o
  limite do Clock, como nas demais instruções do perfil 1.
- Devolver `Exit::Breakpoint { pc }`, representando **#BP, vetor 3**, independente
  de IF. Polls posteriores, inclusive com orçamento zero ou Clock no limite,
  devolvem a mesma exceção sem executar código ou cobrar novo passo.

O main gerado encerra com erro e diagnóstico contendo #BP, vetor, PC de INT3 e
próximo EIP. Não executar INT3 físico nem disparar o debugger do host. A saída
permanece distinta de retorno do título, HLT, orçamento e chamada de kernel.

### Descoberta de fluxo

INT3 termina o bloco e a descoberta desse caminho, porque transfere para uma
exceção. Não há fallthrough automático. Isso evita interpretar preenchimento
como execução normal. Bytes posteriores continuam sujeitos à tradução quando
alcançados por branches ou raízes explícitas `--entry`, com todas as recusas
usuais de instrução, sobreposição e permissões.

Essa política difere de HLT: a continuação de HLT é descoberta para uma futura
retomada por evento; um breakpoint exige tratamento de exceção/debugger, ainda
ausente. Implementar retomada de #BP exigirá compilar seus destinos, entregar a
exceção e consumir o estado pendente explicitamente. Não basta limpar o campo
para alcançar um PC que não foi traduzido.

## Limites e snapshots

Não há frame de interrupção na pilha, IDT, handler SEH, IRET ou retomada automática.
As flags preservadas são as do contexto interrompido, antes de efeitos da entrada
em um handler. `INT n` (`0xCD`), inclusive `INT 3`, INT1, INTO e UD2 continuam
recusados; não ampliar esses contratos por semelhança de opcode.

**AOTC v6** acrescenta presença de breakpoint e PC após o estado de HLT. Versões
v1–v5 migram sem breakpoint pendente; as migrações de FS, Clock/perfil e HLT
permanecem. Um snapshot com breakpoint junto de retorno ou HLT é recusado por
estado incompatível. O perfil de tempo continua 1 e RAM/MMU não mudam de formato.
Regenerar projetos Rust antigos para obter a nova fronteira de execução.

## Validação e resultado real

Testes de análise verificam opcode curto, exceção terminal, roots adicionais,
branch que contorna o breakpoint e recusa de outros opcodes/prefixos. Testes de
snapshot verificam migração de v5 ativo/retornado/suspenso e estados incompatíveis.
Quatro casos adicionais no Rust gerado verificam:

- EIP, registradores, flags com IF ligado/desligado e memória preservados;
- polls, orçamento zero e snapshot/restauração/truncamentos com #BP pendente;
- caminhos de branch e tamanhos diferentes de fatias;
- Clock esgotado antes de INT3, sem perder um store anterior já concluído.

O main realmente emitido é executado para fixtures de HLT e INT3, com erro e
diagnóstico conferidos. O conjunto gerado passa de 88 para **92 casos em x86_64**.

O relatório real avança para **SFENCE em `0x12A43`**, bytes `0F AE F8`, ainda em
tempo de tradução. Definir seu contrato de ordenação/visibilidade de stores no
bus e no perfil de dispositivos antes de implementá-lo. Isso não demonstra que
o homebrew executou. Kernel segue em 1/69 serviços; threads e framebuffer pendentes.
Nenhum unsafe foi adicionado; BIOS e mídias comerciais não foram usadas.

Aceite local: **254 testes do workspace e doctests**, incluindo a geração e
execução dos **92 casos nativos em debug** e os diagnósticos dos mains. Os mesmos
92 casos passaram em **release com `-D warnings`**. Clippy global com
`-D warnings`, fmt, links locais e `git diff --check` passaram. Tanto
`ordinal-report` quanto a CLI `translate` confirmaram SFENCE como próximo erro.
Logs ignorados: `target/breakpoint-workspace.log`, `target/breakpoint-release.log`,
`target/breakpoint-clippy.log`, `target/breakpoint-ordinal.log` e
`target/breakpoint-translate.log`.

## Referências

- [Intel SDM, volume 3A, seção 6-23: #BP](https://www.intel.com/content/dam/www/public/us/en/documents/manuals/64-ia-32-architectures-software-developer-vol-3a-part-1-manual.pdf):
  vetor 3, classe trap e EIP seguinte. A fronteira sem entrega e o custo sintético
  são decisões do runtime, não uma implementação completa de IDT.
- [Runtime e snapshots](../../crates/xbox-recomp/src/runtime.rs).
- [Análise de INT3](../../crates/xbox-recomp/tests/breakpoint.rs).
- [Testes do Rust gerado](../../tools/xberecomp/tests/verification_breakpoint.inc).
- [Build pública e revisão do nxdk](../dev/homebrew.md).
