# ADR-0066: Inventário NV097 e recorte gráfico do triangle

- **Status:** aceito como inventário e escopo; sem efeitos gráficos implementados.
- **Data:** 2026-10-23
- **Fase:** 5 / PLANO 2.1
- **Base:** `7e51aa1`

## Contexto e observação

O frontend já consome comandos reais. A raiz do formatador 0x1E9B8, observada na
entrega anterior, foi incluída: 14 raízes/19391 blocos em
`target/homebrew2/pgraph-review-20261023`. O caso decimal do switch em 0x1E9B6
agora permite executar o texto e continuar o loop. A análise automática P5 não
foi alterada. scripts/sonda.sh aponta para esse artefato.

Sonda oficial: `target/sonda/triangle-20261023-012028.log`, 320 fatias,
226 chamadas/zero recusadas, 181280329 instruções/551550633 ciclos, 752 ms guest,
EIP=0x16D9B por orçamento. Há 282 chaves/42731 parâmetros: NV097=274/42642,
classe 39=2/2, classe 62=3/84, classe 9F=3/3. Incluem bindings de objeto. O desenho
é submetido 40 vezes: DRAW_ARRAYS=0x08000000, start=0 e count=9. Ainda há 170 MMIO
sem modelo e framebuffer habilitado/preto. Submissão não é renderização.

O [inventário completo em CSV](../compat/triangle-nv097-20261023.csv) registra
cada uma das 274 chaves NV097, nome/slot, contagem, primeiro/último parâmetro,
fonte primária e efeito planejado. Todos os endereços foram identificados em
headers/ranges públicos. Os registros são agregados; a tabela não afirma conter
todos os parâmetros intermediários ou um trace temporal completo.

## Fontes públicas fixadas

Revisão nxdk: `14d5ee97e73347c973f1f57b68b79ec08c9e77f2`:

- **N**: [nv_regs.h](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/pbkit/nv_regs.h)
  e [nv_objects.h](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/pbkit/nv_objects.h):
  offsets, campos, aliases e hints de performance.
- **P**: [pbkit.c](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/pbkit/pbkit.c),
  consumidores set_draw_buffer, pb_subprog, pb_gr_handler, pb_fill e pb_finished.
- **T**: [samples/triangle/main.c](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/samples/triangle/main.c),
  [vs.vs.cg](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/samples/triangle/vs.vs.cg)
  e [ps.ps.cg](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/samples/triangle/ps.ps.cg):
  nove vértices float3 de posição/cor, viewport e cor interpolada.

Revisão xemu: `03b988c9d76553b33ed712ba00822ffdf49f0e7d`:

- **X**: [pgraph.c](https://github.com/xemu-project/xemu/blob/03b988c9d76553b33ed712ba00822ffdf49f0e7d/hw/xbox/nv2a/pgraph/pgraph.c)
  e [methods.h.inc](https://github.com/xemu-project/xemu/blob/03b988c9d76553b33ed712ba00822ffdf49f0e7d/hw/xbox/nv2a/pgraph/methods.h.inc):
  semântica dos métodos e ranges. A referência mantém dúvidas explícitas sobre
  NOP com parâmetro não zero e o NSOURCE usado; não tratá-las como medição física.
- **V**: [vsh-prog.c](https://github.com/xemu-project/xemu/blob/03b988c9d76553b33ed712ba00822ffdf49f0e7d/hw/xbox/nv2a/pgraph/glsl/vsh-prog.c)
  e [nv2a_regs.h](https://github.com/xemu-project/xemu/blob/03b988c9d76553b33ed712ba00822ffdf49f0e7d/hw/xbox/nv2a/nv2a_regs.h):
  campos de instruções, registradores e formatos de vértices.

Só se extraem fatos de formato/consumidores dessas fontes. Nenhuma implementação
foi copiada. O backend a implementar será próprio, serial e limitado, com perfil
e precisão declarados; não é prova de bit-exatidão ou reset físico do Xbox.

## Alternativas e decisão

Ignorar as chamadas de software e aplicar CLEAR_SURFACE imediatamente escreveria
na base antiga dos descritores, potencialmente sobre a imagem XBE. Fazer o host
reconhecer PB_SETOUTER seria um atalho específico da biblioteca guest. Foi escolhido
modelar o mecanismo de trap/IRQ para que o próprio guest execute seus handlers,
e só então usar os descritores atualizados no clear.

O backend mínimo fica em xbox-gpu, sem dependência circular de xbox-mem. Um
adaptador de memória do Bus fornecerá leituras de PRAMIN/RAM e escritas físicas
verificadas. Estado gráfico, traps e scanout precisam ser salvos e migrados;
perfis legados não ganharão efeitos gráficos ao restaurar.

### Tarefa 2.2: surface, sincronização e clear

| Métodos / controle | Valor observado / consumidor | Fonte | Efeito mínimo escolhido |
|---|---|---|---|
| 0x0100 NO_OPERATION | 0xBAA/0xB2A e outros códigos de software; pb_subprog | P/X | zero é NOP; não zero captura trap e sinaliza IRQ, suspendendo o consumidor até ACK do guest |
| 0x0110 WAIT_FOR_IDLE | zero | P/X | ordenação do consumidor serial; sem antecipar conclusão de trap |
| 0x0180 / 0x0194 / 0x0198 | handles 2 / 9 / 10 | N/P/X | guardar instâncias resolvidas de notifier/color/zeta |
| 0x0200 / 0x0204 | 640×480 | N/P/X | clip/origem de surface |
| 0x0208 SET_SURFACE_FORMAT | 0x128: cor A8R8G8B8, zeta Z24S8, pitch, uma amostra | N/X | aceitar esse recorte; variantes não interpretadas recusam |
| 0x020C SET_SURFACE_PITCH | 0x0A000A00 | N/P/X | pitch color/zeta 2560 bytes |
| 0x0210 / 0x0214 | zero | N/P/X | offsets relativos aos respectivos descritores DMA |
| 0x1D8C / 0x1D90 | clear Z/stencil/cor; também PARAMETER_A/B no protocolo de software | N/P/X | estado único dos valores, acessível pelo handler nos aliases MMIO |
| 0x1D94 CLEAR_SURFACE | flags 3 e 0xF0 | N/P/X | clear depth/stencil e canais BGRA pedidos, com validação de toda a faixa antes dos stores |
| 0x1D98 / 0x1D9C | máximos inclusivos 639/479, ou sub-retângulos de texto | N/P/X | recorte inclusivo, preservando pixels/canais externos |
| PGRAPH INTR/EN, NSOURCE, TRAPPED_ADDR/DATA, CTX_SWITCH1, FIFO, PARAM_A/B | consultas/ACK do pb_gr_handler | P/X/V | estado/IRQ/trap coerentes; nenhum código PB_* reconhecido pelo host |
| PMC_INTR_0 + INTA | mesma linha GPU já usada pelo vblank | P/X/V | propagar PGRAPH habilitado sem perder PCRTC |
| PCRTC_START | escrito pelo handler de flip guest | N/P/T | scanout físico validado, integrado à captura de framebuffer; sem reescrever o modo HLE silenciosamente |

O NOP de software usará um perfil público de compatibilidade baseado em P/X,
registrando a incerteza sobre a ativação física dessa extensão e o NSOURCE exato.
Não basta gerar um sucesso genérico. O handler deverá ler a classe/método/dados,
limpar a pendência e atualizar PRAMIN por instruções guest reais. O backend não
recebe um tratamento especial para os números PB_SETOUTER/PB_FINISHED.

O clear preto de pb_fill é explícito em T. O texto também usa clears pequenos;
a contagem total de pixels pode refletir esses retângulos. A validação deve
incluir RAM escrita pelo clear e seu retângulo/cor, não só ausência de diagnóstico.

### Tarefa 2.3: vértices e rasterização

| Métodos / faixa | Evidência | Fonte | Efeito mínimo escolhido |
|---|---|---|---|
| 0x019C / 0x01A0 | handle 3, DMA linear | N/P/X | fontes de arrays de vértices |
| 0x1720..0x175C | atributos 0 e 3 em 0x20E000/0x20E00C | N/T/X | offsets e seleção DMA |
| 0x1760..0x179C | 0x1832 para posição/cor: F32×3, stride 24 | N/T/X | atributos ausentes com defaults declarados; tipos não suportados recusam |
| 0x1E9C / 0x1EA0 / 0x1E94 | início zero, modo programável | N/T/X | cursor/início/modo do vertex program |
| 0x0B00..0x0B7C | oito instruções, quatro DWORDs por instrução | T/X/V | upload com cursor; decode de MOV/MUL/ADD/MAD e RCP observados |
| 0x1EA4 / 0x0B80..0x0BFC | constantes a partir de 96 para viewport | N/T/X | matriz/constantes, sem reconhecer o sample por hash |
| 0x17FC SET_BEGIN_END | 5 / 0 | N/T/X | lista de triângulos e término |
| 0x1810 DRAW_ARRAYS | 0x08000000, nove vértices | N/T/X | buscar e transformar os nove vértices; rasterizar por interpolação de cor |
| depth/cull/color mask e combiners necessários ao passthrough | depth LEQUAL=0x203, cull habilitado, máscara 0x01010101 | N/T/X | honrar o subconjunto observado; variantes fora do perfil recusam ou permanecem explicitamente sem efeito conforme o inventário |

A sonda de programação anterior ao primeiro reset preservou o prefixo do
pushbuffer: `target/sonda/triangle-shader-probe-20261023.log`, retired=60177082,
GET=0x18C814. A leitura host foi somente observacional. O dump local
`target/pfifo-evidence/initial-pushbuffer.bin` e sua decodificação mostram oito
instruções, registradores temporários e outputs posição/cor; o shader CG confirma
viewport e divisão por W. Não se copiará esse bytecode para o runtime: a execução
será por campos/opcodes, testada com programas sintéticos próprios.

## Métodos sem efeito neste recorte

A coluna planned_effect do CSV enumera os demais métodos. Iluminação, materiais,
texturas, fog, texgen, hints de swath/compressão/oclusão e estados de outros modos
não se tornam uma GPU completa por terem sido recebidos. Suas chamadas continuam
na telemetria. Quando um desses estados alterar a variante efetivamente desenhada,
será necessário interpretar o estado ou recusar a variante; não aplicar uma cor
fixa ou descartar uma operação que o recorte afirme atender.

Classes auxiliares 39/62/9F permanecem relatadas sem renderização nesta etapa.
Gamma/filtros e tiling real continuam provisórios existentes. Perfis/limites novos
entrarão no ROADMAP §16.1 junto da implementação, com condição de saída.

## Consequências, reversão e verificação

Este inventário não muda emulação ou snapshots. Define as dependências mínimas
para 2.2/2.3 e evita escrever RAM antes de tratar os descritores via IRQ guest.
Reverter custa apenas retirar a decisão/documentação e escolher outro recorte
com evidência. Não há pixels ou compatibilidade comercial comprovados.

Gate Rust 1.99: GATE OK 538 testes, target/gate/20261023-012445.log.
A sonda oficial de 14 raízes e o CSV são a evidência de métodos usados; os testes
do gate validam a base existente, não implementações gráficas futuras.
