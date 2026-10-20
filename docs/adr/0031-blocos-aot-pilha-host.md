# ADR-0031 — Limitar a pilha host dos blocos AOT

Data: 2026-10-20. Estado: aceito após a primeira compilação do hello real.

## Problema observado

BSF/BSR (ADR-0030) destravou a tradução das 8725 instruções/blocos inicialmente
descobertos do nxdk hello. O projeto Rust gerado compilou em debug, mas abortou
ao executar com `STATUS_STACK_OVERFLOW` (`0xC00000FD`) no Windows.

O emissor colocava todas as operações e temporários dos milhares de braços de
despacho em uma função `run`. Em debug, a pilha nativa crescia com o código
emitido, embora a pilha guest já estivesse corretamente representada no bus.
Aumentar a pilha do processo esconderia esse crescimento e não corrigiria a
estrutura do código gerado.

## Decisão

Emitir uma função Rust privada por bloco, marcada `#[inline(never)]`. O
dispatcher escolhe um ponteiro de função e faz uma única chamada de bloco,
propagando seu resultado fora do match. Isso evita também criar temporários
de `Result`/`?` por braço no frame do dispatcher.

Um bloco retorna ao dispatcher antes do próximo ser chamado. CALL/RET guest
continuam operando em EIP/ESP e RAM; não chamam outros blocos recursivamente.
Não há continuação, pilha de jogo ou ponteiro host gravado na memória guest.
O ponteiro de função existe apenas no código host para selecionar Rust já
compilado, sem decodificação x86 durante a execução.

Limitar também blocos lineares a **32 instruções** (`MAX_BLOCK_INSTRUCTIONS`),
para que uma sequência enorme sem branches não produza outro frame enorme.
Aplicar esse particionamento somente ao programa final, **depois** da inferência
de destinos: a análise local conserva o alcance original de suas constantes.
Os novos inícios entram no despacho e na validação normal de destinos RX.

O orçamento continua medido em blocos, agora com até 32 instruções. Programas
sem controle indireto e blocos longos podem ceder mais cedo; isso é intencional.
REP continua com um elemento por passagem, e programas com despacho por início
de instrução mantêm sua granularidade. Custos guest, aposentadoria, faltas,
fronteiras HLE, HLT e INT3 permanecem os mesmos.

Snapshots continuam AOTC v6, sem novo estado. Regenerar projetos para obter o
novo formato de emissão; não interpretar custo de chamadas Rust como ciclos
físicos do Xbox. Desempenho e custo de compilação ainda não são garantias de FPS.

## Validação

Uma regressão compila e executa duas fixtures abertas numa thread host com
pilha solicitada de **128 KiB**: 1000 ADDs lineares e 1000 pares ADD/JMP.
Confere orçamento, resultado 1000, aposentadoria e Clock, sem aumentar a pilha
da aplicação. Um teste de análise confere o limite e a preservação de todas as
instruções da sequência longa.

A suíte completa do Rust gerado precisa continuar validando ABI, REP parcial,
faltas e snapshots, além dos scans. O total passa de 110 para **111 casos em
x86_64** (95 nos demais hosts).

## Resultado integrado inicial

Após separar funções, o hello real deixou de abortar por pilha e devolveu
`UnknownTarget { pc: 0x209D2, target: 0x19110 }`. O desassembly confirma CALL
indireto em `[EBP+0x0C]`, no caminho de inicialização por callback. Esse destino
não era inferido a partir de argumentos na pilha.

Usar o mecanismo já existente `--entry 0x19110`, com a mesma validação de imagem
e sem varrer bytes arbitrários. A análise passa a **8769 instruções e blocos**.
Não converter destinos desconhecidos em sucesso nem implementar decodificação
em execução. Novos callbacks/threads podem exigir entradas adicionais.

Com a raiz explícita, o projeto foi compilado e executado até o dispatcher
recusar **NtCreateMutant (192)**, retorno reportado `0x18CFA`, argumentos
`0x00028EEC, 0x00000000, 0x00000000`. O CALL está em `0x18CF4`; o PC reportado
é o endereço de retorno. O fonte fixado `threads/mtx_init.c` contém essa chamada.
O próximo trabalho é implementar handles/objetos e criação de mutant com estado
real, preservando os contratos do dispatcher e planejando espera/ownership.
Não retornar sucesso sem criar o objeto e escrever o handle guest.

Reprodução, escolhendo diretório novo/vazio:

```powershell
cargo run --offline -p xberecomp -- translate target/homebrew/hello.xbe --entry 0x19110 --out target/alpha-hello
cargo run --offline --manifest-path target/alpha-hello/Cargo.toml
```

O segundo comando ainda termina com erro de serviço não implementado, sem
framebuffer. Não é um homebrew funcional completo nem cobertura de todos os
destinos de threads. O catálogo permanece com 1/69 serviços implementados.

Logs locais ignorados: `target/scan-hello-stack-overflow.log` (reprodução anterior),
`target/scan-hello-unknown-target.log`, `target/scan-hello-translate.log` e
`target/scan-hello-run.log`. `target/scan-hello-project.txt` indica o projeto
usado na execução final desta sessão. Nenhum arquivo de mídia/BIOS foi usado.

## Evidência final

Aceite final dos ADRs 0030/0031: **262 testes do workspace e doctests**,
**111 casos do Rust gerado em debug e release** (release com `-D warnings`),
incluindo a regressão de pilha pequena e 262144 comparações físicas BSF/BSR.
Clippy global com `-D warnings`, fmt, links locais e diff passaram. O hello real
foi compilado e executado em debug, sem aumentar sua pilha host, até a recusa
NtCreateMutant. Essa recusa é o resultado esperado nesta referência.

Logs de verificação: `target/scan-workspace.log`, `target/scan-release.log`,
`target/scan-clippy.log` e `target/scan-ordinal.log`.

## Referências

- [Emissor](../../crates/xbox-recomp/src/emit.rs).
- [Particionamento final](../../crates/xbox-recomp/src/analysis.rs).
- [Regressão nativa](../../tools/xberecomp/tests/verification_blocks.inc).
- [BSF/BSR e tradução inicial](0030-bsf-bsr.md).
