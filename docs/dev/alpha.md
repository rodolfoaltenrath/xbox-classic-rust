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
| 3. Kernel mínimo | dados e dispatcher concluídos (ADR-0022, ADR-0023); 1/69 serviços | funções efetivamente usadas pelo homebrew; erro claro nas demais |
| 4. ISA necessária | RDTSC/Clock, CMPXCHG/LOCK e SETcc implementados; bloqueio em CLI | traduzir o código emitido pelo compilador e validar instruções adicionais |
| 5. Execução integrada | pendente | carregar módulo, rodar com orçamento, logs e encerramento controlado |
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
uma saída explícita do Rust gerado. O dispatcher atende `RtlInitAnsiString`;
os demais serviços permanecem pendentes.

Os três dados que o homebrew exige — `LaunchDataPage`, `PsThreadObjectType` e
`XeImageFileName` — já têm representação em memória guest, em duas páginas com
permissões distintas ([ADR-0022](../adr/0022-dados-exportados-do-kernel.md)). Com
isso, `translate` do `hello.xbe` deixou de parar no ordinal 164 e passou a parar em
`LOCK CMPXCHG` em `0x209C3`. Essa instrução e as 16 condições SETcc agora estão
implementadas ([ADR-0024](../adr/0024-cmpxchg-setcc.md)). RDTSC também foi implementado,
com o Clock guest e custo sintético explícito ([ADR-0025](../adr/0025-relogio-aot-rdtsc.md)).
O bloqueio atual é `CLI` em `0x1409E`, no caminho CLI/HLT de assert do SDK,
ainda em tempo de tradução. Isso não prova que um assert foi executado.
Sendo AOT, instrução faltante no fluxo descoberto bloqueia antes de
qualquer execução — e as entradas de rotinas de thread também precisarão entrar entre
os destinos recompilados.

O aceite da entrega 2 permanece aberto: há apenas um serviço de kernel implementado
(`1/69` no relatório de ordinais) e o CRT cria uma thread para `main`. Build,
inventário, hashes e limites em [`homebrew.md`](homebrew.md) e
[ADR-0021](../adr/0021-tls-imports-aot.md).

A primeira entrega foi validada com mídia comercial apenas como dado do loader.
Ainda não há execução do homebrew completo nem GPU; o serviço existente foi
validado em fixtures. A BIOS não foi usada.
