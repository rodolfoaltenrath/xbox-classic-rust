# ADR-0009: Cinco espaços de endereçamento, com newtypes

- **Status:** aceito
- **Data:** 2026-10-19
- **Fase:** 2 (define a interface de CPU, GPU e kernel)

## Contexto

Um emulador manipula endereços de naturezas diferentes que têm o mesmo tipo de máquina.
No Xbox são cinco: virtual do guest, físico do Xbox, deslocamento na arena, endereço do
processo host e deslocamento em arquivo. Todos caberiam em `u32`/`usize`.

O caminho que torna a confusão fatal é real e o jogo verificado o usa: o título chama
`MmAllocateContiguousMemory` (recebe virtual), `MmGetPhysicalAddress` (recebe físico) e
escreve o físico num objeto DMA do NV2A, que lê sem passar pela MMU. Um virtual aceito
no lugar do físico faz a GPU ler o lugar errado, e o sintoma aparece como "textura
corrompida" três fases depois.

## Alternativas consideradas

1. **`u32` em tudo, com disciplina de nomes** (`va`, `pa`) — zero custo, zero
   ergonomia perdida; a garantia depende de atenção humana em cada chamada, para
   sempre.
2. **Newtypes sem conversão implícita** — a confusão passa a ser erro de compilação;
   custo de `.0` e de conversões explícitas.
3. Newtypes só na fronteira pública, `u32` por dentro — pior dos dois: a fronteira é
   justamente onde o erro não acontece.

## Decisão

Newtypes para `GuestVirt`, `GuestPhys` e `ArenaOff`; `HostPtr` nunca sai de `xbox-mem`;
`FileOff` continua `usize` em `xbox-formats`. Sem `From`, sem `as`, sem aritmética de
janela escondida: as únicas conversões são `Mmu::traduzir`, `Bus::classificar`,
`Arena::ponteiro` e `Xbe::va_to_offset`.

Em particular, a janela contígua `0x80000000` **não** é implementada como soma no bus,
e sim como entrada de tabela de páginas, para que a tradução tenha um caminho só.

## Consequências

- Positivas: elimina por construção a classe de bug mais cara de diagnosticar num
  emulador; a assinatura de cada função passa a documentar o que ela espera.
- Negativas / dívida aceita: ruído de `.0` e de construtores; alguma verbosidade em
  aritmética de endereço.
- O mapa do ROADMAP §3.2 estava misturando físico com janela virtual e foi corrigido em
  `docs/arch/memoria.md`.

## Como reverter

Trocar os newtypes por `u32` é mecânico (um `sed` e remover os construtores), mas
devolveria a classe de bug. Reverter só faria sentido se o custo ergonômico se mostrasse
insuportável em código quente — e nesse caso a saída melhor é `#[inline]` nos
construtores, não remover o tipo.
