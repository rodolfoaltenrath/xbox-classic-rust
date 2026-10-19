# ADR-0011: Arena compartilhável e a corrida entre CPU e GPU

- **Status:** aceito
- **Data:** 2026-10-19
- **Fase:** 2 (prepara a 5)

## Contexto

Duas decisões sobre a arena de 64 MiB têm de ser tomadas antes de escrevê-la, porque
mudá-las depois custa refatoração do subsistema inteiro.

**Primeira:** como criar a memória. Um `mmap` anônimo é o caminho óbvio e o mais
simples. Mas na Fase 5 a GPU precisa ler vértices e texturas que estão na RAM do guest,
e importar memória em Vulkan exige um objeto compartilhável — `dma_buf` no Linux, objeto
de seção no Windows. Memória anônima não se importa; só se copia.

**Segunda:** a thread de CPU escreve na arena enquanto a thread de GPU lê (ROADMAP
§5.2). Pelo modelo de memória do Rust isso é corrida de dados, e corrida de dados é
comportamento indefinido — mesmo que em x86-64 um acesso alinhado não rasgue na prática.

## Alternativas consideradas

### Criação da arena

1. `mmap` anônimo — simples; obriga a copiar tudo o que a GPU precisa, a cada frame.
2. **Objeto compartilhável** (`memfd_create` / objeto de seção) — permite importar no
   Vulkan e mapear em memória host-visible sem segunda cópia; um pouco mais de código
   específico de SO, que vai para `xbox-platform` de qualquer forma.

### A corrida

1. **Acesso atômico byte a byte** — correto pelo modelo; mata a performance do acesso de
   memória, que é a operação mais frequente do emulador.
2. **Duplicar a arena** (buffer duplo) — correto; +64 MiB de RAM no alvo que tem pouca,
   e sincronização por frame que o hardware real não tem.
3. **Aceitar a corrida**, com acesso por ponteiro cru e invariante documentado.

## Decisão

Arena criada como **objeto de memória compartilhável**, com o descritor guardado para a
Fase 5 importar.

Para a corrida, opção 3, com o invariante explícito: *a thread de GPU só lê faixa cuja
posse o título cedeu pelo protocolo do pushbuffer.* A sincronização real do Xbox não é o
modelo de memória da linguagem: são os semáforos e notifiers do NV2A, que o jogo usa
para saber quando a GPU terminou. É isso que ordena os acessos, e é isso que a Fase 5
tem de implementar corretamente.

## Consequências

- Positivas: a Fase 5 importa a arena sem reescrever a Fase 2; a thread de GPU existe
  sem refatoração; zero cópia no caminho de vértice e textura.
- Negativas / dívida aceita: **desvio consciente do modelo de memória do Rust.** Não
  é defensável como "correto"; é defensável como escolha informada, que todo emulador
  com thread de GPU faz, com o custo das alternativas medido e registrado acima.
- A corrida fica confinada à arena. Nenhuma outra estrutura compartilhada entre threads
  usa esse padrão: o resto usa canais e tipos atômicos.
- Miri vai acusar a leitura concorrente. A cobertura de Miri cobre o caminho
  monothread; a corrida é verificada por teste de integração com as duas threads e por
  comparação de hash de frame (Fase 9).

## Como reverter

Buffer duplo é a saída se a corrida se mostrar problema real em hardware fraco ou em
ARM (ordenação mais frouxa): custa +64 MiB e uma barreira por frame, sem mudar a API.
