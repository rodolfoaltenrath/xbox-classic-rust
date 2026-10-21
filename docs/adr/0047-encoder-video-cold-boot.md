# ADR-0047 — Consulta do encoder e estado inicial de vídeo

Data: 2026-10-21. Estado: aceito para cabo padrão e cold boot sem imagem herdada.

## Contrato investigado

O repositório chegou limpo em `a4a2eb7`. O hello já montava D: e parava em
AvSendTVEncoderOption (2), opção 6, retorno 0x12769. O SDK aberto define essa
opção como GET_SETTINGS. XVideoListModes usa os bits de cabo/padrão e flags
para escolher o modo; não basta responder sucesso sem preencher Result.

Fontes públicas do nxdk fixadas em
`14d5ee97e73347c973f1f57b68b79ec08c9e77f2`:

- [video.h](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/hal/video.h): opções 6/9, máscaras e flags;
- [video.c](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/hal/video.c): consulta, escolha do modo, blanking e alocação;
- [xboxkrnl.h](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/xboxkrnl/xboxkrnl.h): ABI stdcall/16 e retorno VOID, cabo padrão = 1.

## Perfil implementado

AvSendTVEncoderOption aceita RegisterBase = 0xFD000000, endereço guest simbólico
da NV2A, sem dereferenciar esse argumento nem acessar hardware do host.

- **GET_SETTINGS (6), Param=0:** escreve em Result uma ULONG com padrão de TV
  e flags derivados dos bytes autoritativos da EEPROM da sessão, mais cabo
  padrão (AV_PACK_STANDARD=1). Aceita NTSC-M, NTSC-J ou PAL-I e os bits públicos
  de proporção, resoluções e 50/60 Hz; campos sem modelo recusam. O valor padrão
  é **0x00480101**: NTSC-M/60 Hz, preferência 480p da EEPROM e cabo padrão. O
  SDK só considera modos HD quando o cabo é HDTV, portanto essa preferência não
  transforma o cabo padrão em HDTV. A EEPROM não é modificada.
- **VIDEOENABLE (9), Param=0/1 e Result nulo:** guarda o estado lógico da saída.
  Param é blanking: 1 desabilita, 0 habilita. O estado inicial é desabilitado.
  Ainda não há scanout, vblank, GPU ou apresentação consumindo esse estado.
- **AvGetSavedDataAddress (1), stdcall/0:** o perfil cold boot não herda
  framebuffer, portanto devolve NULL. É uma invariante declarada do perfil;
  AvSetSavedDataAddress ainda não está implementado e não pode alterar essa
  condição. Isso não libera memória, inventa framebuffer ou recupera imagem host.

São **25/69 serviços**, com variantes limitadas: a opção 6 destravou a opção 9
com Param=1, retorno 0x12DAD; em seguida surgiu AvGetSavedDataAddress, retorno
0x12B45. Cada etapa foi medida no módulo real antes de ampliar o perfil.

Result de GET_SETTINGS exige quatro bytes completos de RAM gravável, inclusive
desalinhados; nulo, RO, MMIO, wrap e faixa incompleta recusam antes do store.
Nenhuma resposta NTSTATUS é simulada para uma API VOID: erro de argumento ou
variante deixa a chamada pendente. EAX=0 é apenas a convenção VOID do runtime;
o resultado real está na memória. Base/opção/parâmetro sem suporte recusam sem
alterar o encoder. A continuação HLE é capturada antes de efeitos, inclusive
quando Result sobrepõe argumentos/retorno.

## Estado e snapshot

VideoState guarda somente enabled. O padrão/flags continuam na EEPROM; cabo
padrão e ausência de framebuffer herdado são invariantes do perfil versionado.
**KRNL v9 acrescenta AVST v1**, com ID StandardCableColdBootV1=1 e booleano
canônico de enabled. Versões/perfis desconhecidos, booleanos inválidos e
truncamento recusam. KRNL v1–v8 migram para o encoder inicial desabilitado,
sem herdar o estado da sessão de destino. EEPR preserva a fonte dos settings.
AOTS v2/AOTC v7/KMUT v3 e as demais seções permanecem; não há relógio adicional.

Oito testes de kernel cobrem padrões/flags, guardas, desalinhamento, falhas,
blanking, cold boot e snapshot/migração. Três casos no Rust realmente emitido
cobrem a ABI stdcall/VOID, CPU/flags/Clock preservados, dados retornados, replay,
EEPROM restaurada, migração e Result sobre o endereço de retorno.
Logs: `target/video-tests.log`, `target/video-workspace.log`,
`target/video-native-release.log`, `target/video-clippy.log`.

Aceite Windows/Rust 1.98.1: **380 testes workspace/doctests e 168 casos nativos
debug/release** (152 fora de x86_64; release com `-D warnings`), Clippy global
com `-D warnings`, fmt, diff e links locais aprovados.

## Evidência real e próximo bloqueio

As nove raízes anteriores continuam produzindo **10676 instruções/blocos**;
não foi acrescentada ISA nem descoberta nova nesta entrega. Reprodução em saída
nova/vazia:

```powershell
cargo run --offline -p xberecomp -- translate target/homebrew/hello.xbe --entry 0x19110 --entry 0x145E0 --entry 0x205E0 --entry 0x20889 --entry 0x13C70 --entry 0x18FC0 --entry 0x18C90 --entry 0x139B0 --entry 0x14F14 --out target/alpha-hello-encoder
cargo run --offline --manifest-path target/alpha-hello-encoder/Cargo.toml --target-dir target/recomp-hello-check
```

O hello passa por consulta de settings, blanking e consulta de imagem anterior;
para em **MmAllocateContiguousMemoryEx (166)**, retorno **0x12C01**, argumentos
`0x12C000, 0, 0x7FFFFFFF, 0x1000, 0x404`. É o pedido de **1.228.800 bytes**
para o framebuffer de 640×480×4, com alinhamento 4 KiB e RW|WRITECOMBINE.
A memória ainda não foi alocada, e nenhuma imagem foi produzida.

Teste local adicional confirmou settings=0x00480101 em 0x27D38, saída
desabilitada, D: e thread ativa 2, **9777 retired e 38 atendimentos** nessa
fronteira. Snapshot integral restaurou bytes idênticos. Evidência:
`target/video-hello-effect.log` e teste local ignorado
`target/alpha-hello-crt-case-20261021/tests/video_startup.rs`.

Próximo trabalho: modelar alocação fisicamente contígua e sua relação com o
endereço guest/alias físico usado pelo vídeo. Não reutilizar a reserva virtual
com frames fragmentados fingindo contiguidade. Depois, atender a programação
do modo/MMIO e apresentar o framebuffer. Outras opções de encoder, preservação
de framebuffer, AvSetDisplayMode, IRQ/vblank e scheduler geral continuam
pendentes. BIOS e mídia comercial não foram usadas.
