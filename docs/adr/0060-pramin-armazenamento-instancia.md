# ADR-0060 — Armazenamento de instância PRAMIN

Data: 2026-10-22. Base: `7dda13b`.
Estado: aceito como perfil de compatibilidade independente, provisório P14.

## Evidência e recorte

Depois da consulta de instância do ADR-0059, triangle/pbkit escreve as tabelas
RAMHT, contextos DMA e gráficos na janela PRAMIN. O placeholder descartava todos
os stores e devolvia zero. Esta entrega preserva esses bytes para os acessos CPU
e o futuro consumidor PFIFO, sem executar os contextos nem adiantar DMA_GET.

Fontes públicas fixadas, usadas como referência de contrato, sem código copiado:

- [nxdk, pbkit.c](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/pbkit/pbkit.c):
  pb_init zera 20 KiB em PRAMIN+padding; pb_create_dma_ctx escreve descritores
  de quatro DWORDs; pb_bind_channel publica entradas da RAMHT. O programa real
  já toca 0xFD71386C em EIP=0x183AA antes da falta seguinte.
- [xemu, nv2a.c](https://github.com/xemu-project/xemu/blob/03b988c9d76553b33ed712ba00822ffdf49f0e7d/hw/xbox/nv2a/nv2a.c#L209):
  janela de 1 MiB no offset 0x700000, com RAMIN independente. A hipótese de alias
  com VRAM permanece comentada por incerteza de localização. nv_dma_load lê os
  descritores dessa mesma região, separada da RAM UMA usada para dados DMA.

A geometria é um perfil público de emulador, não medição física ou prova de que
PRAMIN seja SRAM separada no console. O estado inicial zerado é política explícita
do perfil; não se afirma que seja conteúdo físico de reset. Nenhuma BIOS usada.

## Contrato

Nv2aPramin guarda IndependentV1 com 1 MiB de bytes, inicialmente zero. A janela
física [0xFD700000, 0xFD800000) aceita somente acessos alinhados de 32 bits.
Stores preservam todos os bits e reads retornam little-endian. Aliases virtuais,
acesso físico e a consulta readonly Nv2aPramin::read veem o mesmo armazenamento.
Essa consulta valida offset/alinhamento/limite e será o ponto do consumidor PFIFO.

Outras larguras, desalinhamento ou sobreposição com as bordas recusam
explicitamente. O fatiamento em páginas não pode converter um acesso de oito
bytes em dois acessos válidos: a validação considera os bytes restantes do
acesso original. O preflight de stores verifica todas as páginas antes de tocar
RAM/PRAMIN, inclusive numa faixa que começa em RAM e termina em MMIO inválido.
Offsets externos permanecem na telemetria. Acessos modelados não geram registros
de MMIO sem modelo. Não se criam ciclos, IRQs ou execução de métodos pelos stores.

**P14:** o backing independente segue a referência xemu e mantém a incerteza
física declarada. Não é a RAM do framebuffer duplicada e não cria alias presumido
com a reserva CMEM do ordinal 168. Os 128 KiB dessa reserva continuam pertencendo
à RAM do Bus (P13). Estabelecer o vínculo físico PRAMIN/RAM/reserva exige fonte
ou medição antes de substituir o perfil; essa é a condição de saída do P14.
Não há unsafe novo, thread, relógio, backend de renderização ou mudança em Send/!Sync.

## Snapshot e migração

BUS0 v5 acrescenta PRAM v1 depois de PFBM. Formato:

```text
PRAM | u32 versão=1 | u32 perfil | u64 tamanho | bytes
perfil 0: LegacyUnmodeled, tamanho=0, sem payload
perfil 1: IndependentV1, tamanho=0x100000, payload integral
```

O perfil ativo acrescenta 1048596 bytes por snapshot; o legado acrescenta 20.
Versão zero/futura, perfil desconhecido, tamanho incompatível e truncamento
recusam antes de publicar o modelo PRAMIN. O Bus conserva seu contrato de carga:
qualquer erro exige descartar a sessão, pois RAM/MMU podem já ter sido carregadas.

BUS0 v1–v4 migram para LegacyUnmodeled: zero com telemetria e stores descartados.
Não herdam bytes do destino nem ativam PRAMIN silenciosamente. Salvar de novo
preserva o perfil legado. Sessão nova adota IndependentV1. Os perfis anteriores
IRQ/NVPLL/PFB conservam suas migrações. KRNL v13/GINS v1, AOTC v8 e AOTS v3
permanecem iguais. Fixtures de migração PFB/NVPLL removem PRAM antes do sufixo
antigo; as de Session preservam o kernel/TCXT que vêm depois do Bus.

## Validação

Seis testes do Bus cobrem RAMHT/instâncias/limites, aliases/físico, isolamento de
RAM e frames, acesso fora da janela, larguras, travessia de páginas, preflight
sem store parcial, replay, corrupção/truncamentos e migração BUS0 v1–v4.
Três casos de Rust emitido executam stores/reads PRAMIN reais, verificam
CPU/flags/Clock, snapshot integral e v4, e retentativa após acesso inválido.
Os testes antigos de PFB/NVPLL foram ajustados ao novo sufixo, preservando seus
contratos; os de truncamento continuam cobrindo os metadados de cada seção.

Gate Linux/Rust 1.98.1: 506 testes workspace/doctests, 218 casos gerados debug e
release (contador esperado 202 fora de x86_64), fmt e Clippy all-targets com
warnings negados. Release também usa RUSTFLAGS=-D warnings. Testes Python dos
scripts não foram repetidos: os scripts não mudaram. Win32 não revalidado aqui.
Serviços permanecem hello 37/69 e triangle 41/75; nenhum ordinal novo.

Logs locais sob target/: pramin-mem-tests.log, pramin-workspace.log,
pramin-clippy.log, pramin-native-release.log, pramin-triangle-after.log e
pramin-triangle-probe.log.

## Fronteira real e próximo contrato

O mesmo triangle do ADR-0059, SDK fixado e 17883 blocos/dez raízes, tem os hashes
já registrados lá. A sonda local fica em
`target/homebrew2/instance-review-20261022/tests/pramin_probe.rs`; executa com
4096 blocos/fatia e 20000 fatias, sem alterar bytes ou retornos guest.
O main normal e os 218 casos gerados usam release padrão; a sonda local usa
cargo rustc --release --test pramin_probe -- -C opt-level=1 para reduzir o custo
de recompilar o harness que inclui 17883 blocos. Essa sonda não mede desempenho.
Cargo rustc apenas compila: executar depois o harness desse build; o hash do
executável pode diferir daquele produzido por cargo test.
O main recompilado, com --fatias 4000 e um milhão de blocos por fatia,
registra 230 offsets/1947 acessos sem modelo, sem PRAMIN ou descartes por limite;
antes havia 4096 offsets/11442 acessos e 2688 registros descartados. O framebuffer
segue com zero pixels coloridos e a falta permanece VA=0x80000000/EIP=0x18F19.

A sonda usa fatias menores: 4000 × 4096 só alcançava 16229937 instruções/39
chamadas, antes da consulta. O orçamento foi corrigido para 20000 fatias;
limite do host não equivale a bloqueio guest. O diagnóstico do conduzir é
MemoryAt { pc, fault }, pois run_slice enriquece o Memory cru do Bus com EIP;
a sonda deve verificar essa variante. Ela confere conteúdo não zero na
RAMHT e nos contextos, ausência de telemetria PRAMIN,
retentativa da falta sem mudar CPU e replay integral por snapshot. Passou com
30 DWORDs não zero na RAMHT e 323 nos contextos, retired/ciclos=24826623,
54 chamadas atendidas e EIP=0x18F19. Headers mantêm XBEH em PA zero, RO.

A execução ainda termina na preparação DMA, escrita VA=0x80000000/EIP=0x18F19,
antes de WBINVD/pb_start. Isso não é aceite de pb_init, DMA ou renderização.
A página física zero já pertence aos headers XBE: Image::load aloca primeiro os
headers via bitmap, e a sonda confere VA=0x10000 → PA=0/RO com assinatura XBEH.
A VA 0x80000000 está ausente. Mapear diretamente esse frame como scratch DMA
permitiria sobrescrever os headers; a entrega não adiciona esse alias.

Uma referência pública para o próximo recorte já foi localizada:
[Cxbx, InitializeSystemAllocations](https://github.com/Cxbx-Reloaded/Cxbx-Reloaded/blob/585c49a50af1255ab155099e06f24505f9c5a800/src/core/kernel/memory-manager/VMManager.cpp#L212)
reserva uma página D3D contígua no boot; [CxbxKrnl.h](https://github.com/Cxbx-Reloaded/Cxbx-Reloaded/blob/585c49a50af1255ab155099e06f24505f9c5a800/src/core/kernel/init/CxbxKrnl.h#L71)
fixa D3D_PHYSICAL_PAGE=0. Isso apoia a posse prévia no boot, não um efeito MMIO
artificial em 0x80000000. A integração ao loader ainda exige desenho/migração próprios.

Próximo: definir a reserva da página de bootstrap DMA antes das alocações da
imagem, com posse e migração compatíveis com snapshots antigos e validação dos
headers. Depois, usar PRAMIN como fonte dos contextos PFIFO/DMA e tratar o polling
NV_USER DMA_GET com efeitos reais, antes dos métodos PGRAPH.
