# ADR-0044 — NOP multibyte e descoberta das entradas de thread

Data: 2026-10-21. Estado: aceito para a tradução das formas NOP de 16/32 bits.

## Resultado da retomada

O repositório chegou limpo em `94d222a`, após as entregas 0035–0043 feitas em
outro computador. O fluxo anterior tinha 9027 instruções/blocos e parava em
PsCreateSystemThreadEx (255). Revisar a criação antes de implementar o serviço
mostrou que as entradas da thread ainda não faziam parte da tradução AOT.

O pedido real tem StartRoutine `0x18FC0` e SystemRoutine `0x13C70`. São destinos
passados como argumentos da API, não chamadas diretas inferidas pelo analisador.
Acrescentá-los como raízes explícitas revelou, **antes da execução**, a recusa:

```text
recompilação em 0x00013C68: instrução ainda não suportada: Nop_rm32
```

O preenchimento de alinhamento aparece em um caminho alcançado pela descoberta;
isso não prova que a CPU guest o executou. Não varrer todo o XBE nem simular
criação de thread para contornar uma instrução sem tradução.

## Decisão e semântica

Baixar `iced_x86::Code::Nop_rm16` e `Nop_rm32` para a IR `Operation::Nop` já
existente. O formato `0F 1F /0` admite ModR/M, SIB e deslocamento, mas **não
emite acesso de memória**. Não baixar seu operando como load/store nem tentar
calcular um endereço que pudesse gerar falta.

Registradores, flags, memória e FS permanecem preservados. EIP avança pelo
comprimento completo, e a instrução consome um passo do perfil de tempo AOT,
independentemente de ter três ou nove bytes. Isso não modela latência física.
NOP com endereço de 16 bits também não exige cálculo de BX/SI.

Usar a IR existente preserva a inferência de constantes através do NOP e não
adiciona estado, snapshot, unsafe, dependência ou instrução privilegiada host.
`AOTC` continua v7, `AOTS` v1 e `KRNL` v6.

As validações de prefixo existentes continuam valendo: LOCK e REP/REPNE são
recusados; PAUSE não passa a ser suportado. O perfil atual de segmentos continua
limitado a ausência de override ou FS. Este avanço não declara suporte a todos
os prefixos redundantes ou formas não documentadas de NOP.

## Evidência de semântica

Três testes de análise cobrem comprimentos, limites de prefixo/truncamento e
preservação da descoberta de um CALL indireto através de NOP.

Três casos no Rust realmente emitido verificam:

- NOPs de um a nove bytes, preservação de registradores e 4096 combinações de
  flags, orçamento e cobrança por instrução, não por byte;
- operandos com endereço ausente, MMIO, wrap, FS sem página e endereço de 16
  bits, sem leitura, escrita ou registro de acesso em dispositivos;
- snapshot entre o bloco de NOPs e RET, replay e esgotamento do Clock sem efeito.

O conjunto gerado passa a **156 testes em x86_64** (140 nos demais hosts).

Aceite final em Windows/Rust 1.98.1: **357 testes workspace/doctests**, **156 casos
gerados em debug/release** (release com `-D warnings`), Clippy global com
`-D warnings`, fmt, diff e links locais passaram. O teste de análise foi repetido
após acrescentar a recusa explícita do segmento GS.

## Tradução e limite da execução

Com as seis raízes, a análise passa a **9454 instruções e blocos**:

```powershell
cargo run --offline -p xberecomp -- translate target/homebrew/hello.xbe --entry 0x19110 --entry 0x145E0 --entry 0x205E0 --entry 0x20889 --entry 0x13C70 --entry 0x18FC0 --out target/alpha-hello-thread-roots
cargo run --offline --manifest-path target/alpha-hello-thread-roots/Cargo.toml --target-dir target/recomp-hello-check
```

Escolher diretório novo/vazio. A build real foi compilada e executada em debug;
continua recusando **PsCreateSystemThreadEx (255)**, retorno `0x13BF1`, argumentos
`0x7000FE88, 0, 0x10000, 0x114, 0, 0x18FC0, 0x30008, 0, 0, 0x13C70`.
O catálogo permanece em **17/69 serviços**. Não há criação de thread implementada
por esta entrega, nem execução observada de seu corpo ou framebuffer.

## Contrato investigado para a próxima entrega

O nxdk declara PsCreateSystemThreadEx como stdcall/40; SystemRoutine tem dois
argumentos: StartRoutine e StartContext. Seu WinapiThreadStartup consulta
`KeGetCurrentThread()->TlsData`, configura o slot autorreferente, copia o template
TLS, zera o restante, registra FLS, chama StartRoutine e termina por ExitThread.
Não saltar diretamente para main para pular esse contrato.

O disassembly do hello.exe local confirma os bytes `0F 1F 84 00 00 00 00 00`
em `0x13C68`, após uma chamada indireta, e o prólogo em `0x13C70`. O startup lê
TlsData em **KTHREAD+0x28**, primeiro em `0x13C82` e depois em `0x13CBA`.
O ponteiro deve representar um slot de TLS guest válido, não um endereço host.

Ainda é necessário representar identidade/handle, pilha/TLS e metadados de thread,
contextos executáveis, espera/término e snapshot. Mutants/seções críticas ainda
usam INITIAL_THREAD; seus donos não podem permanecer fixos ao ativar uma segunda
thread. O Clock em State é autoritativo e não pode virar um relógio por contexto.
IRQL continua sendo estado da única CPU guest, independente de IF.

Essas são dependências verificadas no código, não um scheduler já implementado.

Logs locais ignorados: `target/thread-roots-translate.log`,
`target/thread-roots-hello-run.log`, `target/nop-analysis.log`,
`target/nop-workspace.log`, `target/nop-release.log`, `target/nop-clippy.log`.
Projeto real: `target/alpha-hello-thread-roots-20261021`. BIOS e mídia comercial
não foram usadas. O repositório não foi commitado ou enviado nesta retomada.

## Referências

- [Intel SDM, vol. 2B, NOP](https://www.intel.com/content/dam/www/public/us/en/documents/manuals/64-ia-32-architectures-software-developer-vol-2b-manual.pdf).
- [Assinaturas públicas nxdk fixadas](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/xboxkrnl/xboxkrnl.h).
- [WinapiThreadStartup](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/winapi/thread.c).
- [CRT e main_wrapper](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/pdclib/platform/xbox/crt0.c).
- [Análise](../../crates/xbox-recomp/src/analysis.rs), [testes de análise](../../crates/xbox-recomp/tests/nop.rs) e [testes gerados](../../tools/xberecomp/tests/verification_nop.inc).
