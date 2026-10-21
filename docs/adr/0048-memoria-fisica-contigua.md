# ADR-0048 — Memória fisicamente contígua para o framebuffer

Data: 2026-10-21. Estado: aceito para recursos RAM RW, com ou sem WRITECOMBINE.

## Contrato e fonte

O repositório chegou limpo em `84ead43`. O hello parava em
MmAllocateContiguousMemoryEx (166), stdcall/20, retorno 0x12C01. O pedido era
`0x12C000, 0, 0x7FFFFFFF, 0x1000, 0x404`: 1.228.800 bytes para 640×480×4.

A assinatura pública do nxdk tem tamanho, menor/maior endereço físico,
alinhamento e proteção. XVideoInit escreve no virtual retornado, zera o buffer,
executa SFENCE e passa `virtual & 0x7FFFFFFF` como endereço físico ao vídeo.
Depois guarda o ponteiro virtual para desenhar. Fontes fixadas em
`14d5ee97e73347c973f1f57b68b79ec08c9e77f2`:

- [xboxkrnl.h](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/xboxkrnl/xboxkrnl.h);
- [video.c](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/hal/video.c).

O alocador virtual do ADR-0043 permite frames fragmentados e não satisfaz esse
contrato. Esta entrega usa um registro separado e o mesmo bitmap físico do Bus.

## Implementação e perfil

O serviço procura frames livres **consecutivos**, por first-fit, dentro dos
limites físicos inclusivos do pedido e da RAM real. Tamanho é arredondado a
4 KiB; o bloco arredondado inteiro deve caber. Alinhamento precisa ser potência
de dois de pelo menos 4 KiB. São aceitos Passive e proteção 4 ou 0x404,
PAGE_READWRITE com WRITECOMBINE opcional. Outros contextos/proteções/alinhamentos
recusam explicitamente como variantes.

Falta de sequência livre, faixa insuficiente/invertida, tamanho zero/overflow,
RAM insuficiente ou limite de 4096 recursos retorna NULL sem publicar alocação.
A soma de frames livres não substitui a prova de contiguidade. A busca também
pula páginas cujo endereço virtual correspondente já esteja ocupado.

O retorno é **GuestVirt(0x80000000 | PA)**. Apenas as páginas do novo recurso
são mapeadas RW, sem execução, para seus frames consecutivos. Não é criada a
janela completa sobre a RAM, nem um alias baixo de identidade. Isso preserva a
proteção do código/recursos existentes. Retirar o bit alto produz o GuestPhys
esperado pelo SDK; o consumidor DMA continua usando o acesso físico do Bus.

MMU e bitmap são planejados em cópias; nenhum mapeamento existente é sobrescrito.
Antes do commit, os frames reserváveis são zerados por política local, incluindo
slack de página. Não se afirma que a API de hardware garante memória zerada.
RAM virtual/física/DMA usa a mesma arena exclusiva. O metadado mantém Protect;
WRITECOMBINE segue o perfil de coerência síncrona já definido para Bus/SFENCE,
sem simulação de cache ou fila de escrita. Não há unsafe novo ou arena compartilhada.

O serviço retorna ponteiro em EAX; não é NTSTATUS e não recebe ponteiros de
saída. Session valida/captura o retorno HLE antes dos efeitos. Registros, flags
e Clock seguem o contrato stdcall existente. O catálogo passa a **26/69 serviços**.
Liberação, persistência, consulta de tamanho/endereço físico e outras proteções
continuam pendentes; os recursos são retidos nesta versão.

## Snapshot e consistência

**KRNL v10 acrescenta CMEM v1**, registros ordenados de PA/tamanho/alinhamento/
proteção. RAM/MMU/bitmap continuam exclusivamente no Bus; VA é derivado do PA.
AOTS v2/AOTC v7 e as demais seções não mudaram.

A leitura valida versão, contagem, geometrias, ordem/sobreposição e limite físico
do perfil de até 128 MiB. Session cruza a RAM real, os frames reservados, cada
tradução linear RW e a ausência de aliases externos. Páginas RAM na janela
contígua sem registro também recusam. VMEM mantém sua verificação de posse dos
recursos privados; os dois alocadores não compartilham frames. Essa inspeção
não marca Accessed/Dirty.

KRNL v1–v9 migram sem recursos contíguos; um payload antigo não pode esconder
novas páginas dessa janela no Bus. Erro de restauração exige descartar a sessão.

Oito testes de kernel cobrem contiguidade, zero inicial, DMA/store entre páginas,
limites/alinhamento, colisão de VA, fragmentação/OOM, variantes e corrupção do
snapshot/bitmap/permissões/aliases. Três casos no Rust emitido verificam stdcall,
VA→PA do SDK, dados/DMA, duas alocações, replay/migração, erro sem efeitos e
recusa de páginas órfãs. Logs: `target/contiguous-tests.log`,
`target/contiguous-workspace.log`, `target/contiguous-native-release.log`,
`target/contiguous-clippy.log`.

Aceite Windows/Rust 1.98.1: **388 testes workspace/doctests e 171 casos nativos
debug/release** (155 fora de x86_64; release com `-D warnings`), Clippy global
com `-D warnings`, fmt, diff e links locais aprovados.

## Evidência real e reprodução

As mesmas nove raízes continuam traduzindo **10676 instruções/blocos**:

```powershell
cargo run --offline -p xberecomp -- translate target/homebrew/hello.xbe --entry 0x19110 --entry 0x145E0 --entry 0x205E0 --entry 0x20889 --entry 0x13C70 --entry 0x18FC0 --entry 0x18C90 --entry 0x139B0 --entry 0x14F14 --out target/alpha-hello-contiguous
cargo run --offline --manifest-path target/alpha-hello-contiguous/Cargo.toml --target-dir target/recomp-hello-check
```

Usar saída nova/vazia. O hello recebeu VA **0x80050000**, PA **0x50000**, 300
páginas/1.228.800 bytes. O próximo bloqueio é **AvSetDisplayMode (3)**, retorno
**0x12CA4**, argumentos `0xFD000000, 0, 0x04010101, 0x12, 0xA00, 0x50000`:
modo 640×480 NTSC-M/60, formato A8R8G8B8, pitch de 2560 bytes e PA real do buffer.

O teste real sujou os 1.228.800 bytes com 0xA5 após a alocação HLE, comprovou
que o memset guest os zerou e conferiu CPU/DMA e snapshot integral byte a byte.
Na fronteira foram 17213022 retired/ciclos e 39 chamadas atendidas. Evidência:
`target/contiguous-hello-effect.log` e teste local ignorado
`target/alpha-hello-crt-case-20261021/tests/framebuffer_allocation.rs`.

Ainda não há programação do modo ou imagem apresentada. Próximo trabalho:
representar o modo/scanout e encaminhar o framebuffer por seu PA, respeitando
limites/stride/formato e a posse exclusiva do Bus. Depois atender os controles
de encoder/MMIO efetivamente usados pelo SDK e apresentar a saída, sem inventar
sucesso para registradores/dispositivos não modelados. BIOS e mídia comercial
não foram usadas.
