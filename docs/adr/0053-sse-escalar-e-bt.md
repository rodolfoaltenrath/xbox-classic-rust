# ADR-0053 — SSE escalar, x87 de carga/armazenamento, BT/BTS/BTR/BTC e WBINVD

Data: 2026-10-22. Estado: aceito; aritmética x87 pendente.

## Contexto

O segundo sample do nxdk (`triangle`, compilado no WSL com o mesmo SDK fixado do hello)
traduz até uma instrução de ponto flutuante: o AOT só entendia inteiros. Contagem no
executável inteiro (30 466 instruções, 80 mnemônicos distintos), via `llvm-objdump`:

| Família | Instruções (ocorrências) |
|---|---|
| SSE escalar | `movss` 246, `mulss` 9, `addss` 8, `subss` 7, `divss` 5, `cvttss2si` 8, `cvtsi2ss` 6, `ucomiss` 1 |
| SSE empacotado | `xorps` 4, `movaps` 4 |
| x87 | `fild` 4, `fstp` 4, `fldz` 3 |

A contagem é do binário, não do caminho executado: diz o tamanho do problema, não quais
instruções serão de fato alcançadas. Seguindo a regra do projeto, entra o que foi
**observado** mais o que compartilha decodificação e semântica com ele sem ambiguidade (como
as 16 condições de `SETcc` e `CLC`/`STC`/`CMC` entraram de uma vez).

Depois do SSE a tradução parou em `BTR r/m32, r32`, que é inteira, e `BT` entrou junto.

## Decisão

### Estado da CPU

`State` ganha `xmm: [[u32; 4]; 8]` e `mxcsr: u32` (inicial `0x1F80`). Os registradores são
guardados como **bits crus**, não `f32`: igualdade e snapshot exatos, inclusive NaN, e
nenhuma conversão escondida ao copiar 128 bits. O snapshot `AOTC` passa à versão 8; v1–v7
migram para o estado de RESET (XMM zerado), sem inventar valores.

### O que é emitido

- `MOVSS`: carga de memória zera as três palavras altas; registrador→registrador preserva
  as altas; armazenamento grava só 4 bytes.
- `MOVAPS`/`MOVUPS`: 128 bits. `MOVAPS` com endereço fora de múltiplo de 16 devolve
  `Error::MisalignedSse` (o #GP do hardware) **antes** de qualquer efeito.
- `ANDPS`/`ANDNPS`/`ORPS`/`XORPS`, com o mesmo requisito de alinhamento na memória.
- `ADDSS`/`SUBSS`/`MULSS`/`DIVSS`/`SQRTSS`/`MINSS`/`MAXSS`. As quatro operações e a raiz do
  Rust em `f32` são IEEE-754 com arredondamento ao mais próximo, que é o que o MXCSR de
  RESET manda fazer. `MINSS`/`MAXSS` têm regra própria para NaN e zeros iguais (devolvem a
  **origem**), implementada à parte porque `f32::min` do Rust difere.
- `CVTSI2SS`, `CVTTSS2SI` e `CVTSS2SI`. A conversão para inteiro **não** usa `as i32`, que
  satura: NaN e fora do intervalo devolvem o "indefinido inteiro" `0x80000000`, como o
  hardware. Saturar seria um resultado errado sem erro algum.
- `UCOMISS`/`COMISS`: ZF/PF/CF codificam a relação (não ordenado liga os três); OF, SF e AF
  zeram.
- Leituras de memória acontecem antes de qualquer escrita; escritas de 128 bits usam
  `escrever_bytes`, que valida o intervalo todo antes de tocar a RAM.

### BT/BTS/BTR/BTC

Só a forma de 32 bits. Com destino em registrador ou com imediato, só os 5 bits baixos do
deslocamento contam. Com memória **e** registrador o deslocamento é com sinal, em bits, e o
endereço anda `(deslocamento >> 5) * 4` bytes — o bit pode estar fora da palavra nomeada.
Só CF muda; ZF/SF/OF/AF/PF ficam como estavam (a ISA os deixa indefinidos, e preservar é a
escolha que não inventa valor).

### x87 (carga e armazenamento)

`FILD m16/m32/m64`, `FLD m32/m64/m80`, `FLDZ`, `FLD1`, `FST`/`FSTP m32/m64` e `FSTP m80`.
Cada registrador guarda **80 bits exatos** (mantissa de 64 bits e sinal/expoente), e não um
`f64`: converter na pilha mudaria o resultado de qualquer cadeia com mais de um
arredondamento. Só a saída para 32/64 bits arredonda — uma vez, ao par mais próximo, com
estouro virando infinito e subfluxo virando subnormal ou zero. Inteiro de 64 bits entra
sem perda (cabe na mantissa), então `FILD m64` + `FSTP m64` arredonda uma única vez, como
no hardware.

A pilha detecta sobre e subfluxo (`Error::FpuStack`, o #IS) **antes** de qualquer efeito:
o `FSTP` calcula, escreve e só então retira, e o `FLD` lê a memória antes de empilhar.
O snapshot guarda `TOP`, as tags e os oito registradores.

`WBINVD` vira uma operação explícita sem efeito: a memória guest é coerente. `INVD`, que
descartaria dados, segue recusado.

## Provisório declarado — P8

O que **não** está modelado, e que um título pode notar:

| Item | Estado |
|---|---|
| flags de exceção do MXCSR | nunca atualizadas |
| máscaras do MXCSR | todas mascaradas; nenhuma exceção SSE é entregue |
| modo de arredondamento | sempre "mais próximo"; `LDMXCSR`/`STMXCSR` seguem recusados |
| FTZ/DAZ | desligados |
| bits de NaN gerados por operação inválida | vêm do host (em x86-64 coincidem com o SSE) |
| precisão dupla, inteiros empacotados, `SHUFPS`/`UNPCK*` | recusados na análise, com o nome da instrução |
| aritmética x87 (`FADD`, `FMUL`...), `FLDCW`, `FNSTCW`, `FNSTSW`, `FXCH`, comparações | recusadas na análise (P9); palavras de controle/estado e exceções x87 não existem |

Condição de saída: um título real que dependa de qualquer um deles. Registrar, não
antecipar.

## O que ainda falta para o `triangle`

- aritmética x87 e as palavras de controle/estado, quando um caminho executado as usar.
- GPU: `MmClaimGpuInstanceMemory` e a programação do NV2A.
- Interrupção de vblank, que o `triangle` provavelmente espera.

## Verificação

- 5 testes unitários das funções de semântica (`runtime/sse.rs`): truncamento vs
  arredondamento ao par, fora do intervalo, `MINSS`/`MAXSS` com NaN e zeros, flags de
  comparação, alinhamento.
- 6 testes no Rust **gerado** executando bytes x86 escritos à mão (`support/sse.rs`),
  cobrindo cada instrução, a regra de lanes do `MOVSS`, o #GP sem efeito em XMM0 e o
  snapshot de XMM/MXCSR.
- 4 testes de BT/BTS/BTR/BTC (e um WBINVD) no Rust gerado, incluindo deslocamento negativo e a
  preservação de ZF.
- 7 testes de x87 no Rust gerado (arredondamento único de 2^53+1, FILD de 16/32/64 bits,
  FLDZ/FLD1, m80, FST vs FSTP, ida e volta por 80 bits, pilha vazia e cheia, snapshot) e 8
  unitários da pilha e das conversões.
- Contador de casos gerados: 181 → 198 (165 → 182 fora de x86-64).

## Como reverter

`Operation::Sse` e `Operation::BitTest` são aditivas. Removê-las devolve a recusa por
"instrução ainda não suportada". O campo `xmm`/`mxcsr` do snapshot v8 seria lido e
descartado se a versão voltasse a 7.

## Resultado no `triangle`, e o que mais mudou no caminho

Dez raízes de tradução (nove delas descobertas por tentativa e erro, uma a uma, pelo erro
"alvo/endereço não recompilado"):

```
translate target/homebrew2/triangle-raw.xbe --entry 0x22700 --entry 0x22C10 --entry 0x1E110
  --entry 0x2A0E0 --entry 0x2A389 --entry 0x1D7B0 --entry 0x22AC0 --entry 0x22790
  --entry 0x14670 --entry 0x1EA44
```

O binário vem de `make -C $NXDK/samples/triangle` no WSL, com o mesmo SDK fixado do hello.
Traduzido e executado com `--fatias 4000`, ele **passa pela inicialização de vídeo** (modo
`0x4010101`, a mesma rampa de gamma do hello) e entra na inicialização do pbkit. O próximo
bloqueio real é um serviço, não uma instrução: `NtCreateEvent` (ordinal 189), chamado de
`0x180C1`. Eventos do kernel são o caminho para as interrupções (o pbkit os usa com o
DPC/ISR da GPU), então esse é o ponto onde vblank por interrupção deixa de ser hipotético.

Três mudanças pequenas vieram do mesmo caminho:

- `WBINVD` (`0x18F1E`): operação explícita sem efeito, porque a memória guest é coerente.
  `INVD`, que descartaria dados, segue recusado.
- `MmAllocateContiguousMemoryEx` com alinhamento **0**: o nxdk o passa na alocação de 0x20
  bytes da inicialização de vídeo. Antes era recusado como variante não implementada; agora
  0 vira o piso de página (4 KiB). Só o zero foi promovido: 16, 0x3000 e outros valores
  abaixo de uma página ou que não são potência de dois seguem recusados, porque nenhum título
  os usou. O teste que recusava `(alinhamento = 0)` foi trocado por um que exige alocação
  idêntica à de 4096.
- O erro de orçamento esgotado passou a dizer o `EIP` em que a execução parou. Sem isso, "47
  chamadas atendidas" parecia um travamento; era o limite de 64 fatias (cada serviço de
  kernel consome uma fatia), e `--fatias` o ampliou.

**O que isto não prova.** O `triangle` ainda não desenhou nada: 0 pixels. Ele não tem GPU
para desenhar. Nada aqui afirma compatibilidade além de "executa até `NtCreateEvent`".
