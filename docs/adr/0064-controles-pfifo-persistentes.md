# ADR-0064: Controles PFIFO persistentes e aliases NV_USER

- **Status:** aceito para programação de controles; execução DMA pendente.
- **Data:** 2026-10-23
- **Fase:** 5 / PLANO 1.2
- **Base:** `3e4b36c`

## Contexto

O inventário do [ADR-0063](0063-inventario-pfifo-triangle.md) identificou 37
controles PFIFO, os aliases USER PUT/GET e os descritores preparados pelo triangle.
Descartar essas escritas perdia a configuração do canal e os valores salvos em
RAMFC. O decoder seguinte precisa dessa programação como estado real, inclusive
em snapshot, sem inferir configuração a partir do nome do sample.

## Alternativas consideradas

1. Mapear toda a faixa como um vetor genérico: esconderia registradores sem modelo
   e atribuiria readback a offsets sem consumidor/evidência.
2. Atualizar GET ao escrever PUT: satisfaria o polling sem ler um comando e foi
   descartado por violar o contrato do projeto.
3. Guardar apenas os controles inventariados e compartilhar aliases: escolhido;
   possibilita implementar o consumidor numa etapa separada e verificável.

## Decisão

Nv2aPfifo guarda os 37 DWORDs selecionados no ADR-0063. RegistersV1 é um perfil
público de compatibilidade com armazenamento inicialmente zero, não uma alegação
de reset físico do console. O provisório P15 declara que controles persistentes
não implementam DMA, FIFO, arbitragem, temporização, semáforos ou PGRAPH.

INTR_0 é write-one-to-clear: escrever 0xFFFFFFFF não cria pendências. Ainda não há
fonte de IRQ PFIFO; restaurar pendência não nula recusa. Os demais controles
preservam os bits programados, sem normalização ou máscaras especulativas.
O consumidor futuro deverá validar os campos de que depender. Fontes primárias,
revisões e valores observados estão no ADR-0063; especialmente outer.h/pbkit.c
do nxdk e pfifo.c/user.c do xemu. Nenhum código de terceiros foi copiado.

USER DMA_PUT/GET do canal zero são aliases dos controles CACHE1_DMA_PUT/GET.
Exigem MODE bit zero ativo e PUSH1 indicando canal zero em modo DMA. Acesso PIO,
canal não selecionado ou outro canal USER nessa interface recusa explicitamente.
O backup/restore de controles por CPU continua possível, inclusive selecionar PIO
no PUSH1; não há execução PIO. PRAMIN/RAMFC/RAMHT conservam o backing único do
perfil independente existente; este modelo não copia tabelas nem descritores.

Stores exigem DWORD alinhado; leituras BYTE/WORD/DWORD alinhadas podem selecionar
subpalavras little-endian dentro do controle. Largura inválida, cruzamento de
controle e variantes USER recusam antes de publicar efeitos. O preflight virtual
valida o elemento inteiro antes de fatiá-lo por página. A visão física aplica o
mesmo contrato. A cópia da resposta no Bus respeita a largura validada, incluindo
subpalavras, sem copiar quatro bytes para um buffer menor.

CACHE1_STATUS, RUNOUT_STATUS, DEBUG_0 e offsets não inventariados continuam zero
com telemetria. Escrever PUT não altera GET. Não há worker, avanço de Clock,
unsafe, dependência nova ou efeito gráfico. O encaminhamento de stores NV2A foi
extraído para um helper, preservando a ordem dos modelos existentes e o fallback.

## Snapshot e migração

BUS0 passa de v6 a v7 e acrescenta PFFO v1 após BTPG. PFFO contém tag/versão,
perfil u32, contagem u32 e os valores u32 em ordem crescente dos offsets fixados
neste ADR: 37 controles/164 bytes para RegistersV1; zero controles/16 bytes para
LegacyUnmodeled. Versão, perfil, contagem, pendência inválida e truncamento recusam.
A seção PFIFO só é publicada depois de toda sua validação; a carga do Bus ainda
exige descartar a sessão se outro componente já tiver sido alterado antes do erro.

BUS0 v1–v6 migram para LegacyUnmodeled, substituindo o estado do destino. Leituras
seguem zero e stores descartados com telemetria, mesmo após salvar/restaurar outra
vez. Boot, PRAMIN, PFB e NVPLL preservam suas migrações existentes. Adotar o perfil
novo exige sessão nova. KRNL v13, AOTC v8 e AOTS v3 não mudam.

As fixtures de versões antigas retiram PFFO antes das demais seções que não
existiam. Corrupções e truncamentos continuam testando a seção pretendida; não
se deixam bytes extras do modelo novo passarem por um snapshot antigo.

## Consequências

- A configuração do guest persiste por VA, alias e visão física, preparando o
  decoder DMA com o mesmo estado que aparece em snapshot.
- O polling ainda deve expirar enquanto não houver consumo; não há compromisso
  de o triangle avançar mais por esta entrega isolada.
- P15 sai quando o consumo real e os estados de fila substituírem o perfil de
  programação isolado, com migração explícita e evidência do alvo.

## Como reverter

Restaurar o encaminhamento sem PFIFO e migrar PFFO para ausência de modelo. Não
rebaixar silenciosamente snapshots v7 ou alterar seus campos sem nova versão.

## Verificação

Sete testes de memória cobrem controles/aliases/visão física, subpalavras e W1C,
recusa de PIO/canal, largura/preflight, offsets sem modelo, snapshot/corrupção e
migração v6 com boot preservado. As suítes antigas continuam verificando todas as
migrações v1–v5 de PLL/PFB/PRAMIN/boot com fixtures ajustadas ao novo formato.
Três casos de Rust emitido executam programação x86, replay/v6 e store inválido
sem avançar CPU/Clock ou publicar controle. O contador passa a 227 em x86_64,
211 fora dele. Os resultados do gate e da sonda estão registrados no PLANO.md.

Gate Rust/Cargo 1.99: `GATE OK: 526 testes`, log
`target/gate/20261023-003811.log`. 227 casos gerados em debug e release, este com
warnings negados (`target/plan-12-native-release.log`). Sonda oficial
`target/sonda/triangle-20261023-003811.log`, 320 fatias: MMIO sem modelo **209 -> 170**;
PUT/GET não aparecem como sem modelo. A execução mantém 93 chamadas/zero
recusadas, retired/ciclos=252079732, EIP=0x14D36 por orçamento e framebuffer
1580 pixels/29 linhas. O polling ainda expira por ausência de DMA real, conforme
previsto; o próximo trabalho é a tarefa 1.3, não atribuir GET artificialmente.
