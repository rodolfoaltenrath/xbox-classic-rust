# ADR-0050 — Pedidos de filtros do encoder

Data: 2026-10-21. Estado: aceito para o perfil HLE de cabo padrão.

## Evidência e escopo

Base limpa: `0d77ba5`. O hello já configurava o modo 640×480 e parava em
AvSendTVEncoderOption (2), opção 11, Param=5, retorno 0x12DFA.

O nxdk usa a opção 11 para flicker, com nível mascarado a três bits, e 14 para
soften, com BOOL. XVideoSetMode solicita flicker=5 e soften=FALSE antes da rampa
gamma por MMIO e da habilitação do vídeo. Fontes públicas fixadas na revisão
`14d5ee97e73347c973f1f57b68b79ec08c9e77f2`:

- [video.c](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/hal/video.c);
- [video.h](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/hal/video.h).

O efeito implementado é registrar os **pedidos** no encoder lógico. Não há
modelo do filtro analógico, kernel espacial sobre os pixels, registradores de
um chip Conexant/Focus/Xcalibur ou promessa de aparência equivalente ao console.
FramebufferView permanece uma inspeção dos bytes originais do guest.

## Contrato

Com RegisterBase=0xFD000000 e Result=NULL, AvSendTVEncoderOption aceita:

| Opção | Param | Estado publicado |
|---|---|---|
| 11 / FLICKERFILTER | 0..7 | flicker_filter=Some(nível) |
| 14 / SOFTEN_FILTER | 0 ou 1 | soften_filter=Some(false/true) |

O intervalo 0..7 é o subconjunto emitido pelo SDK; não afirma que oito curvas
analógicas distintas foram reproduzidas. O HLE não mascara parâmetros inválidos
para transformá-los em válidos. Base, parâmetros ou Result fora do perfil
recusam como VarianteNaoImplementada, antes de publicar qualquer pedido.
Result não é desreferenciado. O retorno VOID usa EAX=0 na ABI já existente;
isso não é uma consulta de resultado nem um NTSTATUS.

Cold boot usa None para ambos, pois não há configuração herdada conhecida.
Some(0) e Some(false) são pedidos explícitos e distintos da ausência. Repetir
um pedido é idempotente. Cada ajuste preserva o outro, o modo, o blanking,
EEPROM, RAM, Clock/retired e os registradores fora do retorno normal stdcall.
O serviço existente ganhou variantes; o registro continua **27/69 serviços**.
Não há unsafe novo, thread host ou compartilhamento da arena.

## Snapshot

**AVST v3** mantém o prefixo v2 e acrescenta dois bytes após o modo opcional:
flicker (0..7 ou 0xFF para None) e soften (0/1 ou 0xFF para None).
Outras codificações ou truncamentos recusam, preservando o objeto de destino.
AVST v1/v2 migram sem pedidos, sem herdar os valores da sessão de destino;
v2 preserva o modo e v1 continua migrando sem modo. Enabled é preservado.

KRNL permanece v10; nenhuma seção nova entrou na sequência. AOTS v2/AOTC v7,
CMEM v1 e as demais versões permanecem. Os cruzamentos modo/EEPROM/CMEM/Bus
continuam em vigor.

## Validação e próximo passo

Quatro testes novos de kernel verificam a faixa de níveis, pedidos independentes,
idempotência, recusa sem truncamento/desreferência, snapshot/corrupção e migração
v1/v2. O teste de modo também confere que os ajustes preservam framebuffer e
blanking. Três casos no Rust emitido exercitam ABI/CPU/flags/Clock, replay de
chamada pendente e recusa com reparo. A suíte de display verifica a migração de
um AVST v2 com modo configurado através da restauração integral da sessão.

Validação: **398 testes workspace/doctests e 177 casos nativos debug/release**
(161 fora de x86_64), Clippy global e projeto emitido em release com
`-D warnings`, fmt/diff/links limpos. Logs `target/encoder-filters-tests.log`,
`target/encoder-filters-workspace.log`, `target/encoder-filters-native-release.log`
e `target/encoder-filters-clippy.log`.

O hello real atendeu as duas opções:

| Pedido | Retorno guest |
|---|---|
| 11, Param=5 | 0x12DFA |
| 14, Param=0 | 0x12E63 |

Depois parou em **PC=0x12EEB**, escrita de um byte zero em
**VA=0xFD6813C8**, NV_USER_DAC_WRITE_MODE_ADDRESS. A MMU retorna NaoMapeado;
ainda não houve operação de dispositivo. Fronteira: **17232328 retired/ciclos,
42 chamadas atendidas**, saída desabilitada, flicker=Some(5), soften=Some(false).
O teste conferiu snapshot integral byte a byte e repetição da falta sem cobrar
tempo nem modificar CPU/estado. Evidência local ignorada:
`target/alpha-hello-crt-case-20261021/tests/encoder_filters.rs`, log
`target/encoder-filters-hello-effect.log`.

As nove raízes anteriores e 10676 instruções/blocos permanecem. Para reproduzir
com o CLI e um diretório novo/vazio:

```powershell
cargo run --offline -p xberecomp -- translate target/homebrew/hello.xbe --entry 0x19110 --entry 0x145E0 --entry 0x205E0 --entry 0x20889 --entry 0x13C70 --entry 0x18FC0 --entry 0x18C90 --entry 0x139B0 --entry 0x14F14 --out target/alpha-hello-encoder-filters
cargo run --offline --manifest-path target/alpha-hello-encoder-filters/Cargo.toml --target-dir target/recomp-hello-check
```

O próximo passo do SDK após selecionar o índice zero é escrever 256 triplas
R,G,B no registro 0xFD6813C9; o pedido inicial é a rampa identidade. Essa
sequência ainda não executou. Nenhuma janela ou imagem foi apresentada.

Próximo trabalho: modelar o DAC gamma NV2A acessado pelo SDK, com estado e
snapshot próprios, antes de habilitar/apresentar o framebuffer. Abrir uma página
MMIO sobre o destino genérico atual apenas descartaria as escritas; não é um
modelo de rampa gamma. Outros registradores, scanout/vblank/IRQ, filtros visuais,
Sleep/escalonamento e apresentação continuam pendentes. BIOS e jogos comerciais
não foram usados.
