# Primeiro alpha: homebrew real

## Uso vigente — 2026-10-26

O bloco visual Windows reproduz hello/triangle, apresenta seus frames reais e
encerra pelo X. A Fase 5 acrescenta execução contínua, pausa e arquivos de
estado completos. Leia CONTINUIDADE_ALPHA.md para o estado técnico
completo; os relatos por ADR mais abaixo são históricos.

Requisitos: Rust fixado 1.98.1, linker compatível, Cargo/dependências disponíveis
offline, Git Bash no Windows e os dois XBEs homebrew locais. Para gerar entradas,
usar o nxdk público fixado e [os scripts de build](homebrew.md); o launcher não
baixa jogos/SDK nem usa BIOS. Sem entrada, ele informa o caminho ausente.

```bash
# Git Bash, raiz do repositório
bash scripts/run-alpha.sh hello
bash scripts/run-alpha.sh triangle
```

PowerShell: `& "$env:ProgramFiles\Git\bin\bash.exe" scripts/run-alpha.sh triangle`.
Usar o Bash do Git, não o atalho do WSL para a execução visual Windows.

```powershell
& "$env:ProgramFiles\Git\bin\bash.exe" scripts/run-alpha.sh triangle --continuo
& "$env:ProgramFiles\Git\bin\bash.exe" scripts/run-alpha.sh triangle --continuo --carregar "C:/meus estados/final.obsstate"
```

No modo contínuo, Espaço pausa/retoma (título PAUSADO); F5 salva uma geração nova
em `states/checkpoint-NNNNNN.obsstate`; F9 volta à última geração desta execução,
conservando pausa e gerações anteriores. Enquanto pausado CPU, Clock, RAM,
kernel e GPU não avançam; somente a janela é processada. Atalhos são do host,
sem USB/XID/controle Xbox. Falha ao ler/gravar por atalho é relatada em stderr.log
e preserva a sessão ativa; F9 antes de F5 é uma recusa informativa.

O modo contínuo exige janela; `--fatias` define o trecho repetido, não um limite
total. Apenas orçamento autoriza continuar. HLT, INT3, serviço recusado, memória
e demais falhas continuam encerrando o guest com diagnóstico. Sem pacing por
relógio real: não é promessa de FPS ou velocidade igual ao console.
`final.obsstate` é salvo ao fechar uma rodada contínua; `--salvar "novo caminho"`
seleciona outro destino (também disponível em modo finito). Diretório de destino
explícito deve existir. Nunca sobrescreve estado ou o arquivo carregado.

SAVE v1 contém o XBE exato e o snapshot integral com CRC32, limitado a 256 MiB;
arquivos correntes têm dezenas de MiB, sem compressão. XBE divergente, corrupção,
versão futura e truncamento recusam antes de trocar a sessão. CRC não autentica
arquivo malicioso; não carregar estados desconhecidos. Publicação requer hard
links (NTFS/ext4); sem promessa de durabilidade do diretório sob falta de energia.
Pausa/janela/teclas não são serializadas. Formatos internos e migrações legadas
permanecem; imagem igual não promete replay entre revisões diferentes do modelo.
Estados incluem código/dados guest: target e a extensão .obsstate são ignorados
pelo Git. Arquivos temporários .obsidian-state-*.part também são ignorados.

Para testar o executável gerado já compilado, com diretório de evidência novo:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/verify-session.ps1 -Executable target/sample-build/triangle/release/obsidian-native.exe -Sample triangle -OutputDirectory target/session-triangle -TimeoutSeconds 300
```

O teste excede o orçamento antigo, compara SHA-256 de estados completos durante
pausa e após F9, provoca leitura inválida, retoma em outro processo e mede
WM_CLOSE em execução e pausa. Compilação é separada dos deadlines de execução.
Rode a validação visual depois do gate. TimeoutSeconds controla só a espera
por progresso do sample; o prazo de fechamento continua sendo cinco segundos.

Testes de ferramentas (Linux/WSL): `python3 scripts/test-tools.py`. O helper
copia os mesmos scripts/testes para um Git fixture temporário em /tmp, confere
bytes e registra SHA-256 antes de executar os 19 casos. Evita git diff repetido
pelo mount NTFS do WSL; não altera timeouts nem o índice do repositório real.

As mensagens separam **preparo → build → execução**. O preparo sempre traduz
para uma pasta nova, sem --entry, gera Cargo.lock offline e registra hashes e
toolchain. O launcher só reutiliza projeto com imagem, manifest, lock e fontes
idênticos à preparação atual; Cargo ainda verifica/recompila os crates alterados.
Projetos/logs anteriores ficam preservados. Índice inválido do cache é arquivado
antes de apontar para um rebuild aprovado. Não editar target para usar outro PC.

O primeiro build release é custoso: as medições Windows iniciais foram 13m58
para hello e 22m03 para triangle. Não são tempos da execução guest nem garantia
de duração em outra máquina. Reabrir um projeto igual pode aproveitar o build
(0,20/0,23 s nas checagens observadas). CPU guest usa Clock sintético; não tratar
seus milissegundos como medição de FPS ou velocidade do jogo no host.

Opções (caminhos Windows em Git Bash podem usar `C:/pasta/arquivo.xbe`):

```bash
bash scripts/run-alpha.sh hello --xbe "C:/meus samples/hello.xbe"
bash scripts/run-alpha.sh triangle --fatias 4000
bash scripts/run-alpha.sh triangle --sem-janela
bash scripts/run-alpha.sh --help
```

--projeto seleciona um build já preparado; exige equivalência com a tradução
atual e não aceita projeto divergente. Normalmente o cache automático basta.
ALPHA_STATE_DIR muda a raiz de logs/índices (padrão target/alpha), ALPHA_BUILD_DIR
muda o target Cargo do sample. Valores relativos são relativos ao repositório.
O modo visual exige Windows; no Linux somente --sem-janela está disponível.

**Encerramento:** X durante run produz parada pelo host/status 0. Quando o
orçamento acaba, a janela mantém o frame final até X e o processo preserva
status 1 com diagnóstico de orçamento. Headless também usa status 1 ao esgotar
orçamento; outras falhas não são convertidas em sucesso. Erros de opções,
entrada, tradução, dependências ou build são explícitos. O modo padrão é
limitado (hello 64, triangle 320 fatias), para investigação controlada.

Cada pasta nova em target/alpha/runs guarda build.log, stdout.log, stderr.log,
guest.bgra (640×480 BGRA bruto, sem header/padding), project.txt,
prepared-project.txt e build-identity.txt; no Windows também pid.txt.
guest.bgra é só a imagem final, não um snapshot da sessão. Capturas PNG e
comparação RGB pelo sistema usam scripts/verify-window.ps1, com procedimento
no [ADR-0074](../adr/0074-alpha-visual-e-encerramento.md).

Aceite visual: zero diferenças RGB em 640×480, hello 302 pixels/12 linhas e
triangle 305922/479, R4977/G4998/B4977. Fechamento durante run ficou abaixo de
200 ms em ambos sem exportação de estado. Gate Windows 586 testes, 261 casos gerados debug/release;
snapshots/replay preservados. Fontes, métricas e comandos nos ADRs
[0073](../adr/0073-reproducao-samples-windows.md),
[0074](../adr/0074-alpha-visual-e-encerramento.md) e
[0075](../adr/0075-comando-alpha-e-cache.md).

Prova de sessão contínua: hello (23/10) e triangle (26/10) excederam o orçamento,
pausaram sem alterar o arquivo completo e restauraram bytes idênticos depois
de avançar. Arquivo inválido preservou a sessão; outro processo retomou o estado.
WM_CLOSE com save final: hello 594/723 ms (pausado/executando), triangle
2083/2421 ms, todos abaixo de 5 s. Resultados em target/session-hello-phase5/
e target/session-triangle-phase5-isolated-20261026/. O tempo inclui serialização
e gravação final de ~68 MB; é diferente da medição visual sem save acima.
Sondas de 26/10 mantiveram 45/0/302 pixels e 278/0/305922, respectivamente.
Gate target/gate/20261026-101417-ic6bJkvN.log: 586/261; testes de ferramentas
em target/phase5-tools-20261026.log: 19/TOOLS OK. Formatos e comandos nos ADRs
[0076](../adr/0076-arquivos-de-estado-vinculados.md),
[0077](../adr/0077-execucao-continua-e-comandos-host.md) e
[0078](../adr/0078-alpha-continuo-reproduzivel.md).

Limites: dois homebrews, kernel parcial (38/69 hello, 42/75 triangle), raster
software NV097 limitado; sem texturas/blend/renderer geral, áudio ou controles.
Gamma/filtros continuam pendentes (P2); Linux gráfico é P6; precisão e recortes
do raster permanecem P18. Jogos comerciais não foram habilitados por este marco.

## Histórico das entregas anteriores

Estado vigente do [ADR-0065](../adr/0065-consumidor-pushbuffer.md): PFIFO consome
comandos por PRAMIN/RAM e o triangle passa o polling por GET=PUT. BUS0 v8/PFFO v2;
v7 preserva controles sem consumo. Sonda com 13 raízes/17997 blocos recebe NV097
e para no formatador 0x1E9B6 -> 0x1E9B8; 66 chamadas/zero recusadas. Framebuffer
preto habilitado, sem efeitos gráficos ainda. Gate 535 testes/230 nativos debug/
release. Próxima tarefa 1.4 verifica snapshot no meio do pacote; depois PGRAPH.
As seções seguintes registram as entregas anteriores.


Estado vigente após o [ADR-0064](../adr/0064-controles-pfifo-persistentes.md):
37 controles PFIFO persistentes e aliases USER, BUS0 v7/PFFO v1. Snapshots v1–v6
conservam ausência de PFIFO. Sonda triangle: MMIO sem modelo 209 -> 170, PUT/GET
modelados; ainda sem avanço DMA ou imagem 3D. Gate Rust 1.99: 526 testes e 227
casos gerados debug/release. Próxima tarefa do PLANO: consumo real do pushbuffer.
Os relatos anteriores abaixo são registros das entregas indicadas.


Para retomar o projeto em outra sessão ou LLM, consulte
[CONTINUIDADE_ALPHA.md](../../CONTINUIDADE_ALPHA.md), com as seis entregas,
o estado de implementação, seus limites e os próximos passos.

Meta inicial: executar um XBE homebrew compilado por um SDK real, produzir saída
verificável e depois apresentar um framebuffer. Fixtures de bytes do projeto
continuam sendo testes; não substituem esse marco.

| Entrega | Estado | Aceite |
|---|---|---|
| 1. Memória/loader | aceite local concluído | T14 real + fixture, contrato sob Miri e benchmark seguro |
| 2. TLS/imports | infraestrutura validada; integração com homebrew pendente | TLS nxdk, FS e ABI disponíveis; dados exportados dependem da entrega 3 |
| 3. Kernel mínimo | memória contígua, modo, filtros, janela MMIO e espera por intervalo (ADR-0050, ADR-0051); 38/69 serviços | funções efetivamente usadas pelo homebrew; erro claro nas demais |
| 4. ISA necessária | nove raízes traduzem 10676 blocos; execução chega ao vídeo | traduzir o código emitido pelo compilador e validar instruções adicionais |
| 5. Execução integrada | executa main do hello; Session::conduzir, --fatias e interrupção pelo host; IRQ/vblank testados com fixtures | carregar módulo, rodar com orçamento, logs e encerramento controlado |
| 6. Framebuffer | apresentado em janela Win32 (ADR-0052), imagem idêntica à RAM guest; só Windows, bytes crus (sem gamma/filtros — provisório, ROADMAP §16.1) | primeira imagem apresentada; controle/áudio ficam para o próximo marco |

As seis entregas são uma divisão de trabalho, não garantia de seis commits ou prazo.
Dependências reais de kernel/ISA só serão conhecidas após compilar o alvo.

A retomada de 2026-10-22 usa triangle/pbkit como segundo alvo. O recorte PRAMDAC
do [ADR-0057](../adr/0057-pramdac-perfil-clock.md) acrescenta NVPLL programável,
perfil público explícito e BUS0 v3/NPLL v1. Bus novo informa 0x00011C01 e
233333324 Hz nominais. Snapshots BUS0 v1/v2 preservam o perfil antigo sem modelo;
não passam a ter PLL válido por migração implícita. O
[ADR-0058](../adr/0058-pfb-perfil-memoria.md) acrescenta CFG0/CFG1 com perfil
Cromwell conforme a arena de 64/128 MiB. BUS0 v4/PFBM v1 preserva os valores;
snapshots BUS0 v1–v3 continuam sem PFB modelado. Readback não implementa geometria
elétrica nem altera o tamanho da RAM quando o guest reprograma o registro.
O [ADR-0059](../adr/0059-consulta-memoria-instancia-gpu.md) atende a consulta
retail 64 MiB de MmClaimGpuInstanceMemory com reserva física real, fronteira
0x83FF0000/padding 0x10000 e KRNL v13/GINS v1. A reserva na primeira consulta é
provisória (P13); resize/devkit recusam. O
[ADR-0060](../adr/0060-pramin-armazenamento-instancia.md) acrescenta armazenamento
PRAMIN independente de 1 MiB (P14), BUS0 v5/PRAM v1 e migração v1–v4 sem modelo.
O [ADR-0061](../adr/0061-pagina-boot-dma.md) reserva a página D3D antes dos frames
XBE: PA zero/VA=0x80000000, RW sem execução, headers em PA=0x1000/RO. BUS0 v6/BTPG
v1 guarda a posse; v1–v5 migram sem reserva e preservam o layout/replay anterior.
TLS e imagem não podem ocupar a nova VA. Triangle preserva RAMHT/contextos,
passa a escrita/WBINVD e chega ao polling DMA_GET sem modelo. Com 12 raízes,
ISR/DPC executam; timeout leva a pb_kill. O [ADR-0062](../adr/0062-liberacao-memoria-contigua.md)
libera quatro recursos CMEM (133 frames), preservando os backings referenciados.
A sonda de 320 fatias termina por orçamento em 0x14D36, 93 chamadas/zero recusadas,
no laço PFIFO (CACHE1_STATUS sem EMPTY). Framebuffer segue diagnóstico: 1580 pixels/
29 linhas. Próximo: PFIFO/processamento real do pushbuffer, depois PGRAPH. P13/P14 e
renderização continuam pendentes. Registros por ADR abaixo são históricos; a passagem de bastão é vigente.

## Alvo escolhido

[nxdk samples/hello](https://github.com/XboxDev/nxdk/tree/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/samples/hello),
revisão `14d5ee97e73347c973f1f57b68b79ec08c9e77f2`, consultada em 2026-10-20.
O sample configura vídeo 640×480, imprime texto e usa Sleep no laço. Isso oferece
um objetivo observável para vídeo e temporização; seu laço termina pelo controle
de execução do host, não por um retorno espontâneo do programa.

O alvo foi compilado no WSL/Fedora e reconstruído neste Linux/Fedora 44, com
SDK/submódulos fixados e os mesmos hashes após normalização de metadata. O loader
verificou 4 seções, 27 páginas e 72 imports: 69 funções e 3 dados. Build,
inventário, hashes e limites estão em [`homebrew.md`](homebrew.md).

A infraestrutura de uma thread inicial prepara TLS nxdk, aplica FS e transporta
chamadas stdcall/cdecl/fastcall até o dispatcher. Os três dados exportados já têm
representação guest. Os primeiros 17 serviços são RtlInitAnsiString, criação/espera/liberação
de mutant, SHA-1 Init/Update/Final, consulta integral de EEPROM e a família
Init/Enter/Leave/TryEnter de seção crítica, leitura PCI de clock, InterlockedExchange, KeQuerySystemTime, KeGetCurrentIrql e NtAllocateVirtualMemory. Há variantes recusadas explicitamente;
a contagem de serviços não mede compatibilidade.

As seções críticas usam a estrutura pública Xbox de 28 bytes na RAM guest, com
recursão, contagens e identidade guest ativa (pai 1 ou filha 2). O prefixo KTHREAD
da filha é limitado ao bootstrap; não há escalonamento geral. TryEnter retorna FALSE em contenção; Enter
bloqueante e filas de espera recusam sem bloquear o host. O snapshot integral
preserva RAM, CPU, kernel e chamada pendente sem formato novo
([ADR-0037](../adr/0037-secoes-criticas-guest.md)).

O hello real traduz **10676 instruções/blocos** com as raízes `0x19110`, `0x145E0`,
`0x205E0`, `0x20889`, `0x13C70`, `0x18FC0`, `0x18C90`, `0x139B0` e `0x14F14`. Passa pelos
serviços anteriores e cria a filha com pilha de 64 KiB e TLS de 0x114 bytes.
Executa SystemRoutine, inicializa TLS/FLS no guest e entra em StartRoutine e
main_wrapper. A última raiz destrava um case do formatador do CRT. O namespace
guest começa sem D:; a consulta retorna ausência, a criação guarda o diretório
e open/close operam sobre handles reais. Isso não conecta mídia à VFS.
AvSendTVEncoderOption (2) agora responde GET_SETTINGS com padrão/flags da EEPROM
e cabo padrão, resultado 0x00480101 no perfil inicial. VIDEOENABLE guarda o
blanking; AvGetSavedDataAddress retorna NULL no cold boot sem imagem herdada.
MmAllocateContiguousMemoryEx (166) agora entrega **1.228.800 bytes/300 páginas
fisicamente contíguas para 640×480×32**. O hello recebeu VA 0x80050000 e PA
0x50000; o teste real sujou a região e comprovou que o memset guest a zerou.
AvSetDisplayMode (3) agora configura o modo 0x04010101 (NTSC-M 640×480/60),
formato 0x12/pitch 2560 e PA do recurso. O runtime lê as linhas BGRA por DMA,
sem compartilhar a arena. AvSendTVEncoderOption já registra os pedidos
flicker=5 e soften=false. O bloqueio atual é a escrita ao DAC gamma por MMIO,
**0xFD6813C8**, ainda sem mapeamento/modelo. A saída permanece desabilitada;
rampa gamma e apresentação ainda faltam
([ADR-0050](../adr/0050-pedidos-filtros-encoder.md)).

O [ADR-0045](../adr/0045-bootstrap-thread-guest.md) guarda a continuação do pai
sem copiar Clock/retired; término da filha restaura esse contexto. A filha roda
primeiro, sem preempção ou yield. Espera na filha terminada funciona; espera viva,
segunda criação, Sleep e escalonamento geral continuam pendentes. Não há garantia
de progresso do pai se a filha entrar em laço. O prefixo KTHREAD não é ETHREAD
completo. Snapshot AOTS v2/KRNL v7/KMUT v3 acrescenta THRD v1/TCXT v1, migrando
formatos anteriores sem threads. Recursos são retidos; NtClose de thread/mutant
e liberação faltam. KRNL v8 acrescenta SYML v1 para namespace/handles de link;
KRNL v1–v7 migram sem D:. NtClose só fecha links nesta versão. KRNL v9/AVST v1
preserva o encoder; v1–v8 migram com saída desabilitada. A EEPROM continua
autoritativa para o padrão/flags; não há estado duplicado ou tempo novo.

KRNL v10/CMEM v1 acrescenta os recursos contíguos. A janela 0x80000000+PA só
mapeia suas páginas RW, sem abrir acesso geral à RAM/código. O snapshot cruza
tradução linear, posse, bitmap e aliases; v1–v9 migram sem recursos. Há suporte
a limites/alinhamento e proteção RW com WRITECOMBINE opcional sob coerência
síncrona do Bus. Liberação e persistência ainda faltam.

AVST v2 guarda o modo, conferindo geometria, EEPROM e recurso CMEM antes de
publicar o estado restaurado; AVST v1 migra sem modo, preservando enabled.
KRNL continua v10. A visão empresta a RAM do Bus e expõe apenas pixels visíveis
de cada linha, excluindo o padding. Ela permite inspeção com saída desabilitada;
não implica scanout, vblank ou uma imagem já apresentada.

AVST v3 acrescenta os pedidos de flicker (0..7) e soften (0/1), com None no cold
boot e na migração de AVST v1/v2. Valores fora do perfil e Result não nulo
recusam antes do efeito. O estado registra intenção do guest; não aplica um
filtro analógico aos pixels nem altera a visão BGRA da RAM.

O perfil `CpuProfile::XboxRetailV1` define EBL_CR_POWERON (0x2A) nominal para
bus 133⅓ MHz × razão 5,5; demais bits são sintéticos, não dump físico. A leitura
TSC (0x10) e RDTSC usam o mesmo Clock. Índices sem modelo recusam com PC/índice,
sem efeitos. AOTC v7 salva o perfil e migra v1–v6; não há RDMSR host ou relógio
adicional ([ADR-0038](../adr/0038-rdmsr-perfil-cpu-guest.md)).

A consulta PCI atende o PLL do controlador DRAM e suas subleituras, com valor
público 0x01230801 e configuração compartilhada com CPU/MSRs no core. Escritas
ou áreas sem modelo recusam. KRNL v3/PCIC v1 guarda o perfil, com migração de
KRNL v1/v2; AOTC permanece v7. O hello real calcula **733333333 Hz**, conferidos
na RAM após o construtor ([ADR-0039](../adr/0039-configuracao-pci-clock-guest.md)).

InterlockedExchange usa ECX/EDX (FASTCALL/8), atualiza a RAM e devolve o valor
anterior em EAX. Valida toda a faixa antes do efeito; não há yield entre load/store
sob Bus exclusivo. Preserva snapshot/retorno capturado sem novo formato; outros
serviços Interlocked continuam ausentes
([ADR-0040](../adr/0040-interlocked-exchange-guest.md)).

KeQuerySystemTime escreve oito bytes em unidades de 100 ns desde 1601/UTC,
com referência sintética de 2000-01-01 no power-on, configurável e somada ao Clock
único. Session passa Clock ao dispatcher; não há tempo host ou contador paralelo.
KRNL v4/STIM v1 preservam a referência e migram v1–v3 para o padrão. A saída real
foi conferida em RAM: 125911584000000009 aos 704 ciclos. Erros de ponteiro/overflow
recusam antes de escrever ([ADR-0041](../adr/0041-tempo-sistema-guest.md)).

KeGetCurrentIrql consulta o nível explícito guardado no kernel para a única CPU
guest. O padrão é Passive; CLI/IF não alteram IRQL. KRNL v5/IRQL v1 preserva o
nível e migra v1–v4 para Passive. Raise/Lower, IRQ/APC/DPC, DIRQL e layout físico
de KPCR/APIC seguem ausentes. O hello real devolveu Passive com IF=0 e avançou
até a alocação ([ADR-0042](../adr/0042-irql-guest.md)).

NtAllocateVirtualMemory atende reserva+commit privado, base escolhida/ZeroBits=0,
PAGE_READWRITE, em Passive. Base alinhada a 64 KiB, tamanho a 4 KiB, first-fit
após a imagem inteira, frames próprios zerados e saídas validadas antes do efeito.
KRNL v6/VMEM v1 salva regiões e confere Bus/MMU/bitmap no snapshot. O hello recebeu
0x30000 e usou essa RAM. Reserva isolada, commit posterior e liberação faltam;
RW sem execução é política AOT ([ADR-0043](../adr/0043-alocacao-virtual-guest.md)).

A ISA já inclui CMPXCHG/LOCK, SETcc, RDTSC, CLI/HLT, INT3/#BP, SFENCE, CMOVcc,
BSF/BSR e CLC/STC/CMC. Blocos em funções separadas corrigiram o estouro de pilha
host. O controle guest e as exceções pendentes sobrevivem a snapshot, mas ainda
não há IRQ/NMI, IDT/SEH, scheduler geral ou retomada automática dessas exceções.

A validação atual no Linux tem **519 testes workspace/doctests** e **224 casos de
Rust gerado em debug/release**, incluindo PRAMIN/PFB/NVPLL, memória e snapshots. O Rust
fixado é 1.98.1. Hello executa main/apresenta em Win32; o segundo alvo triangle
passa do PLL/PFB e da consulta de instância GPU, mas ainda não renderiza.
Scripts não mudaram; a apresentação Win32 não foi revalidada aqui.
O aceite não cobre GPU 3D ou scheduler geral. A primeira
entrega de memória usou mídia comercial apenas como dado do loader; a BIOS não
foi usada. A EEPROM padrão continua sintética, com bytes/origem explícitos.
