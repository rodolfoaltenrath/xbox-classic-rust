# ADR-0087 — Inventário do sample `mesh`

Data: 2026-10-26. Estado: aceito. PLANO 9.1. Sem emulação nova.

## Consumidor

`samples/mesh` do nxdk fixado (14d5ee9) desenha uma malha com textura,
iluminação por vértice e depth buffer. A referência pública é
`samples/mesh/screenshot.png` no próprio SDK. Os shaders foram compilados pelo
build do sample: `vs.inl` com cgc 3.1 no perfil vp20 (44 instruções, constantes
c[0..16]) e `ps.inl` com o texture shader `2D_PROJECTIVE` no estágio 0 e um
combiner `textura × cor`.

Build no Fedora 44 com o toolchain extraído em `~/.cache/obsidian/toolchain`
(clang/lld 22, bison, flex), exatamente como `docs/dev/homebrew.md` descreve.
`bash scripts/build-nxdk-sample.sh mesh homebrew-mesh` gerou
`target/homebrew-mesh/mesh-raw.xbe` e `mesh.exe`. O manifest do script só tem
hash de `Makefile`/`main.c`; `math3d.c`, `verts.h`, `texture.h` e os `.cg`
vêm da árvore fixada e limpa do SDK, conferida pelo próprio script.

`prepare-sample.sh`, `sonda.sh` e `run-alpha.sh` aceitam `mesh` (entrada
padrão `target/homebrew-mesh/mesh-raw.xbe`, 320 fatias).

## Bloqueios, em ordem

### 1. CPU: aritmética x87 (observado)

`bash scripts/sonda.sh mesh` para na tradução:
`recompilação em 0x0001269E: instrução ainda não suportada: Fmulp_sti_st0`
(log `target/samples/mesh-Jk0wfCeX.translate.log`). `ordinal-report` dá o
mesmo primeiro bloqueio.

A análise para na primeira recusa, então o resto do x87 foi inventariado
estaticamente na desmontagem do `mesh.exe` inteiro (`llvm-objdump`, superconjunto
do alcançável):

| Forma | Ocorrências | Estado |
|---|---|---|
| `fld`/`fstp` m32/m64, `fild` m64, `fldz`, `fld1` | 70 | já suportadas (ADR-0053) |
| `fmulp st(1), st` | 2 | **falta** |
| `fdivrp st(1), st` | 1 | **falta** |
| `fsqrt` | 3 | **falta** |
| `fsin`, `fcos` | 3 cada | **falta** |

Não há `fldcw`, `fnstcw`, `fnstsw`, `ldmxcsr` nem `stmxcsr`: o P8 (MXCSR) não
bloqueia, e a palavra de controle do RESET continua sendo o único modo usado.
Todo o SSE do binário (movss, mulss, addss, divss, subss, xorps, movaps,
ucomiss, cvtsi2ss, cvttss2si) já é aceito pela análise.

Fontes: Intel SDM vol. 1 cap. 8 (ambiente x87, arredondamento ao par em 64 bits
de mantissa no controle de RESET) e vol. 2 (FMULP, FDIVRP, FSQRT, FSIN, FCOS).
A pilha já guarda valores de 80 bits exatos (`runtime/x87.rs`), então
multiplicação, divisão e raiz podem ser exatas com aritmética inteira de 128
bits. FSIN/FCOS dependem da redução de argumento interna do processador; o
resultado do hardware não é corretamente arredondado e não há medição de
console aqui. A 9.2 precisa declarar esse recorte de precisão.

### 2. Kernel (não observável ainda)

79 imports, 45/75 serviços implementados (`target/homebrew-mesh/ordinal-report.txt`).
O caminho do pbkit é o mesmo do triangle, que roda com zero recusas, e o sample
só acrescenta `XGetTickCount` (dado `KeTickCount`, já publicado). Só a execução
dirá se outro serviço é alcançado.

### 3. PGRAPH (estático, a confirmar pela telemetria de métodos)

Métodos do `main.c` além do recorte SoftwareV1 (ADR-0068):

- textura no estágio 0: `TX_OFFSET/FORMAT` (formato `0x0001122a`),
  `TX_NPOT_PITCH`, `TX_NPOT_SIZE`, `TX_WRAP` (clamp), `TX_ENABLE`,
  `TX_FILTER`; estágios 1–3 desabilitados;
- `SET_SHADER_STAGE_PROGRAM` (2D_PROJECTIVE) e combiner `tex0 × col0`;
- desenho indexado por `INDEX_DATA` (o triangle usa só arrays);
- vertex program de 44 instruções com transformação por matriz, normalize e
  `pow(…, 64)`. Os opcodes exatos serão listados pelo interpretador ao recusar,
  porque o SoftwareV1 só aceita MOV/MUL/ADD/MAD/RCP;
- depth: `pb_erase_depth_stencil_buffer` e teste de profundidade.

Fontes previstas: cabeçalhos `nv2a_regs.h` e pbkit do nxdk fixado, xemu na
revisão já fixada no ADR-0056 e envytools, com o consumidor real decidindo cada
recorte.

## Consequências

A 9.2 implementa o x87 do bloco 1 com teste por instrução, e então a sonda
revela kernel e métodos. A 9.3 trata textura, depth, opcodes de VP e o
combiner com o recorte que a telemetria confirmar. Nada aqui reconhece título,
hash ou endereço.
