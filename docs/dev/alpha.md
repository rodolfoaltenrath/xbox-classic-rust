# Primeiro alpha: homebrew real

Para retomar o projeto em outra sessão ou LLM, consulte
[CONTINUIDADE_ALPHA.md](../../CONTINUIDADE_ALPHA.md), com as seis entregas,
os dois commits de implementação, seus limites e os próximos passos.

Meta inicial: executar um XBE homebrew compilado por um SDK real, produzir saída
verificável e depois apresentar um framebuffer. Fixtures de bytes do projeto
continuam sendo testes; não substituem esse marco.

| Entrega | Estado | Aceite |
|---|---|---|
| 1. Memória/loader | aceite local concluído | T14 real + fixture, contrato sob Miri e benchmark seguro |
| 2. TLS/imports | infraestrutura validada; integração com homebrew pendente | TLS nxdk, FS e ABI disponíveis; dados exportados dependem da entrega 3 |
| 3. Kernel mínimo | dados, dispatcher e criação de mutexes (ADR-0032); 2/69 serviços com variantes limitadas | funções efetivamente usadas pelo homebrew; erro claro nas demais |
| 4. ISA necessária | fluxo descoberto traduz; 8769 instruções com callback explícito; novas entradas podem exigir mais ISA | traduzir o código emitido pelo compilador e validar instruções adicionais |
| 5. Execução integrada | hello inicia nativamente e para em XcSHAInit (335); aceite aberto | carregar módulo, rodar com orçamento, logs e encerramento controlado |
| 6. Framebuffer | pendente | primeira imagem apresentada; controle/áudio ficam para o próximo marco |

As seis entregas são uma divisão de trabalho, não garantia de seis commits ou prazo.
Dependências reais de kernel/ISA só serão conhecidas após compilar o alvo.

## Alvo escolhido

[nxdk samples/hello](https://github.com/XboxDev/nxdk/tree/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/samples/hello),
revisão `14d5ee97e73347c973f1f57b68b79ec08c9e77f2`, consultada em 2026-10-20.
O sample configura vídeo 640×480, imprime texto e usa Sleep no laço. Isso oferece
um objetivo observável para vídeo e temporização; seu laço termina pelo controle
de execução do host, não por um retorno espontâneo do programa.

O alvo já foi compilado pelo WSL/Fedora, com SDK/submódulos fixados e hashes
repetidos após normalização de metadata. A inspeção verificou 4 seções, 27 páginas
e 72 imports: 69 funções e 3 dados. A infraestrutura de uma thread inicial já
inicializa TLS nxdk, aplica FS e transporta chamadas stdcall/cdecl/fastcall até
uma saída explícita do Rust gerado. O dispatcher atende `RtlInitAnsiString`
e criação de mutex sem nome por `NtCreateMutant`;
os demais serviços permanecem pendentes.

Os três dados que o homebrew exige — `LaunchDataPage`, `PsThreadObjectType` e
`XeImageFileName` — já têm representação em memória guest, em duas páginas com
permissões distintas ([ADR-0022](../adr/0022-dados-exportados-do-kernel.md)). Com
isso, `translate` do `hello.xbe` deixou de parar no ordinal 164 e passou a parar em
`LOCK CMPXCHG` em `0x209C3`. Essa instrução e as 16 condições SETcc agora estão
implementadas ([ADR-0024](../adr/0024-cmpxchg-setcc.md)). RDTSC também foi implementado,
com o Clock guest e custo sintético explícito ([ADR-0025](../adr/0025-relogio-aot-rdtsc.md)).
CLI/HLT também têm semântica guest e parada explícita ao host, preservada por
snapshot ([ADR-0026](../adr/0026-controle-cli-hlt.md)). INT3 tem fronteira #BP
explícita, preservada em snapshot, sem entrega ao kernel ainda
([ADR-0027](../adr/0027-int3-breakpoint-aot.md)). SFENCE usa a ordenação do bus
síncrono exclusivo ([ADR-0028](../adr/0028-sfence-ordenacao-do-bus.md)), sem concluir
trabalho de GPU. CMOVcc cobre 16 condições em 16/32 bits, com leitura
incondicional da origem ([ADR-0029](../adr/0029-cmovcc.md)). BSF/BSR destravaram
a tradução ([ADR-0030](../adr/0030-bsf-bsr.md)). A execução real revelou estouro
de pilha host, corrigido separando funções de bloco e limitando-as a 32 instruções.
Com a entrada explícita `0x19110` para um callback observado, o hello traduz
8769 instruções/blocos e **executa nativamente até NtCreateMutant (192)**,
retorno reportado `0x18CFA`, argumentos `0x28EEC, 0, 0`
([ADR-0031](../adr/0031-blocos-aot-pilha-host.md)). Esse serviço foi implementado
para mutexes sem nome, com handles guest e estado no snapshot integral da sessão
([ADR-0032](../adr/0032-mutants-guest-snapshot-sessao.md)). O hello real passou
pela criação e agora para em **XcSHAInit (335)**, retorno `0x19140`, argumento
`0x28EF0`, no caminho de `init_rand_s`. Esse é o próximo serviço a modelar;
outras raízes de callbacks/threads ainda podem revelar novos bloqueios.

O aceite da entrega 2 permanece aberto: há dois serviços com variantes limitadas
(`2/69` no relatório de ordinais) e o CRT cria uma thread para `main`. Build,
inventário, hashes e limites em [`homebrew.md`](homebrew.md) e
[ADR-0021](../adr/0021-tls-imports-aot.md).

A primeira entrega foi validada com mídia comercial apenas como dado do loader.
Ainda não há execução completa até o framebuffer. A primeira execução do
hello parou na inicialização SHA após criar mutex, sem respostas genéricas de sucesso.
A BIOS não foi usada.
