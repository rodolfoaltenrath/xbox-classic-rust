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
| 3. Kernel mínimo | dados, dispatcher, mutex com espera/liberação, SHA-1 e EEPROM (ADR-0022 a 0035); 8/69 serviços | funções efetivamente usadas pelo homebrew; erro claro nas demais |
| 4. ISA necessária | CLI/HLT, RDTSC, CMPXCHG/LOCK, SETcc, CMOVcc, BSF/BSR e CLC/STC/CMC; bloqueio atual é de serviço, não de ISA | traduzir o código emitido pelo compilador e validar instruções adicionais |
| 5. Execução integrada | executa até `RtlInitializeCriticalSection`; orçamento/logs/interrupção pendentes | carregar módulo, rodar com orçamento, logs e encerramento controlado |
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
e criação de mutex sem nome por `NtCreateMutant`, além de SHA-1 Init/Update/Final
e consulta integral da EEPROM;
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
([ADR-0032](../adr/0032-mutants-guest-snapshot-sessao.md)). Nessa entrega o hello
passou pela criação e parou em **XcSHAInit (335)**, retorno `0x19140`, argumento
`0x28EF0`, no caminho de `init_rand_s`. SHA-1 agora está
implementado com contexto opaco HLE na RAM e testes de hash/cópia/snapshot
([ADR-0033](../adr/0033-sha1-contexto-guest.md)). A execução real passou por
SHAInit e parou em **ExQueryNonVolatileSetting (24)**, retorno `0x19176`, consulta
de 256 bytes da EEPROM com índice `0xFFFF`. O ADR-0034 implementou essa leitura
com bytes/origem explícitos e snapshot. O hello passou pela consulta e agora
para em **NtWaitForSingleObject (233)**, retorno `0x16B99`, argumentos `4, 0, 0`.
O próximo passo é aquisição/espera real sobre o mutex criado;
outras raízes de callbacks/threads ainda podem revelar novos bloqueios.

O aceite da entrega 2 permanece aberto: há seis serviços com variantes limitadas
(`6/69` no relatório de ordinais) e o CRT cria uma thread para `main`. Build,
inventário, hashes e limites em [`homebrew.md`](homebrew.md) e
[ADR-0021](../adr/0021-tls-imports-aot.md).

A primeira entrega foi validada com mídia comercial apenas como dado do loader.
Ainda não há execução completa até o framebuffer. A execução mais recente do
hello passou pela consulta de EEPROM e chegou a NtWaitForSingleObject, sem respostas
genéricas de sucesso. O padrão usa EEPROM sintética, sem dump físico;
contrato e limites em [ADR-0034](../adr/0034-consulta-eeprom-sessao.md).
A BIOS não foi usada.
