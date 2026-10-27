# Referência do triângulo sintético

`triangle-4x4.ppm` é uma imagem PPM RGB textual criada analiticamente, sem ser
regenerada pelo raster testado. Vértices em (0,0), (4,0), (0,4), cores R/G/B.
As amostras ficam em (x+0.5,y+0.5); para o interior os pesos são
`(1-(x+y+1)/4, (x+0.5)/4, (y+0.5)/4)`. Multiplicar por 255 e arredondar ao
inteiro mais próximo produz as cores listadas. A diagonal é uma aresta
bottom/right excluída; há seis pixels cobertos. O restante é preto.

O teste compara os 16 pixels de RAM produzidos por arrays + vertex program +
DRAW_ARRAYS com esses bytes RGB e verifica separadamente alpha e depth/stencil.
É referência do perfil software declarado, não captura de hardware NV2A.

# Textura e depth do perfil do mesh (ADR-0089)

`texture-4x4.ppm`: textura `LU_IMAGE_A8R8G8B8` 4×4 cujo texel (x, y) vale
R = 60·x, G = 60·y, B = 128, desenhada num quad que cobre o alvo 4×4 com
coordenadas iguais às de tela. Em nearest, o centro do pixel (x+0,5, y+0,5) cai
no texel (x, y); o combiner `tex0 × v0` com `v0` branco devolve o texel. Os
valores do arquivo são essa fórmula, escrita à mão.

`depth-overlap-4x4.ppm`: três quads com teste LEQUAL e escrita de depth, na
ordem vermelho (z = 10, alvo inteiro), azul (z = 5, colunas 0–1) e verde
(z = 20, alvo inteiro). O azul vence à esquerda, o vermelho à direita e o verde,
desenhado por último, perde em todos os pixels. Os quads são enviados por
`INDEX_DATA`.
