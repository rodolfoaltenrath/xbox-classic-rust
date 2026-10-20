# Primeiro alpha: homebrew real

Meta inicial: executar um XBE homebrew compilado por um SDK real, produzir saída
verificável e depois apresentar um framebuffer. Fixtures de bytes do projeto
continuam sendo testes; não substituem esse marco.

| Entrega | Estado | Aceite |
|---|---|---|
| 1. Memória/loader | aceite local concluído | T14 real + fixture, contrato sob Miri e benchmark seguro |
| 2. TLS/imports | infraestrutura validada; integração com homebrew pendente | TLS nxdk, FS e ABI disponíveis; dados exportados dependem da entrega 3 |
| 3. Kernel mínimo | pendente | funções efetivamente usadas pelo homebrew; erro claro nas demais |
| 4. ISA necessária | pendente | traduzir o código emitido pelo compilador e validar instruções adicionais |
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
uma saída explícita do Rust gerado. Os serviços ainda não foram implementados.

O aceite completo da entrega 2 permanece aberto: o homebrew exige
`LaunchDataPage`, `PsThreadObjectType` e `XeImageFileName`, além de criar uma thread
para `main`. O AOT recusa esses dados até que existam implementações adequadas.
Também foram identificados LOCK CMPXCHG e SETE sem suporte na ISA. A próxima
entrega deve tratar os dados e serviços de inicialização, com execução integrada
ainda sujeita aos bloqueios de ISA. Build, inventário, hashes e limites em
[`homebrew.md`](homebrew.md) e [ADR-0021](../adr/0021-tls-imports-aot.md).

A primeira entrega foi validada com mídia comercial apenas como dado do loader.
Ainda não há execução de homebrew, GPU ou serviços de kernel. A BIOS não foi usada.
