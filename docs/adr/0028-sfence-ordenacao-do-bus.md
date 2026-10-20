# ADR-0028 — SFENCE e ordenação de stores no bus síncrono

Data: 2026-10-20. Estado: aceito para o perfil de memória exclusivo atual.

## Contexto

Após INT3 (ADR-0027), a tradução do nxdk hello parava em SFENCE (`0F AE F8`)
no PC `0x12A43`. A revisão fixada do SDK usa SFENCE em `XVideoFlushFB` e em
outros caminhos de vídeo/pbkit. Descobrir a instrução estaticamente não prova
execução desses caminhos ou funcionamento do framebuffer.

SFENCE ordena stores anteriores aos posteriores em termos de visibilidade.
Não significa aguardar a conclusão de comandos da GPU. O projeto ainda não
modela store buffers, write-combining ou uma GPU concorrente com a CPU.

## Decisão

Criar `Operation::StoreFence` e emitir `bus.ordenar_escritas()`. A instrução
preserva registradores e flags, avança EIP e custa um passo sintético do perfil
de tempo 1. O limite do Clock é verificado antes da instrução. A descoberta
continua normalmente; SFENCE não é salto, exceção ou retorno ao host.

### Contrato da barreira

Todas as escritas de RAM são aplicadas sincronicamente no bus. As visões virtual,
física, DMA e aliases observam a mesma arena. Ela tem posse exclusiva, sem `Sync`
ou compartilhamento concorrente: não pode haver um leitor DMA mantendo sua
fatia enquanto a CPU modifica a memória pela API segura.

Por isso, **o corpo atual de `Bus::ordenar_escritas` é vazio**: não existe store
pendente para publicar, e o modelo já fornece a ordenação exigida. Não há fence
física ou atômica host necessária para os observadores admitidos neste perfil.
O tipo `&mut Bus` exige posse exclusiva na barreira e mantém um ponto explícito
para implementar protocolos de escrita diferida no futuro. O allow pontual de
`clippy::unused_self` documenta essa escolha de API, sem acesso artificial à RAM.

Não agrupar a instrução com NOP na IR nem apagar bytes do XBE. O código emitido
atravessa a API de ordenação do bus. Esta implementação não pretende simular
latências, caches ou toda a memória fracamente ordenada do Xbox.

### Dispositivos e evolução

MMIO de dispositivos ainda não implementados continua com o comportamento já
declarado: telemetria e descarte de escritas. SFENCE não passa a implementá-los,
fabricar conclusão, produzir imagem ou sinalizar uma fence de GPU.

Antes de introduzir filas de stores, write-combining, DMA concorrente ou workers,
rever esta barreira: os stores anteriores devem ficar visíveis antes dos
posteriores para os observadores desse novo perfil. Uma fence atômica do host
sozinha não drena filas guest, publica buffers de upload nem define o protocolo
de um dispositivo. Isso também exige rever o contrato de acesso à arena e o
snapshot de qualquer fila nova.

LFENCE, MFENCE, CLFLUSH e stores não temporais continuam recusados. Implementar
SFENCE isoladamente não habilita operações de dados SSE/MMX.

## Snapshots e validação

Sem fila de stores ou continuação nova, **AOTC permanece v6**, memória mantém seu
formato e o perfil de tempo continua 1. Snapshot após a barreira contém os bytes
já escritos, como as demais fronteiras de execução. Não acrescentar um contador
de fences como estado fictício do hardware.

Testes de análise cobrem IR, continuação e opcodes/prefixos ainda recusados. O
contrato do bus verifica visibilidade por alias e DMA após escrita virtual e
por acesso virtual após escrita física, sem acessos MMIO inventados. Quatro
novos casos do Rust gerado cobrem:

- registradores/flags preservados e ausência de acesso implícito aos endereços
  contidos nos registradores;
- stores em torno de barreiras, aliases/DMA, fatias e snapshot/replay;
- faltas de escrita antes/depois de SFENCE, sem mudar permissões nem perder
  passos anteriores já concluídos;
- orçamento zero e limite do Clock antes de SFENCE e da instrução seguinte.

O conjunto nativo passa de 92 para **96 casos em x86_64**. O próximo bloqueio
observado do alvo é **CMOVNE em `0x13B88`** (`Cmovne_r32_rm32`), ainda durante a
tradução. Continuar com CMOVcc e seus operandos/flags/faltas. Kernel segue com
1/69 serviços; threads, execução completa do homebrew e framebuffer pendentes.
Nenhum unsafe foi adicionado; BIOS e mídias comerciais não foram usadas.

Aceite local: **257 testes do workspace e doctests**, incluindo geração/execução
dos **96 casos nativos em debug**. Os mesmos casos passaram em **release com
`-D warnings`**. O novo contrato de alias/físico/DMA passou sob Miri, com o aviso
informativo já conhecido de opt-level ignorado. Não foi uma repetição de toda a
suíte Miri. Clippy global com `-D warnings`, fmt e diff passaram.
`ordinal-report` e `translate` confirmaram CMOVNE como próximo bloqueio.
Logs ignorados: `target/sfence-workspace.log`, `target/sfence-release.log`,
`target/sfence-clippy.log`, `target/sfence-miri.log`, `target/sfence-ordinal.log`
e `target/sfence-translate.log`.

## Referências

- [Intel SDM, volume 2B, seção 4-294: SFENCE](https://www.intel.com/content/dam/support/us/en/documents/processors/pentium4/sb/25366721.pdf),
  ordenação de stores. A aplicação a um bus síncrono exclusivo é uma decisão
  deste runtime, não uma alegação de simular caches físicos.
- [Bus e barreira](../../crates/xbox-mem/src/bus.rs).
- [Contrato de memória](../../crates/xbox-mem/tests/contract.rs).
- [Verificação do Rust gerado](../../tools/xberecomp/tests/verification_ordering.inc).
- [Build do nxdk](../dev/homebrew.md).
