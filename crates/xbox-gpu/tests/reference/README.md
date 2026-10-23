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
