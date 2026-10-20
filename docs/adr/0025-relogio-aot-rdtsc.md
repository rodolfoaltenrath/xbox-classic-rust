# ADR-0025 — Relógio virtual do AOT e RDTSC

Data: 2026-10-20. Estado: aceito como política de tempo funcional provisória.

## Contexto

Após o ADR-0024, o nxdk hello parava em RDTSC no PC `0x2055B`, durante a
tradução. O projeto já tinha `xbox_core::time::Clock`, suas conversões de tempo,
snapshot `CLK0` e um `Scheduler`, mas o AOT não usava o relógio. `State::retired`
conta instruções concluídas; não pode descrever elementos REP parcialmente
executados ou esperas futuras.

## Decisão

Reutilizar `Clock` em `State::clock`, como relógio único da sessão AOT atual.
Não ler RDTSC físico, `Instant` ou tempo de parede para executar código guest.
RDTSC tem operação própria na IR, lê `clock.tsc()` antes do custo da própria
instrução e escreve a parte baixa em EAX e a alta em EDX, sem alterar flags.
O perfil atual é flat/privilegiado; não introduz CR4, CPL ou um modelo de
execução fora de ordem. RDTSCP continua recusado.

### Política AOT de tempo, perfil 1

- Uma instrução concluída custa **um ciclo virtual**.
- REP custa um ciclo por elemento concluído, mesmo enquanto `retired` ainda não
  inclui a instrução. REP com contador inicial zero custa um ciclo, sem acesso
  de dados.
- Uma instrução/elemento que falha não cobra seu passo. Trabalho já concluído
  antes da falta conserva o tempo que consumiu.
- Orçamento zero, sessão já retornada, entrada/poll de trampolim e serviço HLE
  atual não acrescentam ciclos. Não há custo implícito por chamada do host.
- Trocar o tamanho das fatias de execução não muda a sequência de timestamps.

Esse é um modelo sintético determinístico, **não** uma estimativa de latências
do Pentium III. As unidades/conversões permanecem as de `Clock`, mas um passo
AOT não é prova de um ciclo físico do Xbox. O perfil deve ser refinado e
versionado antes de prometer precisão temporal, FPS ou sincronização audiovisual.

O agendador do core ainda não está integrado à sessão AOT; timers, esperas e
vblank ainda não disparam por este trabalho. Ao introduzir threads, todas devem
usar o relógio da sessão: não copiar `State::clock` para cada contexto. O futuro
scheduler deve assumir a mesma autoridade de tempo, avançando até deadlines
quando todas as threads estiverem esperando, sem manter relógios independentes.

### Limite e snapshots

Validar a capacidade de avançar o relógio antes dos efeitos de cada instrução ou
elemento REP. No limite de representação `u64`, retornar `TimeExhausted` sem
executar o próximo passo. Não permitir overflow com pânico em debug ou wrap
silencioso em release. É um limite explícito do runtime, não uma exceção da CPU
nem uma implementação do rollover físico de TSC.

`AOTC` v4 guarda FS, identificador de perfil de tempo **1** e a seção `CLK0` v1
do `Clock` existente. Um perfil desconhecido é erro. Migração de v1/v2/v3 inicia
o relógio em zero: essas versões não guardavam tempo e `retired` não permite
reconstruí-lo. RAM/MMU não mudam de formato. Snapshots atuais restauram o instante
exato, inclusive no meio de REP ou em uma fronteira HLE.

Projetos Rust gerados anteriormente devem ser regenerados para usar a
contabilização nova. O custo não deve ser alterado silenciosamente mantendo o
mesmo perfil de snapshot; revisões de tempo precisam de contrato de compatibilidade.

## Validação e resultado real

Os testes compilam e executam o Rust emitido. Cobrem valor de 64 bits e carry
entre EAX/EDX, flags/registradores preservados, tempo independente de `retired`,
fatias diferentes, orçamento zero, sessão retornada, REP parcial/vazio,
snapshot/replay, falta de store, perfil inválido e esgotamento do relógio.
Há também uma chamada real ao dispatcher de `RtlInitAnsiString`, demonstrando
que entrada HLE e polls não inventam tempo nem criam outro relógio.

O projeto gerado passa de 79 para 84 testes em x86_64. O nxdk hello passa pelo
RDTSC e encontra **CLI no PC `0x1409E`**, seguido de HLT em `0x1409F`, no
caminho `_xbox_assert` do SDK, ainda durante a tradução. Isso não
significa que o homebrew executou. Kernel segue com 1/69 serviços do alvo,
threads e framebuffer ainda pendentes. Nenhum unsafe foi adicionado.

Aceite local: 245 testes somando workspace e doctests, mais 84 casos do projeto
gerado em debug e release (release com `-D warnings`). Clippy global, fmt e
diff passaram. Logs ignorados: `target/rdtsc-workspace.log`,
`target/rdtsc-native.log`, `target/rdtsc-release.log` e `target/rdtsc-clippy.log`.

## Referências

- [Intel SDM, volume 2B — RDTSC](https://www.intel.com/content/dam/www/public/us/en/documents/manuals/64-ia-32-architectures-software-developer-vol-2b-manual.pdf):
  destinos EDX:EAX e flags preservadas. A política de custos é uma decisão do
  nosso runtime, não uma propriedade extraída da instrução.
- [Clock e Scheduler existentes](../../crates/xbox-core/src/time.rs).
- [Runtime AOT](../../crates/xbox-recomp/src/runtime.rs) e
  [testes do Rust gerado](../../tools/xberecomp/tests/verification_timing.inc).

## Próxima ação

Definir o perfil de controle/parada de CLI e HLT nesse caminho de assert, com
flags, diagnóstico e fronteiras apropriadas. A descoberta estática não prova
que o assert tenha sido executado. Não retirar instruções para passar pela
tradução. Continuar a descoberta de ISA, serviços e entradas de threads
conforme os bloqueios reais; esta entrega não fecha o alpha completo.
