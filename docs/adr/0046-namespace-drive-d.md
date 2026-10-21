# ADR-0046 — Namespace guest mínimo para a montagem de D:

Data: 2026-10-21. Estado: aceito para o perfil D: → diretório de CdRom0.

## Bloqueio e fontes

O repositório chegou limpo em `63ad4ee`. O hello parava no JMP indireto
0x14E86 → 0x14F14, um case do formatador usado pelo CRT. Acrescentar a raiz
`--entry 0x14F14` às oito anteriores traduziu **10676 instruções/blocos**, sem
mudança de ISA ou inferência. A execução revelou NtOpenSymbolicLinkObject (203),
retorno 0x143B3, argumentos 0x4FD28/0x4FD0C.

O caminho público do nxdk confirma a sequência: automount_d_drive consulta se
D: existe; quando ausente, obtém o caminho de XeImageFileName, remove o nome do
executável e cria o link `\??\D:` para esse diretório. A consulta fecha o handle
se o link já existir. Referência fixada em
`14d5ee97e73347c973f1f57b68b79ec08c9e77f2`:

- [automount_d.c](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/nxdk/automount_d.c);
- [mount.c](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/nxdk/mount.c);
- [xboxkrnl.h](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/xboxkrnl/xboxkrnl.h);
- [ntstatus.h](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/xboxkrnl/ntstatus.h).

## Decisão e perfil

O dispatcher mantém um namespace guest inicialmente sem D:. Implementa:

- **NtOpenSymbolicLinkObject (203), stdcall/8:** OBJECT_ATTRIBUTES Xbox de 12
  bytes, RootDirectory nulo e somente OBJ_CASE_INSENSITIVE (0x40). A consulta
  de D: ausente retorna STATUS_OBJECT_NAME_NOT_FOUND sem alterar a saída ou
  consumir handle. Quando existe, publica um handle real do objeto.
- **IoCreateSymbolicLink (67), stdcall/8:** aceita o nome `\??\D:` e destino
  ASCII de até 260 bytes sob `\Device\CdRom0`, inclusive seu diretório raiz.
  Guarda uma cópia do destino; criação duplicada retorna
  STATUS_OBJECT_NAME_COLLISION e preserva o objeto anterior.
- **NtClose (187), stdcall/4:** fecha handles de link, sem desmontar D:. Handle
  inválido/fechado retorna STATUS_INVALID_HANDLE; handles de mutant/thread são
  reconhecidos e recusados como variantes sem ciclo de vida implementado.

São **23/69 serviços** com variantes limitadas. O alvo atual usa primeiro a
consulta negativa e depois a criação; os testes exercitam também open/close
positivos e seus erros. O contador não significa suporte completo a essas APIs.

OBJECT_STRING tem Length/MaximumLength de 16 bits e Buffer de 32 bits. A leitura
usa Length, sem exigir NUL, valida a estrutura/faixa inteira como RAM e não
acessa MMIO. String vazia, Length > MaximumLength, NUL interno, truncamento ou
comprimento fora do limite recusam. Nomes fora de D:, atributos adicionais,
nomes relativos, destinos fora de CdRom0, componentes `.`/`..`, slash e colon
no destino não estão no perfil. Os serviços exigem Passive.

Há no máximo 256 handles simultâneos de link. O contador é compartilhado com
mutants e a filha guest, sem colisão ou reutilização. Todas as leituras e
validações precedem a publicação da saída; ela pode sobrepor os argumentos ou
o endereço de retorno. Session usa a continuação capturada antes dos efeitos.
Links não são aguardáveis: a espera identifica o tipo e recusa explicitamente.

**É um objeto de namespace guest.** Não cria link no host nem conecta a mídia à
VFS. O destino usado pelo CRT deriva do XeImageFileName atual, ainda declarado
como `\Device\CdRom0\default.xbe`. Arquivos, outras unidades,
NtQuerySymbolicLinkObject, IoDeleteSymbolicLink, desmontagem e fechamento de
mutants/threads continuam pendentes. Nenhuma BIOS ou mídia comercial foi usada.

## Snapshot e validação

**KRNL v8 acrescenta SYML v1** com destino e handles ordenados. AOTS v2, AOTC v7,
KMUT v3, THRD/TCXT v1 e as demais seções permanecem. A carga confere tamanho,
destino, contagem, handles alinhados/ordenados/únicos, contador compartilhado e
ausência de colisão com mutant/thread. KRNL v1–v7 migram com namespace vazio,
sem herdar o destino da sessão usada para restaurar.

Sete testes de kernel cobrem ausência, criação/cópia, colisão, case-insensitive,
open/close, tipos, limites, ponteiros/strings inválidos, alias, IRQL e snapshot.
Três casos no Rust realmente emitido verificam o fluxo do nxdk, ABI/flags/Clock,
replay com handle aberto, migração e saída sobre o retorno. Logs locais:
`target/links-tests.log`, `target/links-workspace.log`,
`target/links-native-release.log`, `target/links-clippy.log`.

Aceite Windows/Rust 1.98.1: **372 testes workspace/doctests e 165 casos nativos
debug/release** (149 fora de x86_64; release com `-D warnings`), Clippy global
com `-D warnings`, fmt, diff e links locais aprovados.

## Evidência real e reprodução

```powershell
cargo run --offline -p xberecomp -- translate target/homebrew/hello.xbe --entry 0x19110 --entry 0x145E0 --entry 0x205E0 --entry 0x20889 --entry 0x13C70 --entry 0x18FC0 --entry 0x18C90 --entry 0x139B0 --entry 0x14F14 --out target/alpha-hello-drive-d
cargo run --offline --manifest-path target/alpha-hello-drive-d/Cargo.toml --target-dir target/recomp-hello-check
```

Usar saída nova/vazia. O hello passou pela montagem de D: e chegou à chamada
de vídeo **AvSendTVEncoderOption (2)**, retorno **0x12769**, argumentos
`0xFD000000, 6, 0, 0x27D38`. Esse serviço ainda não está implementado.

Teste local adicional no Rust real confirmou D: = `\Device\CdRom0\`, thread
ativa 2, TLS self-slot inicializado pelo guest em 0x4FEEC, **9498 instruções
concluídas e 35 chamadas atendidas** na fronteira de vídeo. O snapshot integral
nessa fronteira restaurou CPU, memória, namespace e pai estacionado com bytes
idênticos. Evidência: `target/links-hello-effect.log` e teste local ignorado
`target/alpha-hello-crt-case-20261021/tests/drive_mount.rs`.

Próximo trabalho: investigar a opção 6 no caminho público de vídeo, implementar
o comportamento exigido e seguir os diagnósticos. Execução completa, scheduler
geral, controle integrado e framebuffer seguem pendentes.
