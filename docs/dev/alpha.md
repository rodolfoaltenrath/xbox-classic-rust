# Primeiro alpha: homebrew real

Meta inicial: executar um XBE homebrew compilado por um SDK real, produzir saída
verificável e depois apresentar um framebuffer. Fixtures de bytes do projeto
continuam sendo testes; não substituem esse marco.

| Entrega | Estado | Aceite |
|---|---|---|
| 1. Memória/loader | aceite local concluído | T14 real + fixture, contrato sob Miri e benchmark seguro |
| 2. TLS/imports | pendente | inicializar TLS, resolver ordinais e chamar trampolins com ABI correta |
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

Primeiro passo da entrega 2: preparar a build reproduzível desse alvo e inventariar
TLS, ordinais, instruções e destinos do XBE efetivamente gerado. O host atual não
tem clang/make detectados no PATH; o SDK e o XBE ainda não foram compilados.
O executável gerado será artefato local em target, sem binários versionados.
O código de teste nxdk não altera a escolha de Rust para o runtime do projeto.

A primeira entrega foi validada com mídia comercial apenas como dado do loader.
Ainda não há execução de homebrew, GPU ou serviços de kernel. A BIOS não foi usada.
