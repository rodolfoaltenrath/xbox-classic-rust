# Avaliação do XboxXBERecomp para o Obsidian

Data: 2026-10-19.

Natureza: análise de viabilidade; não altera as decisões aceitas nem declara uma
integração implementada. Foram lidos o ROADMAP, os ADRs pertinentes, os subsistemas
locais e o código-fonte externo. Não foram compilados o SDK ou jogos, nem medidos FPS.

Repositório analisado: [TGP482/XboxXBERecomp](https://github.com/TGP482/XboxXBERecomp).
Revisão inspecionada: `99078b119b5d5e0d1d6afb1390e29437c0c984c0`, commit de
2026-10-18, “Release ships the compiler instead of the game code”. A cópia para
consulta ficou em `%TEMP%/obsidian-xberecomp-review/XboxXBERecomp`, fora do workspace.

## Conclusão

É possível reutilizar o recompilador e construir um modo de execução nativa por
jogo. A licença MIT permite copiar e modificar o código mediante preservação dos
avisos exigidos. Entretanto, copiar apenas o tradutor não entrega jogos funcionando:
o resultado depende de serviços de execução, kernel, bibliotecas XDK, gráficos,
áudio, arquivos e controles.

O projeto externo é um SDK para produzir ports específicos. Ele não demonstra que
qualquer ISO possa ser convertida automaticamente em um jogo compatível. O Obsidian
poderia oferecer uma biblioteca de jogos com perfis de recompilação conhecidos,
compilação local e cache dos módulos nativos. Tornar isso universal é um problema
adicional, principalmente de descoberta de código e cobertura das APIs.

Para preservar Linux/Vulkan, pausa controlada e snapshots, o caminho exige adaptar
o código gerado e a camada de execução. Para chegar primeiro a um jogo no Windows,
reutilizar também o runtime externo aproveita mais trabalho pronto, mas muda uma
parte substancial do produto planejado.

## O que o seu projeto pretende entregar

O [ROADMAP](../../ROADMAP.md) descreve um mini console: biblioteca navegável por
controle, jogo e UI no mesmo processo e device gráfico, retorno ao menu, Windows e
Linux, e finalmente Linux direto em KMS/DRM. Há requisitos de snapshots,
determinismo, baixo consumo e compatibilidade sem ajustes por título como estratégia
principal.

O plano já prevê código de CPU executando nativamente: o JIT traduz blocos para
instruções do host, e o hypervisor executa instruções do guest no processador. A
diferença principal da recompilação estática é fazer a tradução antes da execução e
permitir que um compilador C++ otimize o resultado. Isso não implica equivalência de
desempenho a um port escrito a partir dos fontes originais.

Na árvore consultada, formatos/VFS e infraestrutura já têm implementação; há código
de memória/MMU/bus, enquanto CPU, kernel e GPU ainda são esqueletos. A documentação
de memória e os checkboxes da Fase 2 não refletem toda a implementação encontrada.
Não foi feita uma auditoria de conclusão dessa fase. O rodapé “Próxima ação” também
manda fechar itens da Fase 1 já marcados como concluídos.

## Como o projeto externo funciona

O fluxo observado é:

1. `xbe_symbols` lê versões das bibliotecas e procura funções/dados por assinaturas,
   gerando a configuração específica do XBE.
2. `tools/recomp` decodifica x86 com Zydis, descobre funções e fluxo de controle e
   emite C++.
3. Clang/LLD compila o C++ e o liga aos serviços de execução. O mecanismo de release
   atual prevê gerar a DLL do jogo no PC do usuário a partir do XBE dele.
4. Durante o jogo, chamadas indiretas usam uma tabela de funções traduzidas;
   chamadas do kernel e bibliotecas substituídas chegam a implementações nativas.
5. O runtime gráfico usa Direct3D 12; o áudio usa XAudio2; controles usam SDL3.

Fontes: [README](https://github.com/TGP482/XboxXBERecomp/blob/99078b119b5d5e0d1d6afb1390e29437c0c984c0/README.md),
[build CMake](https://github.com/TGP482/XboxXBERecomp/blob/99078b119b5d5e0d1d6afb1390e29437c0c984c0/cmake/XbeRecomp.cmake),
[dispatcher](https://github.com/TGP482/XboxXBERecomp/blob/99078b119b5d5e0d1d6afb1390e29437c0c984c0/runtime/cpu/dispatch.cpp).

O código do jogo passa a executar como instruções do host. Ainda existem estado de
registradores x86, endereços de 32 bits, tratamento de chamadas e semântica de APIs
do Xbox. O termo “nativo” não elimina essas responsabilidades.

## O que pode ser aproveitado

| Componente externo | Utilidade no Obsidian | Adaptação principal |
|---|---|---|
| `tools/recomp/analysis.*` | Descoberta de funções, blocos e tabelas de salto | Entrada do loader e formato dos perfis |
| `tools/recomp/codegen*` | Tradução de x86, x87, MMX e SSE para C++ | Memória, ABI e pontos de saída/pausa |
| `tools/symbols` e `xdk/5849` | Reconhecimento das APIs substituíveis | Cobertura por versão e revisão do perfil |
| `runtime/cpu` | Contexto de registradores e despacho de funções | Relógio virtual, erros e integração com o scheduler |
| `runtime/kernel` e `runtime/xdk` | Serviços prontos e referência de comportamento | Modelo de objetos, threads, VFS e snapshots |
| `runtime/gpu` | Tradução de estados, shaders e texturas | Backend Vulkan e apresentação compartilhada |
| `harness` | Execução de teste no Linux | Integração com fixtures e testes do projeto |

É possível manter ferramentas C++ executadas fora do jogo e um frontend Rust.
Também é possível ligar código C++ ao Rust por uma ABI C. Isso preserva um produto
nativo, mas muda a exigência atual de implementação integral em Rust. Reescrever o
tradutor em Rust é outra opção, com esforço adicional de implementação e validação.

## Por que não é um backend de CPU intercambiável sem alterações

### Memória e MMIO

As macros `X86_MEM*` de
[`x86_ops.h`](https://github.com/TGP482/XboxXBERecomp/blob/99078b119b5d5e0d1d6afb1390e29437c0c984c0/runtime/cpu/x86_ops.h)
acessam diretamente `base + endereço_guest`. O runtime reserva um espaço virtual de
4 GiB. Isso não corresponde diretamente ao `Bus` do Obsidian, que distingue virtual,
físico e arena e aplica tradução, permissões e classificação de dispositivos.

Uma ponte apenas para chamadas do kernel não captura escritas de memória ou acessos
MMIO feitos pelo C++ gerado. Para manter o modelo atual, é necessário emitir acessos
por uma interface de memória compatível, ou desenhar um fastmem que preserve essas
propriedades. RAM pode ter um caminho rápido, mas acessos a dispositivos precisam
ser encaminhados corretamente; o ADR-0010 já rejeita faults como caminho comum de
MMIO no Windows.

### GPU e bibliotecas XDK

O ADR-0002 escolhe executar o D3D8 embutido no jogo e traduzir seu pushbuffer NV2A.
O SDK externo substitui APIs D3D8 e usa renderer D3D12. Adotar esse runtime muda a
decisão arquitetural; portar somente o tradutor mantém pendente a GPU do Obsidian.

AOT e GPU em nível de pushbuffer podem coexistir em princípio. Isso exigiria
traduzir também as rotinas não substituídas das bibliotecas e encaminhar seus acessos
ao hardware virtual. O funcionamento dessa combinação não foi demonstrado nesta
análise.

### Threads, pausa e estado persistente

[`threads.cpp`](https://github.com/TGP482/XboxXBERecomp/blob/99078b119b5d5e0d1d6afb1390e29437c0c984c0/runtime/kernel/threads.cpp)
usa threads do host. As chamadas traduzidas aninham frames na pilha nativa, e o
dispatcher implementa RDTSC a partir de `steady_clock`. Há estado global e objetos
do host. Isso difere do ADR-0004 e do scheduler determinístico planejado.

Salvar somente os registradores e a RAM não salva a pilha nativa nem os recursos do
runtime. AOT não impede snapshots, mas exige projetar continuação de execução,
pontos seguros de suspensão, contexto capturável e reconstrução dos recursos. Uma
FFI com `start/stop` por si só não oferece essas propriedades. Erros que chamam
`std::abort()` também precisam de tratamento antes de hospedar esse runtime no mesmo
processo da interface.

### Compatibilidade e compilação

Chamadas indiretas para código não descoberto encerram a execução com instrução de
adicionar o endereço à configuração e recompilar. O
[template de configuração](https://github.com/TGP482/XboxXBERecomp/blob/99078b119b5d5e0d1d6afb1390e29437c0c984c0/templates/game/config/game.toml)
prevê correções manuais de funções, switches, regiões de dados e hooks. O emissor
também pode gerar traps para instruções não implementadas.

Código criado ou modificado em tempo de execução requer uma estratégia adicional;
não há evidência nesta inspeção de um fallback JIT universal. Um fallback futuro
também precisaria compartilhar estado e memória e atravessar corretamente chamadas
entre código AOT e JIT.

O perfil deve ser associado à revisão exata do XBE, e não apenas ao nome do jogo. O
CMake externo já calcula SHA-1 para identificar a imagem esperada. Um cache do
Obsidian deveria incluir ainda a revisão do tradutor, do runtime e do perfil.

### Linux e experiência de console

O [harness Linux](https://github.com/TGP482/XboxXBERecomp/blob/99078b119b5d5e0d1d6afb1390e29437c0c984c0/harness/README.md)
é útil e demonstra alguma portabilidade: usa substitutos de apresentação e áudio e
possui renderer de referência em CPU. Ele não entrega o backend Vulkan/KMS necessário
ao appliance, nem comprova desempenho jogável nessa plataforma.

Compilar no primeiro uso é compatível com uma UX guiada por controle, mas exige
progresso, tratamento de erros, armazenamento e ferramentas de compilação. A meta de
“primeiro frame em menos de 2 s” deve distinguir instalação/compilação de lançamento
com cache pronto. Um único executável estático, sem ferramentas auxiliares e com
menos de 25 MB, precisaria ser reavaliado para essa distribuição.

## Candidato concreto: NFS Underground 2

A [verificação local dos formatos](../compat/formatos.md) registra oito bibliotecas
1.0.5849 no seu XBE. O SDK externo contém `xdk/5849/layout.toml` e
[`signatures.toml`](https://github.com/TGP482/XboxXBERecomp/blob/99078b119b5d5e0d1d6afb1390e29437c0c984c0/xdk/5849/signatures.toml).
Isso favorece um teste inicial: existe uma base de reconhecimento para a versão
declarada. Não significa que todas as assinaturas coincidam, que todos os símbolos
sejam encontrados ou que esse jogo já funcione. Def Jam não foi avaliado.

O documento de conformidade atual registra uma decisão de não analisar o código das
bibliotecas Microsoft embutidas no XBE. Uma prova de recompilação comercial deve
documentar a mudança desse procedimento e a origem do código reaproveitado, sem
confundir tradução local do binário com implementação limpa de APIs ou permissão de
redistribuição do conteúdo traduzido.

## Licenciamento do reaproveitamento

A [licença MIT](https://github.com/TGP482/XboxXBERecomp/blob/99078b119b5d5e0d1d6afb1390e29437c0c984c0/LICENSE)
autoriza uso, cópia, modificação e distribuição, inclusive comercial, exigindo manter
o aviso de copyright e a permissão nas cópias ou porções substanciais. Portar trechos
para Rust não elimina esses avisos. As dependências têm seus próprios termos; o
SDK fornece um `THIRD_PARTY.txt`, e os avisos de Zydis e demais componentes realmente
distribuídos também precisam ser conferidos.

Essa licença cobre o SDK. Ela não concede direitos sobre jogos ou bibliotecas
proprietárias presentes no XBE. Seguir o mecanismo de compilação local evita incluir
código traduzido do jogo na distribuição do SDK, sem representar uma conclusão
jurídica geral sobre todos os usos possíveis. A licença do próprio Obsidian segue
pendente no ADR-0007.

## Caminho recomendado de validação

1. **Prova isolada do tradutor.** Fixar esta revisão como dependência de experimento,
   construir `xbe_recomp`/`xbe_symbols` e validar geração e execução com um XBE de
   teste aberto. Conferir registradores, FLAGS, memória, chamadas e ponto de saída;
   C++ compilável sozinho não é aceite suficiente.
2. **Prova comercial delimitada no Windows.** Usar o NFSU2 local como candidato,
   medir símbolos ausentes/ambíguos, traps e correções necessárias. Com um perfil
   utilizável, verificar menu, gameplay, áudio, controles, arquivos de save e troca
   de XBE se aplicável. Registrar a revisão exata da mídia e o trabalho por jogo.
3. **Prova de integração.** Expor um módulo nativo com ABI versionada, erros
   recuperáveis e encerramento completo. Validar memória/bus, renderer compartilhado
   e suspensão antes de apresentar o módulo como backend equivalente ao atual.
   Se o caminho escolhido reutilizar o runtime externo sem essas adaptações,
   registrar explicitamente os requisitos relaxados.
4. **Prova da plataforma final.** Produzir frame Vulkan no Linux e medir custo de
   CPU/GPU/RAM e estabilidade no hardware-alvo. O harness headless não substitui
   esse teste. Comparar save states persistentes separadamente de simples pausa.
5. **Revisar o roadmap com os resultados.** Caso a prioridade passe a ser ports de
   jogos selecionados, propor AOT + perfis como caminho principal e rever os ADRs
   0002, 0003, 0004 e 0006 conforme as escolhas. Se as exigências originais forem
   mantidas, tratar AOT como experimento e preservar a infraestrutura de validação
   e um caminho de execução mais geral.

O investimento atual em formatos, VFS, configuração, relógio e diagnóstico continua
útil nos dois caminhos. O benefício provável da recompilação é reduzir tradução de
CPU durante o jogo e aproveitar um SDK já implementado. Compatibilidade,
desempenho no notebook e custo real de adaptação permanecem hipóteses até essas
provas.
