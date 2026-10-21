# ADR-0049 — Modo de vídeo HLE e leitura do framebuffer

Data: 2026-10-21. Estado: aceito para NTSC-M 640×480, A8R8G8B8, cabo padrão.

## Contrato

O repositório chegou limpo em `7e48ef9`. O hello já alocava/zerava o framebuffer
e parava em AvSetDisplayMode (3), stdcall/24, retorno 0x12CA4. O pedido era
`0xFD000000, 0, 0x04010101, 0x12, 0xA00, 0x50000`.

O nxdk repete a chamada enquanto Step retornado não for zero. Depois guarda o
ponteiro virtual do framebuffer, ajusta flicker/soften, programa a rampa gamma
por MMIO e habilita a saída. A tabela pública associa 0x04010101 a 640×480/60 Hz
NTSC-M e cabo padrão; 0x12 corresponde a A8R8G8B8. Fontes fixadas em
`14d5ee97e73347c973f1f57b68b79ec08c9e77f2`:

- [video.c](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/hal/video.c);
- [xboxkrnl.h](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/xboxkrnl/xboxkrnl.h).

## Implementação e limites

AvSetDisplayMode aceita a base NV2A guest, Step=0, modo 0x04010101 e formato
0x12. A EEPROM precisa informar NTSC-M. Pitch deve ser múltiplo de quatro,
entre 2560 e 16384 bytes; framebuffer físico alinhado a quatro bytes.
O intervalo completo `pitch * 480` deve caber em **um recurso CMEM registrado**,
inclusive quando começa em um offset interno. Endereço virtual no lugar de PA,
faixa fora do recurso, overflow ou geometria inválida recusam antes do efeito.
Passos/modos/formatos e padrões de TV sem modelo recusam como variantes.

O efeito é um DisplayMode explícito em VideoState: ID, formato, pitch e PA.
Dimensões/refresh são invariantes desse modo único. Todas as leituras, validação
de posse/MMU/bitmap e consistência da EEPROM ocorrem antes de publicar o modo.
O serviço preserva enabled e devolve Step=0, concluindo a transação HLE em uma
chamada. Não simula a sequência interna de programação dos registradores NV2A.
O catálogo passa a **27/69 serviços**, com variantes limitadas.

Dispatcher::framebuffer(&Bus) retorna uma visão emprestada da RAM configurada,
por DMA físico, com metadados de modo e enabled. Cada row(y) contém somente os
640 pixels visíveis, excluindo padding de pitch; linhas fora de 0..480 retornam
None. Bytes são B,G,R,A em little-endian. A visão também existe com saída
desabilitada, para inspeção; isso não significa imagem apresentada. Nenhum
pixel é inventado ou copiado para um framebuffer independente.

Os campos do descritor não permitem construção externa sem validação. A visão
empresta o Bus e impede escrita/retomada mutável da CPU durante seu uso. Não há
unsafe novo, thread de renderização ou alteração de Send/!Sync da arena. A
apresentação futura poderá copiar as linhas, liberando o empréstimo antes de
retomar o guest.

## Snapshot e validação

**AVST v2** acrescenta indicador canônico de modo e seus quatro campos. AVST v1
migra com modo ausente, preservando enabled e limpando o modo da sessão de
destino. KRNL continua v10: a sequência de suas seções não mudou. AOTS v2/AOTC
v7/CMEM v1 e as demais seções permanecem.

A carga valida geometria/formato e cruza modo, EEPROM e CMEM antes de publicar
o dispatcher; a restauração de Session ainda confere Bus/MMU/frames. A visão
de framebuffer também recusa recurso/mapa inconsistente. Inspeção por DMA e
consulta de metadados não marcam A/D. Guardar modo sem uma alocação compatível,
ou apontá-lo para recurso insuficiente/EEPROM incompatível, é erro de snapshot.

Seis testes de kernel cobrem BGRA/padding/offset, limites e variantes sem efeitos,
preservação do blanking, reconfiguração, snapshot/corrupção e migração de AVST v1.
Três casos no Rust realmente emitido verificam stdcall/Step, CPU/flags/Clock,
pixels escritos pelo guest, replay e recusa sem publicar modo. Logs locais:
`target/display-tests.log`, `target/display-workspace.log`,
`target/display-native-release.log`, `target/display-clippy.log`.

Resultado: **394 testes workspace/doctests**, **174 casos nativos em debug e
release** (158 nos demais hosts), Clippy global com `-D warnings`, fmt e diff
limpos. O release do projeto emitido também foi compilado com `-D warnings`.

## Homebrew e próximo bloqueio

As nove raízes anteriores continuam produzindo **10676 instruções/blocos**:

```powershell
cargo run --offline -p xberecomp -- translate target/homebrew/hello.xbe --entry 0x19110 --entry 0x145E0 --entry 0x205E0 --entry 0x20889 --entry 0x13C70 --entry 0x18FC0 --entry 0x18C90 --entry 0x139B0 --entry 0x14F14 --out target/alpha-hello-display
cargo run --offline --manifest-path target/alpha-hello-display/Cargo.toml --target-dir target/recomp-hello-check
```

Usar diretório novo/vazio. O hello configura o modo com PA=0x50000 e pitch=2560,
avança após o retorno Step=0 e para em **AvSendTVEncoderOption (2), opção 11**,
retorno **0x12DFA**, argumentos `0xFD000000, 11, 5, 0`: filtro de cintilação.
Essa opção ainda não foi implementada; a saída permanece desabilitada.

O teste local ignorado
`target/alpha-hello-crt-case-20261021/tests/display_mode.rs` conferiu modo,
480 linhas pretas e snapshot integral byte a byte nessa fronteira: **17213045
retired/ciclos e 40 chamadas atendidas**. Log `target/display-hello-effect.log`.

Ainda não há janela/arquivo de apresentação nem texto Hello nxdk desenhado.
O conteúdo lido nessa fronteira é o buffer preto zerado anteriormente pelo guest.
Próximo trabalho: modelar os ajustes do encoder chamados pelo SDK, a rampa gamma
via MMIO e a apresentação do framebuffer, respeitando o empréstimo da memória.
Vblank/IRQ, outros modos/formatos, preservação de framebuffer e scheduler geral
seguem pendentes. BIOS e mídia comercial não foram usadas.
