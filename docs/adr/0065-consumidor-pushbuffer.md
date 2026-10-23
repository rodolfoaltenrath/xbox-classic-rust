# ADR-0065: Consumidor limitado do pushbuffer NV2A

- **Status:** aceito para consumo/telemetria; sem efeitos gráficos.
- **Data:** 2026-10-23
- **Fase:** 5 / PLANO 1.3
- **Base:** `34b9801`

## Contexto e fontes

Os controles PFIFO persistem desde o ADR-0064. A instância ativa observada no
ADR-0063 é 0x111C, um descritor linear de leitura desde RAM zero. O guest escreve
um comando jump na página de boot e espera que GET alcance o head do pushbuffer.
O passo agora é buscar e decodificar essa palavra, preservando a configuração
programada e recusando descritores inválidos.

Fontes públicas fixadas no ADR-0063:
[nxdk pbkit.c](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/pbkit/pbkit.c),
[xemu nv2a.c](https://github.com/xemu-project/xemu/blob/03b988c9d76553b33ed712ba00822ffdf49f0e7d/hw/xbox/nv2a/nv2a.c),
[pfifo.c](https://github.com/xemu-project/xemu/blob/03b988c9d76553b33ed712ba00822ffdf49f0e7d/hw/xbox/nv2a/pfifo.c)
e [nv2a_regs.h](https://github.com/xemu-project/xemu/blob/03b988c9d76553b33ed712ba00822ffdf49f0e7d/hw/xbox/nv2a/nv2a_regs.h).
O formato de descritores/pacotes, campos RAMHT e comportamento de call/return
foram confrontados com essas referências. Não foi copiada implementação de
terceiros; não há BIOS nem valor atribuído como medição do console.

## Alternativas consideradas

1. Atualizar GET no store PUT: não busca comandos e viola o contrato do projeto.
2. Executar GPU em thread separada já nesta etapa: exigiria outro protocolo de
   concorrência, relógio e snapshots antes de existir um consumidor verificável.
3. Consumidor serial com orçamento e estado explícito: escolhido como perfil
   limitado P16, preparando a integração gráfica e sincronização posteriores.

## Decisão e divisão de responsabilidades

`xbox-gpu::pushbuffer` decodifica cabeçalhos sem memória, MMIO ou dependência do
Bus. `xbox-mem` passa a depender desse crate interno; o estado PFIFO continua no
Bus, ao lado da PRAMIN e RAM que o alimentam. Não há cópia de descritores ou RAM.
O decoder reconhece jump antigo/novo, call de um nível, return, métodos com
incremento e sem incremento. Header reservado, call aninhado e return sem call
recusam explicitamente.

Bus::service_gpu limita o trabalho por palavras, incluindo cabeçalhos. O pusher
exige canal DMA zero, acesso de push/pull habilitado e ausência de suspensão DMA.
Lê a instância programada, valida classe de leitura 0x02/0x3D e alvos lineares
UMA NVM/PCI; AGP/tiled, outros canais e PIO não são executados. O endereço usa
frame/adjust e cada fetch verifica DWORD alinhado, limite inclusivo do descritor,
soma sem overflow e faixa real da RAM. O limite lógico não autoriza acessar
bytes fora da arena. Não depende de mapeamento CPU nem de frames virtuais.

Somente depois de interpretar uma palavra válida publica o GET correspondente.
O jump do boot usa o destino decodificado; não se reconhece o sample ou se copia
PUT para GET. Um erro preserva a palavra inválida pendente, mas não desfaz o
prefixo de comandos já consumidos. GPU é um consumidor independente; erro após
uma fatia CPU não reverte instruções já executadas. Não há avanço de Clock pela
GPU neste perfil.

DMA_STATE preserva método/subcanal/contagem/tipo do pacote incompleto;
DMA_SUBROUTINE preserva retorno/estado do call. GET==PUT com parâmetros ainda
pendentes aguarda outro PUT. Métodos incrementam endereço por parâmetro; os de
tipo não incremental mantêm o endereço. Um limite de palavras interrompe o
processamento sem perder esses campos.

O método zero consulta RAMHT e liga o objeto ao subcanal, conferindo handle,
validade, canal, engine e instância. O hash usa os campos de tamanho/base
programados; colisão/handle ausente recusa em vez de escolher outro objeto.
Parâmetros de métodos 0x180..0x1FC resolvem handles de contexto para instâncias.
Métodos gráficos ainda sem efeito são registrados por classe/subcanal/endereço,
com contagem e primeiro/último parâmetros brutos e resolvidos. O limite é de
4096 chaves distintas; excedê-lo recusa, sem descartar silenciosamente. O resumo
não é um trace de todos os parâmetros intermediários.

O CLI gerado imprime classe, método e contagem dos registros em qualquer saída.
Registrar um método não aplica seu efeito gráfico: PGRAPH continua para a Fase 2
do plano. A ligação de objeto e a resolução de handles são efeitos do frontend.

## Execução e estados de fila

Session::conduzir chama o consumidor com até 4096 palavras após uma fatia normal,
uma fronteira de kernel ou retorno. Breakpoint/HLT não causam serviço implícito.
Não há worker, unsafe ou novo relógio. `guest::run` isolado continua executando
apenas CPU; testes/embedders podem chamar service_gpu explicitamente.

No perfil serial, CACHE1_STATUS e RUNOUT_STATUS têm EMPTY quando GET==PUT e não
há parâmetro pendente. Ao anunciar dados novos ficam não vazios até consumo real;
ler ou escrever PUT não conclui a fila. Isso representa a fila serial deste
perfil, não os caches/latências físicos do NV2A. DEBUG_0 e demais offsets fora do
recorte continuam sem modelo. Não se criam IRQs de PFIFO/PGRAPH nessa etapa.

## Snapshot e migração

BUS0 v8/PFFO v2 acrescenta CommandsV1. Os 37 controles continuam em ordem fixa;
perfil 2 acrescenta bytes com PCMD v1. Esse estado guarda oito bindings opcionais
canônicos (handle/instância/classe) e até 4096 registros de métodos ordenados,
com contadores não nulos. Marcadores, geometria, classe, ordem, contagem, tamanho
e truncamento inválidos recusam antes de publicar PFIFO. Payload PCMD é limitado
a 160000 bytes e não admite sobra. RAM, PRAMIN e demais registros permanecem únicos.

BUS0 v7/PFFO v1 restaura RegistersV1, sem ativar consumo ou estados de fila novos.
BUS0 v1–v6 conserva LegacyUnmodeled. Regravar qualquer perfil preserva a escolha;
abrir uma sessão nova adota CommandsV1. As fixtures antigas retiram também o
payload PCMD ao compor versões anteriores. KRNL/AOTC/AOTS não mudam.

## Consequências e como reverter

P16 declara o consumidor serial: só canal zero e memória linear, orçamento fixo
por fronteira CPU, sem timing, IRQ gráfico ou efeitos PGRAPH. A condição de saída
é integrar métodos, sincronização/backpressure e perfis com evidência, preservando
snapshots. O perfil RegistersV1 permanece legível sem promoção silenciosa.
Reverter requer migração explícita de CommandsV1 para ausência de consumidor;
não rebaixar a versão externa ou jogar fora pacotes pendentes silenciosamente.

## Verificação

Dois testes de decoder verificam as formas de cabeçalho e campos extremos.
Sete testes de memória verificam leitura do jump de boot, métodos/handles,
pacote incompleto, orçamento/call/return, descritores/limites/RAMHT inválidos,
headers inválidos e migração v7. Três casos gerados exercitam a integração em
conduzir, snapshot antes do consumo e erro de descritor sem alterar CPU/Clock.
Gate Rust 1.99: 535 testes (target/gate/20261023-005858.log); 230 casos gerados
em debug/release, este com warnings negados. A sonda real será a evidência do
primeiro polling encerrado por consumo, não pelo timeout.

### Tradução e sonda de bootstrap

A primeira sonda com as 12 raízes antigas confirmou o consumo e parou depois em
UnknownTarget: PC=0x19647, destino=0x196CF. A desassemblagem e a tabela de 10
entradas em VA=0x2F1B8 mostram o switch de pb_ColorFmt; o destino observado é o
caso A8R8G8B8/32 bits. Foi acrescentada essa raiz explícita, sem alterar a análise
P5 ou incluir instruções inventadas. São 13 raízes/17997 blocos no artefato
`target/homebrew2/dma-review-20261023`; scripts/sonda.sh aponta para ele. Fontes,
XBE e SDK continuam os mesmos. Outros formatos do switch não são necessários
para esse perfil e não foram promovidos a compatibilidade medida.

A sonda de bootstrap com a tradução anterior já comprovou GET=PUT=0x18C000,
retired/ciclos=25749621, EIP=0x18F8C, 54 chamadas/zero recusadas e snapshot integral
estável. Ela não comprovou a chegada a métodos NV097: o assert correspondente
falhou depois do destino não traduzido. Não é um erro de DMA nem evidência de
renderização. A captura seguinte precisa atravessar o novo destino e listar
os métodos para fechar o aceite 1.3.

### Aceite real da tarefa 1.3

A sonda com 13 raízes passa o destino de cor e recebe métodos: log oficial
`target/sonda/triangle-20261023-010537.log`, 66 chamadas/zero recusadas,
retired/ciclos=62188772. Termina em outro destino AOT ausente, 0x1E9B6 -> 0x1E9B8,
no formatador de texto, fora de pb_kill. São 257 chaves/376 parâmetros: classe 97
com 249/366, classe 39 com 2/2, classe 62 com 3/5, classe 9F com 3/3. Os números
incluem ligações de objeto (método zero). O framebuffer está habilitado/preto;
não há triângulo ou clear gráfico aplicado ainda. MMIO sem modelo: 170.

A sonda `target/sonda/triangle-dma-root13-probe-20261023.log` passou: confirma
GET=PUT pelo jump real antes do timeout, snapshot integral estável e métodos
NV097 ao continuar. Gate final: `GATE OK: 535 testes`,
`target/gate/20261023-010738.log`; 230 casos gerados debug/release. O limite atual
é a tradução do formatador e a ausência de efeitos PGRAPH, não a espera inicial.

### Aceite da tarefa 1.4 — estado no meio do pushbuffer

Três testes adicionais em crates/xbox-mem/tests/pushbuffer.rs verificam:

- Snapshot após header e após dois parâmetros, com o restante ainda pendente;
  restauração produz o mesmo GET, registros e snapshot integral da execução contínua.
- Snapshot com call ativo e objeto ligado; return, jump e parâmetros posteriores
  reproduzem endereço final/retorno vazio e os mesmos métodos/bytes.
- PCMD corrompido: versão, marcador, instância, classe, limite de registros e
  contador zero recusam antes de publicar PFIFO.

Gate Rust 1.99: `GATE OK: 538 testes`, log target/gate/20261023-011523.log.
Os 230 casos gerados continuam passando; não houve mudança de formato ou runtime.
