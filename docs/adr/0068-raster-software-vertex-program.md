# ADR-0068 — Arrays, vertex program e triângulos em RAM

Data: 2026-10-23. Estado: aceito no perfil público provisório P18.
Escopo: tarefa 2.3 do PLANO, Fase 5 do ROADMAP.

## Contexto e fontes

[ADR-0066](0066-inventario-pgraph-triangle.md) inventaria as 274 chaves NV097;
[ADR-0067](0067-pgraph-clear-traps-scanout.md) entrega a sincronização necessária
para que os descritores de cor/zeta sejam válidos. Agora os DRAW_ARRAYS precisam
buscar os dados reais e executar o programa enviado, produzindo pixels na RAM.

Pins e fontes primárias:

- nxdk `14d5ee97e73347c973f1f57b68b79ec08c9e77f2`,
  [sample triangle](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/samples/triangle/main.c),
  [vertex shader CG](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/samples/triangle/vs.vs.cg),
  [pixel shader CG](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/samples/triangle/ps.ps.cg),
  [nv_regs.h](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/pbkit/nv_regs.h):
  float3 posição/cor, stride 24, nove vértices, matriz de viewport enviada como
  constantes e combiner final com RGB=D=diffuse e alpha=G=diffuse alpha.
- xemu `03b988c9d76553b33ed712ba00822ffdf49f0e7d`,
  [pgraph.c](https://github.com/xemu-project/xemu/blob/03b988c9d76553b33ed712ba00822ffdf49f0e7d/hw/xbox/nv2a/pgraph/pgraph.c),
  [vsh-prog.c](https://github.com/xemu-project/xemu/blob/03b988c9d76553b33ed712ba00822ffdf49f0e7d/hw/xbox/nv2a/pgraph/glsl/vsh-prog.c),
  [nv2a_regs.h](https://github.com/xemu-project/xemu/blob/03b988c9d76553b33ed712ba00822ffdf49f0e7d/hw/xbox/nv2a/nv2a_regs.h):
  campos dos tokens, uploads de quatro DWORDs por instrução/constante,
  136 instruções/192 constantes, máscaras e modos de arrays/surface/depth/cull.
- Khronos, [OpenGL 1.5](https://registry.khronos.org/OpenGL/specs/gl/glspec15.pdf),
  rasterização de polígonos: amostras e interpolação são uma referência pública
  para a escolha software. Não demonstram a precisão física do NV2A.

Extraem-se formatos/semântica; a implementação do raster e do interpretador é
própria. A captura de oito instruções citada no ADR-0066 é dado de teste, nunca
uma chave para identificar o programa no backend.

## Decisão

Novo perfil `SoftwareV1` em xbox-gpu, sobre o clear/trap do ADR-0067:

- Métodos 0x19C/1A0 guardam instâncias DMA de leitura resolvidas; 0x1720..175C
  guardam offset/seleção DMA, 0x1760..179C formato/stride. F32 de 1–4 componentes;
  atributos desabilitados recebem (0,0,0,1) neste perfil. Cada acesso respeita
  alinhamento, limite inclusivo DMA e RAM física; stride zero mantém o atributo.
- Upload 0xB00..B7C usa lane módulo quatro e cursor de programa, avançando na
  quarta palavra. Constantes 0xB80..BFC têm cursor separado. Máscaras preservam
  uploads parciais; temporários lidos antes de definição e constantes não
  carregadas recusam. Início/modo/cursor vêm dos métodos 0x1E94..1EA4.
- O interpretador lê os tokens: MAC MOV/MUL/ADD/MAD, ILU RCP, mux de
  temporário/atributo/constante, swizzle, negação e máscaras. R12 compartilha
  posição; FINAL termina o programa. Exige posição e diffuse completos, finitos,
  W positivo. Pareamento, indexação relativa, outros opcodes e saída para
  constantes recusam. Não há comparação por nome, endereço ou hash de shader.
- BEGIN/END aceita lista TRIANGLES, DRAW_ARRAYS interpreta start e count+1.
  Cada pacote deve conter triângulos completos neste recorte. São buscados e
  transformados todos os vértices antes de produzir qualquer escrita guest.
- O raster usa posições de janela que o programa já calculou; nenhuma matriz
  host substitui o vertex program. Arestas inteiras com oito bits fracionários,
  centro do pixel e regra top/left; barycentrics f64 e cores com correção de W.
  Float32 nas operações do VP, sem FMA explícita; quantização de canal/depth por
  arredondamento ao inteiro mais próximo. Isso define o perfil software, **não**
  bit-exatidão medida do chip. Guard band de ±1048576 e até 16 milhões de
  avaliações de pixel por pacote, sobre surfaces limitadas pelo ADR-0067.
- Honra cull CW/CCW e FRONT/BACK/BOTH, depth LESS/LEQUAL/ALWAYS com máscara,
  Z24S8 fixed (stencil preservado; Z-perspective limitado a W exatamente 1), máscara BGRA e união de oito window clips
  inclusivos. Stencil ativo exige ALWAYS/KEEP; polígonos FILL; combiner diffuse
  passthrough. Fog, alpha test, blend, textura, logic op e offset de polígono
  ativos recusam. Configurações não suportadas nunca recebem sucesso gráfico.

Os pixels de cada draw são resolvidos em cópias locais de cor/zeta validadas;
só depois do pacote inteiro válido são publicados na RAM exclusiva. Erros de
DMA, programa, configuração e overflow não deixam draw parcial nem contadores
avançados. Máscaras, padding e stencil permanecem. Estatísticas distinguem
pacotes, triângulos submetidos e fragmentos aprovados.

Outros métodos continuam na telemetria com efeito não modelado. A tabela de
inventário é histórica: controles de stencil/window/combiner necessários à
validação do perfil foram acrescidos ao recorte executado nesta tarefa.

## Persistência e migração

PGRF passa de v1 para **v2**, com perfil 2 SoftwareV1 e seção interna **PRND v1**:
registros ordenados/limitados, cursores, programa e máscaras de upload,
constantes com máscaras de upload e contadores. BUS0 mantém v9 porque o layout externo não mudou e a
subseção tem sua própria versão. PGRF v1/perfil 1 migra para ClearV1 sem raster;
perfil 0 continua sem PGRAPH. Nenhuma sessão antiga ganha um backend novo ao
restaurar. Regravar os perfis migrados mantém essa escolha.

## Alternativas, provisórios e condição de saída

Raster fixo que ignorasse o programa foi rejeitado. Executar shaders em API
host antes de um backend de referência verificável adicionaria dependências e
dificultaria o diagnóstico de RAM. Este recorte permite quadro real em software
com precisão declarada e testes; não representa suporte gráfico geral.

P18 registra formatos/modos/opcodes limitados, defaults e precisão software.
Sua saída exige novos perfis com comparação contra hardware/capturas e extensão
de shader/combiner/raster, preservando SoftwareV1 e seu replay. P17 continua
registrando a incerteza física do trap de software.

## Validação

`tests/reference/triangle-4x4.ppm` foi calculada analiticamente: amostras e pesos
explicitados no README, seis pixels RGB sobre fundo preto. O teste golden
compara os 16 pixels gerados por arrays + programa + DRAW_ARRAYS, e alpha/depth
são conferidos separadamente. Não se regenera a referência pelo raster testado.
Testes adicionais cobrem cull/depth/máscaras, limites e recusa sem publicação,
replay de upload/BEGIN, migração ClearV1 e os oito opcodes/tokens observados com
matriz identidade e outra matriz que muda a geometria.

Gate final: **556 testes**, target/gate/20261023-020429.log, Rust/Cargo 1.99.0;
230 casos gerados debug/release (target/plan-23-native-release.log).

Sonda oficial `bash scripts/sonda.sh triangle 320`,
`target/sonda/triangle-20261023-020000.log`: 282 chamadas/zero recusadas,
241241306 instruções/305861656 ciclos (417 ms guest), orçamento em 0x16C96,
165 MMIO sem modelo. Dez DRAW_ARRAYS, 30 triângulos submetidos e 4608000
fragmentos aprovados. Framebuffer **305922 pixels coloridos/479 linhas**,
acima dos 1542 pixels/32 linhas de clear+texto do ADR-0067. Cor do clear
**0x00000000**. Regiões das três cores: **R=4977, G=4998, B=4977** (canal
>=192 e outros <=32), além dos intermediários interpolados.

Probe real `target/plan-23-real-raster-probe.log` confirma a mesma contagem,
scanout **0x210000**, snapshot integral byte a byte e mais 20 fatias com estado
idêntico após restauração. Artefato de pixels reais:
`target/pfifo-evidence/triangle-software.ppm` e conversão PNG para inspeção.
A inspeção mostra triângulo RGB, fundo interpolado e texto Triangle Demo/Frames.
Nenhuma imagem pré-pronta ou desenho host substitui RAM/vertex program.
