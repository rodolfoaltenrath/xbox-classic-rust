# ADR-0010: Caminho seguro de memória primeiro; fastmem medido depois

- **Status:** aceito — **revisa o ADR-0005**
- **Data:** 2026-10-19
- **Fase:** 2 (fastmem passa para a 3b)

## Contexto

O ADR-0005 afirmou que o acesso seguro custa "~15 instruções" contra "~3" do fastmem e
concluiu que fastmem deveria ser o caminho padrão desde a Fase 2. A afirmação nunca foi
medida. Antes de escrever o subsistema, medi no host do mantenedor.

### O que a medição mostrou

Custo por acesso de 32 bits, 50 milhões de acessos por medição:

| Working set | direto | + limites | + MMU e limites |
|---|---|---|---|
| 32 KiB (L1) | 0,60 ns | 2,07 ns | 2,34 ns |
| 256 KiB (L2) | 1,29 ns | 2,51 ns | 3,14 ns |
| 8 MiB (L3) | 12,47 ns | 13,15 ns | 17,98 ns |
| 64 MiB (RAM) | 21,26 ns | 22,16 ns | 22,11 ns |

E o custo de uma falha de página tratada (VEH no Windows): **11,5 µs**, contra 0,3 ns de
uma escrita normal. A 60 fps, um milissegundo paga 86 falhas.

Duas conclusões:

1. a checagem triplica o custo **no regime de cache** e desaparece quando o acesso vai à
   RAM. O ganho real do fastmem depende da fração de acessos do guest que fica em cache
   — número que **não existe** antes do JIT existir;
2. **MMIO por falha de página é inviável**, ao menos no Windows. O desenho original do
   fastmem ("região não mapeada → falha → despacho") não serve para o caminho comum.

Nota: a primeira versão da sonda usou acesso aleatório em 64 MiB, deu empate entre os
três caminhos e quase produziu a conclusão oposta — "checagem é grátis". Era benchmark
ruim: working set desse tamanho mede latência de memória, não custo de checagem.

## Alternativas consideradas

1. **Seguir o ADR-0005** e implementar fastmem na Fase 2 — entrega o caminho rápido
   antes; constrói a parte mais perigosa do subsistema (unsafe + manipulador de sinal +
   recuperação dentro de código gerado) sem número que justifique, e com um mecanismo de
   MMIO que a medição mostrou inviável.
2. **Abandonar fastmem** — tentador pelo empate no regime de RAM; errado, porque no
   regime de cache a diferença é de 3,4×.
3. **Caminho seguro na Fase 2, fastmem na Fase 3b**, com a API já preparada para os
   dois e teste diferencial obrigatório entre eles.

## Decisão

Opção 3. A Fase 2 entrega arena, MMU, bus, classificação de região e dispositivos stub,
tudo em Rust seguro exceto dois blocos na arena. O fastmem entra na Fase 3b, quando o
JIT permitir medir o que falta, e o MMIO passa a ser resolvido por classificação em
tempo de tradução, não por falha de página.

## Consequências

- Positivas: a Fase 2 fica verificável por Miri de ponta a ponta; o fastmem passa a ser
  decidido por número e não por intuição; evita-se construir um mecanismo de MMIO que a
  medição já condenou.
- Negativas / dívida aceita: o emulador roda mais devagar até a Fase 3b. Como até lá não
  há jogo executando, o custo prático é zero.
- O manipulador de falha, quando existir, não decodificará instrução: o JIT manterá uma
  tabela de recuperação `PC do host → (registrador, tamanho, tipo)`.
- O ADR-0005 continua válido no que diz sobre **o desenho** do fastmem (reserva de
  4 GiB, teorema de não-escape, slowmem sempre disponível); muda o **quando** e o
  mecanismo de MMIO.

## Como reverter

Implementar fastmem antes do JIT é possível a qualquer momento — a API não muda. O que
não se deve reverter é a exigência de medir antes de adotar como padrão.
