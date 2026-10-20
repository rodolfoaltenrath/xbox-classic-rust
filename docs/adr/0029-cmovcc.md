# ADR-0029 — CMOVcc de 16/32 bits e leitura incondicional da origem

Data: 2026-10-20. Estado: aceito para o perfil inteiro AOT atual.

## Contexto

Após SFENCE (ADR-0028), a tradução do nxdk hello parava em CMOVNE no PC
`0x13B88`, bytes `0F 45 C1`: ECX para EAX quando ZF=0. Já existem as 16
condições usadas por Jcc/SETcc e operandos de memória com FS/SIB, mas nenhuma
operação de movimento condicional na IR.

## Decisão

Adicionar `Operation::ConditionalMove { condition, dst, src }` para as 16
condições CMOVcc de **16 e 32 bits**, com destino registrador e origem
registrador/memória de mesma largura. A origem é capturada **antes** de testar
a condição. Isso é necessário porque CMOVcc lê a memória mesmo com condição
falsa: falta de página e efeitos de leitura do bus não podem ser eliminados
como se fosse um branch em torno de MOV.

Se a condição for verdadeira, escrever o valor capturado. Em word, preservar
os 16 bits superiores do registrador; em dword, substituir o valor completo
guest de 32 bits. Se falsa, preservar todo o destino. Flags são preservadas
em ambos os casos. Aliases de registrador, endereço e ESP usam o valor anterior
à escrita do destino.

Reutilizar o decoder/operandos, `condition_met` e a escrita de subregistradores.
Não inferir valores constantes através de CMOV: a descoberta de destinos
indiretos permanece conservadora. LOCK, REP/REPNE, segmentos fora do perfil e
endereçamento de memória de 16 bits continuam recusados. Não há variante byte
ou execução x86-64 guest adicionada.

Uma instrução concluída cobra um passo sintético do perfil de tempo 1,
inclusive quando falsa. Uma leitura que falha não altera destino/flags nem
avança o tempo do passo. Bits de acesso/telemetria de leituras do bus seguem
seu contrato existente; não se promete rollback de leituras já observadas.

Não há estado de CPU ou fila novo: **AOTC permanece v6**, RAM/MMU e perfil de
tempo permanecem compatíveis. Snapshots em fronteiras de bloco preservam as
flags consumidas pela instrução, como nos demais movimentos inteiros.

## Verificação independente

O oráculo opcional de `xbox-cpu` ganhou CMOVcc físico em registradores locais.
Somente esse oráculo usa o novo unsafe: assembly fixo, sem ponteiros guest,
pilha balanceada e entrada de flags mascarada a CF/PF/AF/ZF/SF/OF. Flags de
controle do host são preservadas. Ele continua restrito à feature `isa-oracle`
em x86_64; não é um backend de execução do Xbox.

O Rust realmente emitido é comparado com esse oráculo em **32768 casos**:
16 condições × 2 larguras × 64 combinações de flags aritméticas × 16 pares de
valores de borda/pseudoaleatórios. Também há seis verificações portáveis:

- condições verdadeiras/falsas, flags preservadas e metade alta de word;
- origem RO, leituras incondicionais e telemetria MMIO mesmo quando falsa;
- origem não mapeada em todas as condições/larguras, com CPU intacta;
- leitura cruzando páginas e largura correta após resolver a falta;
- aliases, FS/SIB e alteração de SP/ESP seguida de RET na pilha guest;
- flags após CMP, snapshot/replay, fatias, orçamento zero e limite do Clock.

MMIO ainda não implementado só produz a telemetria/placeholder já declarado;
testar essa leitura não prova funcionamento de dispositivo. O conjunto gerado
passa de 96 para **103 testes em x86_64** (88 em outros hosts).

## Resultado do alvo e próxima ação

O relatório real passa de CMOVNE para **BSR em `0x1657C`**, bytes `0F BD CB`,
EBX para ECX. Há um JZ logo depois (`0x1657F`), portanto o comportamento de
origem zero e ZF precisa ser definido e testado na próxima implementação,
incluindo a política para resultados/flags indefinidos da ISA.

O bloqueio continua em tempo de tradução; o homebrew completo não executou.
Kernel segue com 1/69 serviços, threads e framebuffer pendentes. BIOS e mídias
comerciais não foram usadas. Nenhum unsafe foi adicionado ao AOT/runtime/bus.

Aceite local: **259 testes do workspace e doctests**, incluindo a geração e
execução de **103 testes nativos em debug**, com as 32768 comparações físicas.
Os mesmos 103 passaram em **release com `-D warnings`**. Clippy global com
`-D warnings`, fmt e diff passaram. `ordinal-report` e `translate` confirmaram
BSR como próximo bloqueio. Não foi necessário alterar contratos de memória nem
repetir Miri: o novo assembly só participa do oráculo físico de testes.
Logs ignorados: `target/cmov-workspace.log`, `target/cmov-release.log`,
`target/cmov-clippy.log`, `target/cmov-ordinal.log` e `target/cmov-translate.log`.

## Referências

- [Intel SDM, volume 2A — CMOVcc](https://cdrdv2-public.intel.com/812383/253666-sdm-vol-2a.pdf):
  origem lida incondicionalmente, condições, larguras e flags preservadas.
- [Análise e IR](../../crates/xbox-recomp/src/analysis.rs).
- [Oráculo físico opcional](../../crates/xbox-cpu/src/oracle/conditional.rs).
- [Verificações do Rust gerado](../../tools/xberecomp/tests/verification_conditional.inc).
