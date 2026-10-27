# ADR-0089 — PGRAPH SoftwareV1 para o mesh: índices, textura e combiners

Data: 2026-10-26. Estado: aceito. PLANO 9.2/9.3, provisórios P17/P18.

## Contexto

Com o x87 do ADR-0088, o `mesh` executa com zero recusas, mas nenhum triângulo
aparece. A telemetria (`target/sonda/mesh-20261026-220157-QsTArM0Z.log`) mostra
que os desenhos chegam por `INDEX_DATA` (0x1800, 5832 palavras em 54 pares
BEGIN/END), sem modelo. Ao modelar o desenho indexado, o SoftwareV1 recusa em
sequência o combiner (não é o passthrough do triangle) e o vertex program. Os
valores que o mesh programa foram lidos da telemetria, sem inferência por nome:

- combiner: um estágio, cor `0xC8C40000`/`0x40` e alfa `0xD8D41010`/`0x40`
  (V0 ← T0 × V0, mapeamento SIGNED_IDENTITY), final `D = V0`, `G = V0.a`;
- textura do estágio 0: `0x0001122A` (`LU_IMAGE_A8R8G8B8`, 2D, um nível, contexto
  DMA B), retângulo 256×256, pitch 1024, endereçamento `0x00030303`
  (clamp-to-edge) e filtro `0x04074000` (MIN/MAG de convolução 2D, kernel
  gaussiano 3×3); shader stage 0 em PROJECT2D;
- vertex program de 44 instruções (cgc vp20): MOV, MUL, ADD, MAD, **DP3, MIN,
  MAX, RSQ, LIT** e RCP, com saída o[9] (TEX0);
- depth LEQUAL com escrita e `SET_CONTROL0 = 0x110001` (Z_PERSPECTIVE), culling
  de faces traseiras, stencil ALWAYS/KEEP.

## Fontes

Perfil público do xemu na revisão fixada no ADR-0056
(`03b988c9d76553b33ed712ba00822ffdf49f0e7d`):

- `pgraph/glsl/psh.c`: mapeamentos de entrada e escalas de saída dos combiners,
  `ab`/`cd`/soma com clamp em [−1, 1], ordem leitura→escrita por estágio,
  `r0.a = t0.a`, final `D + mix(C, B, A)` e `G`; profundidade com
  Z_PERSPECTIVE = W com interpolação de perspectiva; texturas lineares com
  coordenadas em texels; convolução gaussiana 3×3 de amostras `GL_LINEAR`.
- `pgraph/glsl/vsh-prog.c`: DP3, MIN, MAX, RSQ (`inversesqrt(abs(x))`), LIT com
  potência limitada a ±(128 − 1/256); `pgraph/glsl/vsh.c`: `vtxD0` com clamp
  em [0, 1] por vértice.
- `pgraph/gl/constants.h` e `nv2a_regs.h`: campos de filtro, modos de
  endereçamento e bits de `SET_CONTROL0`.

São a semântica de um emulador, não medição de console; ficam declaradas em P18.

## Decisão

1. **INDEX_DATA:** dois índices de 16 bits por palavra (o baixo primeiro),
   acumulados durante o BEGIN TRIANGLES e desenhados de uma vez no END, com cada
   índice transformado uma só vez. Recusa fora de BEGIN, lista incompleta no END
   e mais de 2^20 índices pendentes. Draw recusado não escreve pixels e devolve a
   lista intacta. Snapshot `PRND` v2 guarda os índices pendentes; v1 migra sem
   índices.
2. **Vertex program:** DP3, MIN, MAX, RSQ e LIT, mais a saída o[9]. O `pow` do
   LIT usa log2/exp2 próprios em `f64` (só operações IEEE básicas), sem a libm do
   host. A recusa de temporário indefinido passa a olhar só as faixas que entram
   no resultado: xyz no DP3, x nos escalares do ILU e as faixas escritas nas
   operações componente a componente.
3. **Combiners gerais** (`render/combiner.rs`): até 8 estágios e o final,
   decodificados uma vez por draw. Recusa produto escalar, mux, blue-to-alpha,
   C0/C1, fog, especular e V1+R0/E·F. O passthrough do triangle é o caso
   particular em que o estágio descarta tudo.
4. **Textura** (`render/texture.rs`): `LU_IMAGE_A8R8G8B8` 2D de um nível,
   clamp-to-edge, filtros nearest, linear ou gaussiano 3×3, com MIN igual a MAG
   (LOD sem modelo). Os texels são copiados no início do draw. Coordenadas com
   perspectiva e divisão por q (PROJECT2D). Recusa qualquer outro formato, modo,
   filtro ou estágio.
5. **Profundidade:** com Z_PERSPECTIVE, depth = 1/Σ(bᵢ/wᵢ); sem ele, o z
   interpolado de antes. A recusa "W = 1" sai.
6. **Orçamento do raster:** soma das áreas das caixas envolventes por draw
   (64 Mi pixels), em vez de largura × altura × triângulos, que impedia draws
   com centenas de triângulos pequenos.

Contextos DMA de textura 0x184/0x188 são resolvidos como os de vértices.

## Evidência

- Sonda mesh/320 (`target/sonda/mesh-20261026-224137-aMy2E7hf.log`): 281
  chamadas, **zero recusas**, `GPU raster: chamadas=54 triângulos=3888
  fragmentos=179834`. Quadro exportado em `target/mesh-phase9/guest.png`: letras
  "nxdk" em 3D com a textura metálica e o texto "Mesh Demo / Frames: 8".
- As faces frontais saem escuras no quadro 8, e isso é coerente: a luz fica em
  (0, 140, 0), acima do objeto, e com t ≈ 0,13 s a rotação é ~−0,1 rad, então as
  faces frontais ficam quase perpendiculares à luz. O `screenshot.png` do SDK é
  do quadro 348 a 40 fps (t ≈ 8,7 s, rotação ≈ −0,55 rad); a comparação no mesmo
  ângulo é o PLANO 9.3.
- Triangle/320 sem mudança: 278/0, 305922 pixels em 479 linhas, R4977/G4998/B4977,
  27 triângulos e 4147200 fragmentos. O PPM exportado tem SHA-256
  `1c2d8fc58e5852926ae8c54377174aea5b5a6f21308db2ad804b9c60dd7c280c`, idêntico à
  referência do ADR-0069: o caminho passthrough e a nova regra de profundidade
  (W = 1) preservam a imagem byte a byte.
- Testes novos (`crates/xbox-gpu/tests/mesh.rs`): golden de textura 4×4 e golden
  de depth com quads sobrepostos, comparadas a PPMs escritos à mão
  (`tests/reference/README.md`); índices equivalentes a arrays; recusas de
  INDEX_DATA sem efeito; snapshot no meio do BEGIN com índices pendentes;
  migração `PRND` v1 → v2 e recusa de índices corrompidos. Unitários de
  combiner (passthrough, mesh, mapeamentos, recusas), textura (nearest, linear,
  gaussiano) e LIT/pow.

## Comparação com a referência pública (PLANO 9.3)

`samples/mesh/screenshot.png` é uma captura de janela do macOS: o quadro 640×480
ocupa a área (66,5; 65,5)–(832,5; 638,5), em escala de ~1,2×, e passou por
gerenciamento de cor (o fundo `0x202020` aparece como 43). Cores absolutas não
são comparáveis. **Métrica declarada** (`scripts/comparar-referencia.py`): a
referência é reamostrada para 640×480 (BOX); em cada imagem, a máscara do objeto
são os pixels cuja luminância difere da mediana do fundo por mais de 10, fora do
texto (x < 260, y < 100) e de uma margem de 8 px (cantos da janela). Medem-se o
IoU das máscaras, a caixa envolvente, o centroide e o brilho relativo
(luminância média do objeto menos a do fundo).

O ângulo do objeto depende do tempo (`−π/4 · t`), e a referência não informa t.
Foram exportados quadros em quatro tempos de guest:

| Fatias | t guest | IoU | Caixa (referência 144,190–492,300) | Centroide (ref. 317,9; 254,1) | Brilho rel. (ref. 30,7) |
|---|---|---|---|---|---|
| 5000 | 8228 ms | 0,684 | 167,178–473,300 | 324,3; 252,6 | −2,3 |
| 5280 | 8696 ms | 0,767 | 153,182–484,300 | 320,0; 253,7 | 19,5 |
| **5500** | **9063 ms** | **0,956** | **144,190–491,300** | **317,2; 254,6** | **28,4** |
| 5800 | 9563 ms | 0,695 | 137,205–496,290 | 307,4; 251,0 | 24,4 |

Em t = 9063 ms a silhueta coincide com a referência: IoU 0,956, caixa a 1 px e
centroide a 0,7 px. O brilho relativo fica 2,3 níveis abaixo, compatível com a
curva de cor aplicada à captura, que clareia o fundo de 32 para 43. Inspeção
visual lado a lado (`target/mesh-phase9/comparacao-f5500.png`): mesma geometria,
textura, iluminação de cima e faces laterais escuras; mudam só o contador de
quadros/FPS do texto e o tom do fundo. Na execução longa (5800 fatias) foram
6035 chamadas, zero recusas e 122256 triângulos. Na janela X11 do Linux
(`tools/verificar-janela apos`, 5500 fatias) a captura tem zero diferenças RGB
em relação à exportação.

## Consequências

P17/P18 crescem: combiners gerais e textura linear entram no perfil software,
com as recusas acima. Continuam fora: texturas swizzled/comprimidas, mipmaps e
LOD, outros modos de endereçamento, blend, fog, alpha test, produto escalar e
mux nos combiners, e precisão física do NV2A.
