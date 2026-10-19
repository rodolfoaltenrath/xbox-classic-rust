# ADR-0013: Recompilação estática para Rust como direção principal

- **Status:** aceito — revisa a prioridade do ADR-0003
- **Data:** 2026-10-19
- **Fase:** 3-AOT (primeira prova executável)

## Contexto

O mantenedor escolheu implementar em Rust a tradução antecipada dos jogos e a camada
de compatibilidade, visando execução nativa. A avaliação do XboxXBERecomp mostrou
que reaproveitar diretamente seu runtime C++/D3D12 conflitaria com Linux/Vulkan e com
o estado capturável do Obsidian.

## Alternativas consideradas

1. Copiar o SDK C++ com FFI: aproveita mais implementação, mas mantém o runtime e o
   modelo de threads/pilhas nativas daquele projeto.
2. Implementar o JIT antes: mantém o plano anterior, sem validar a direção AOT pedida.
3. Analisar x86 em Rust, baixar para IR explícita e emitir blocos de Rust compilados
   por Cargo/rustc; manter o estado e as chamadas do guest fora da pilha nativa.

## Decisão

Opção 3. O crate `xbox-recomp` implementa a análise e a emissão; `xberecomp` gera um
projeto Cargo local e pode compilá-lo e executá-lo. `iced-x86` 1.21.0 (MIT, Rust) faz
somente a decodificação durante a análise. Não foi incorporado código-fonte do
XboxXBERecomp; a avaliação registra a referência arquitetural.

A execução gerada contém blocos com operações Rust já traduzidas, despachados por
EIP em fronteiras de bloco. Não há busca/decodificação de opcodes durante a execução.
Calls e returns usam a pilha na memória do guest, e não chamadas recursivas do host.
O orçamento é contado em blocos; instruções concluídas são contadas separadamente.
Nenhum desses números representa ciclos do Pentium III.

Todos os acessos à memória passam por `xbox-mem::Bus`. Código fica RX, dados RW e
seções executáveis graváveis são recusadas. Snapshots de CPU e bus preservam a
continuação em fronteiras de bloco. Código automodificável, TLS, kernel e outras
instruções ainda sem suporte produzem erros, sem fallback fictício ou NOP implícito.

O ADR-0001 (kernel próprio), o ADR-0004 (snapshots) e os ADRs de memória permanecem
válidos. A estratégia gráfica ainda não foi alterada por esta prova de CPU.

## Consequências

- AOT passa a ser a prioridade da Fase 3. Interpretador continua planejado como
  oráculo; JIT e hypervisor ficam adiados. Não existe fallback automático hoje.
- O protótipo exige Cargo/rustc na geração do executável. O projeto gerado referencia
  os crates locais; ainda não é pacote de distribuição independente ou cache de jogo.
- O mesmo crate reúne tradução e runtime no protótipo. Separá-los antes da
  distribuição evita levar o compilador como dependência de build de cada módulo.
- A meta de lançamento em menos de 2 s aplica-se futuramente a um módulo já compilado;
  instalação e compilação inicial precisam de orçamento separado.
- O primeiro aceite é uma fixture criada por nós. Não demonstra execução comercial,
  compatibilidade nxdk, desempenho no hardware-alvo ou fidelidade completa de x86.
- A BIOS não participa da implementação. Seu eventual uso como referência de
  validação deve ser registrado separadamente; o produto continua carregando XBE.

## Como reverter

O compilador e a ferramenta são crates isolados. As APIs do bus e de snapshot não
mudaram. É possível retomar os backends do ADR-0003 sem perder formatos e ferramentas
de teste. Aceitar um runtime de terceiros exigiria rever novamente memória,
continuações, apresentação e licenças.
