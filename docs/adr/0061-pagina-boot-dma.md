# ADR-0061 — Página de boot D3D antes do carregamento da imagem

Data: 2026-10-22. Base: `b712e8d`.
Estado: aceito para a reserva/mapeamento da página; não implementa PFIFO/DMA.

## Evidência e decisão

Triangle já preserva PRAMIN (ADR-0060), mas parava escrevendo em VA=0x80000000,
EIP=0x18F19, antes de WBINVD/pb_start. O loader usava o frame físico zero para
os headers XBE. Criar um alias RW desse frame permitiria sobrescrever os headers.

Fontes públicas fixadas, sem código copiado:

- [nxdk, pb_init](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/pbkit/pbkit.c#L2775)
  escreve (pb_Head & 0x0FFFFFFF)+1 em 0x80000000 e executa WBINVD antes de
  iniciar o pushbuffer. Cria antes um contexto DMA com essa base e depois espera
  NV_USER DMA_GET, offset 0x44. Essa página é RAM, não um registrador novo.
- [Cxbx, CxbxKrnl.h](https://github.com/Cxbx-Reloaded/Cxbx-Reloaded/blob/585c49a50af1255ab155099e06f24505f9c5a800/src/core/kernel/init/CxbxKrnl.h):
  D3D_PHYSICAL_PAGE=0.
- [Cxbx, InitializeSystemAllocations](https://github.com/Cxbx-Reloaded/Cxbx-Reloaded/blob/585c49a50af1255ab155099e06f24505f9c5a800/src/core/kernel/memory-manager/VMManager.cpp):
  reserva uma página contígua D3D no boot, RW, antes das alocações do título.

A decisão é representar essa posse no estado de memória e instalar a reserva
antes da primeira página XBE. Trata-se de um perfil público de compatibilidade,
não do boot completo do firmware ou de uma medição física do console. O zero
inicial é política local de inicialização. Nenhuma BIOS proprietária foi usada.

## Contrato

Bus novo começa sem reserva. Image::load em Mode::Aot instala uma página de
boot: PA=0, VA=0x80000000, tamanho=4 KiB, RW sem execução. O frame entra no bitmap
antes dos frames da imagem. Headers passam a PA=0x1000 no layout padrão;
seus endereços virtuais e proteções permanecem. Código continua sem alias RW.
Mode::Inspect mantém seu layout e alias integral de inspeção, sem essa reserva.

BootDmaPage registra a posse, não uma cópia da RAM. CPU e DMA veem os mesmos
bytes na arena. Repetir a instalação valida o estado e preserva conteúdo; frame
ou VA ocupados, alias prévio ou RAM insuficiente recusam sem alterar recursos.
Reserva/MMU são preparadas em cópias antes da publicação. Apenas a página de boot
ganha o mapeamento: não se abre um alias contíguo de toda a imagem.

A validação exige frame reservado, tradução exata, permissões RW e ausência de
aliases externos do frame zero. O alocador contíguo reconhece a posse do boot
como recurso distinto de CMEM: não pode alocá-la nem reivindicá-la num snapshot.
A página não aparece na lista de alocações do título. Imagem sobre a VA reservada
é recusada tanto no loader AOT quanto na validação de startup/análise.

A posse fica no Bus junto de MMU/frames; o loader escolhe a política de boot.
Isso permite restaurar layouts antigos sem inferir recursos dos dados atuais
do Dispatcher ou depender dos imports do título. A reserva vale para todo AOT
novo, inclusive imagens que não importam pbkit, e não depende do nome do sample.

Stores nessa RAM não disparam emulação do DMA nem modificam registradores da
GPU. WBINVD conserva sua semântica atual do runtime síncrono. Não há novo relógio,
IRQ, worker, unsafe ou mudança em Send/!Sync. P13/P14 permanecem abertos: a reserva
de instância ainda é lazy e PRAMIN continua independente do backing físico UMA.

## Snapshot e compatibilidade

BUS0 v6 acrescenta BTPG v1 depois de PRAM: tag, u32 versão e u8 reserved canônico
0/1, nove bytes. RAM, MMU e bitmap continuam salvos uma única vez. Marcador,
versão, frame, tradução, permissões ou alias incoerentes recusam. A carga do Bus
mantém o contrato de descartar a sessão após erro, pois outros componentes podem
já ter sido restaurados. KRNL v13/GINS v1, AOTC v8 e AOTS v3 não mudam.

BUS0 v1–v5 migram para ausência de reserva, substituindo a configuração do destino.
Os headers podem continuar em PA zero; não são movidos ao carregar um snapshot
antigo. A VA DMA não é adicionada e o replay reproduz a falta antiga. Salvar
novamente guarda BTPG reservado=false. Para adotar o layout novo, abrir sessão nova.
Os perfis legados de NVPLL/PFB/PRAMIN conservam suas migrações existentes.

**Fixtures de migração precisam representar o layout anterior completo.** Remover
BTPG de um snapshot novo mantendo o alias da página de boot produziria um estado
sem dono, que o kernel recusa. A fixture nativa realoca os frames de imagem/startup
uma página abaixo, remove a VA de boot, reconstrói o bitmap e depois emite BUS0 v5.
VAs, dados, CPU, kernel e os estados NV2A são preservados. O helper só aceita uma
sessão sem alocações dinâmicas; não é um conversor geral de snapshots com CMEM.
As migrações PFB/NVPLL/PRAMIN usam esse layout anterior antes de remover suas seções.

## Verificação

Quatro testes de memória cobrem reserva/zero/idempotência, coerência CPU/DMA,
execução recusada, colisões sem mutação, snapshot, corrupção e migração v1–v5 com
headers em PA zero. Dois testes do kernel verificam isolamento dos headers,
indisponibilidade para alocação contígua e recusa de CMEM que reivindique o boot.
O teste antigo de loader agora verifica o alias de código ausente e permite só
a página de boot; o teste de startup inclui colisão com a nova VA reservada.
Um teste adicional confere que origem e índice de TLS não possam usar a página
de boot: antes ela não existia quando o loader verificava esses endereços; agora
precisa ser excluída explicitamente para preservar o contrato de TLS na imagem.

Três casos de Rust emitido executam MOV/WBINVD reais, conferem flags/Clock/RAM,
replay e a falta sem efeito na CPU após migração BUS0 v5. Gate Linux/Rust 1.98.1:
513 testes workspace/doctests, 221 casos gerados debug/release (contador esperado
205 fora de x86_64), fmt e Clippy all-targets com warnings negados. Release também
usa RUSTFLAGS=-D warnings. Scripts não mudaram; Win32 não revalidado aqui.
Serviços permanecem hello 37/69 e triangle 41/75.

Logs locais sob target/: boot-workspace.log,
boot-clippy.log, boot-native-release.log, boot-triangle-translate.log,
boot-triangle-probe.log e boot-triangle-after.log.

## Reprodução do alvo real

O triangle-raw.xbe do ADR-0059 continua com as mesmas dez raízes e 17883 blocos,
sem alterar bytes guest. Projeto novo: target/homebrew2/boot-review-20261022.
Para evitar recompilar todo o guest a cada ajuste da sonda, o projeto local expõe
src/generated.rs numa biblioteca; main e tests/boot_probe.rs chamam essa mesma
função run. O runtime e a semântica do programa não mudam. Compilação release
padrão, sem reduzir opt-level. O teste usa 4096 blocos/fatia, até 40000 fatias.

A sonda moderna interrompe deliberadamente ao observar a escrita no bootstrap e
a leitura de DMA_GET, sem simular resposta da GPU. Confere valor escrito, headers
em PA=0x1000/RO e snapshot integral. A sonda v5 constrói o layout anterior,
restaura-o e confere a falta histórica VA=0x80000000/EIP=0x18F19 e seu replay.
O comando normal --fatias 4000 é executado separadamente para registrar a próxima
fronteira; a parada deliberada da sonda não equivale à conclusão de pb_init.


A regressão do hello também foi conferida em release/headless no Linux, com as
nove raízes e 10676 blocos da passagem de bastão, --fatias 64. Continua desenhando
1208 pixels coloridos em 48 linhas, 47 chamadas atendidas, 17452569 instruções e
8023 ms guest. O retorno de erro é BudgetExhausted esperado do laço de main, não
uma falta nova. Logs target/boot-hello-translate.log e target/boot-hello.log.
A janela Win32 não foi exercitada nesta máquina.


Com as dez raízes antigas, o main agora chega ao ISR em 0x1B420 e para por
UnknownTarget. Disassembly de pb_install_gpu_interrupt confirma o endereço
passado a KeInitializeInterrupt e DPC=0x1B340 passado a KeInitializeDpc.
Foram acrescentadas as duas entradas ao projeto local
`target/homebrew2/boot-irq-review-20261022`: 12 raízes, 17984 blocos, sem ampliar
ISA ou alterar bytes guest. Log de tradução target/boot-triangle-irq-translate.log;
execução completa target/boot-triangle-irq.log. A sonda de 17883 blocos continua
válida como comparação de boot/legado antes dessa entrega de interrupção.


Com 12 raízes, a execução completa atende ISR e DPC (12 ocorrências do acesso
NV_PFIFO_DEBUG_0 em EIP=0x1B3AA), mas DMA_GET continua zero: 12402025 leituras de
0xFD800044. A GPU não consome o pushbuffer; pb_init expira a espera e chama
pb_kill, que também expira a espera por DMA ocioso. O framebuffer mostra texto
de diagnóstico: 1580 pixels coloridos em 29 linhas, sem triângulo.

O próximo serviço não atendido é **MmFreeContiguousMemory (171)** na limpeza,
retorno 0x17520, argumento 0x80189000, EIP=0x7FFE0558; 173079257 retired/ciclos,
81 chamadas atendidas e uma recusada. O problema primário continua sendo o
processamento PFIFO/DMA_GET ausente; implementar a liberação é necessário para
concluir a limpeza, não satisfaz por si só a inicialização gráfica.

A sonda adicional `boot-irq-review-20261022/tests/end_probe.rs`, compilada em
0,85 s reutilizando a biblioteca do guest, confirmou DPC, polling, contexto da
chamada pendente, headers intactos e roundtrip integral nesse ponto. Passou;
log target/boot-triangle-irq-probe.log. A execução normal, as duas sondas de boot,
o replay legado e a regressão hello usam Rust gerado real, sem interpretador.
