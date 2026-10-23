# ADR-0073 — Reprodução dos samples entre máquinas

Data: 2026-10-23. Estado: aceito. PLANO 4.1; base `17cbc1d`.

## Contexto e decisão

O plano anterior produziu triangle em RAM no Linux, mas a sonda apontava para
projetos gerados sob target naquele computador. Esses projetos não são
versionados, e o template/emissor dos ADRs 0071/0072 exige nova tradução.
O novo bloco autorizado pelo mantenedor valida os samples no Windows antes
de ampliar a compatibilidade. Não acrescenta modelo de hardware.

`prepare-sample.sh <hello|triangle> [xbe]` traduz com o compilador do workspace
para um diretório novo em target/samples, sem --entry manual. Preserva o projeto,
log de tradução, SHA-256 da entrada e versões de Rust/Cargo/revisão do repositório.
O build nxdk continua separado: não baixar SDK nem jogos para preparar o sample.
Entrada ausente é erro explícito com a localização esperada.

`sonda.sh` prepara por padrão uma tradução atual. SONDA_XBE seleciona a entrada;
SONDA_PROJETO reutiliza um projeto explicitamente selecionado, que deve conter
manifest, imagem, módulo gerado e main. Quando ambos são fornecidos, exige
imagem idêntica. Isso permite reaproveitar a compilação sem editar caminhos
versionados. Reutilização explícita não afirma atualidade do emissor; após
alterar emissor/template, preparar novamente.

SONDA_TARGET_DIR seleciona o cache, por padrão separado por sample. Tradução,
build release e execução são etapas diferentes. Build falho não se torna uma
sonda com métricas vazias. O executável é iniciado diretamente, com nome Unix
ou .exe; o log preserva o status real do guest. BudgetExhausted (status 1 com
diagnóstico preciso) é uma parada esperada para esses samples em loop, mas
qualquer outro erro propaga status. Resumo exige Kernel e Framebuffer presentes;
não afirma o aceite sozinho: conferir recusadas, parada e cores/draws.

Logs novos usam mktemp para não colidir entre rodadas. Ferramentas não apagam
nem sobrescrevem projetos anteriores e não tornam a arena compartilhada.
Não modificam formatos de snapshot nem políticas de CPU/kernel/GPU.

## Evidência de entrada

XBEs disponíveis neste Windows, distintos dos hashes Linux registrados no ADR-0072:

- hello.xbe: `baaa88110014743864d0f38606e18eb423b0bf0da5bb4083a3e55b89a0a2af7f`;
- triangle-raw.xbe: `e2571d9daad266938049920d15ed0401f75c02d6a075b23fc86bab1861d5aad2`.

As traduções locais sem entradas extras produziram 12345 blocos (hello) e
19707 blocos (triangle). Comparar desempenho/contagens entre máquinas somente
identificando entrada, perfis, orçamento e compilação; diferença de hash impede
afirmar identidade binária com a referência Linux só pela aparência da imagem.

Seis testes Python com Cargo/guest controlados cobrem preservação de projetos,
hash/entrada com espaços, nome .exe, status de tradução/build/runtime, orçamento
falso, métricas ausentes, limites inválidos e projeto incompatível com XBE.
Executar: `python3 scripts/tests/test_sample_tools.py` (Linux ou WSL).
Essa suíte verifica a orquestração; as sondas reais continuam obrigatórias.

## Gate por plataforma

O primeiro gate Windows (`target/gate/20261023-151002.log`) passou com **580
testes**, Rust 1.98.1 e 253 casos gerados. A referência Linux tem 576: os cinco
testes Win32 substituem um teste de backend indisponível. A diferença vem de
cfg da plataforma, não de teste removido.

Gate passa a usar gate-baseline-windows.txt=580 e gate-baseline-linux.txt=576,
mantendo o piso já demonstrado em cada SO. O gate-baseline.txt anterior, que a
primeira execução elevou para 580, permanece como fallback para outros hosts.
Arquivo ausente/inválido falha; não inicializa o piso conhecido com zero.
Novas contagens só sobem automaticamente. Não reduzir pisos para acomodar falha.

Quatro testes do gate em workspace temporário protegem os dois pisos, contagem
inferior, atualização isolada e baseline ausente/inválido. A primeira execução
desses testes no WSL revelou que aquele Fedora não tinha awk. A contagem passou
a usar Bash, sem instalar ferramenta de sistema ou aceitar métricas vazias.
O teste de duas rodadas rápidas também revelou colisão de nomes por segundo,
somando logs e escondendo uma queda de contagem. Gate agora cria log exclusivo
com mktemp, como a sonda. Os testes protegem essa recusa em rodadas consecutivas.

Gate final das ferramentas: `target/gate/20261023-152653-XlQbs6tT.log`, **580
testes**, 253 casos gerados; seis testes de samples e quatro do gate passaram
separadamente no WSL. A primeira sonda Windows do hello, 64 fatias, terminou por
orçamento: 45 chamadas, zero recusadas, 17290355 instruções, 2023 ms guest,
302 pixels/12 linhas, dois endereços MMIO sem modelo.
Log: `target/sonda/hello-20261023-151407-gMdXUwHB.log`. É uma linha de texto;
o histórico Linux com quatro linhas/1208 pixels não foi declarado idêntico.
O build release inicial levou 13m58 neste host, separado da execução.

Triangle, 320 fatias, projeto target/samples/triangle-NBBSOjUk: **278 chamadas,
zero recusadas, 231556968 instruções/296627592 ciclos/404 ms guest**, orçamento
em EIP=0x16440. Framebuffer habilitado com **305922 pixels/479 linhas**,
R=4977/G=4998/B=4977, nove draws/27 triângulos/4147200 fragmentos e 165 MMIO sem
modelo. Log target/sonda/triangle-20261023-151407-XlZPyXo0.log; build inicial
release 22m03. Essas contagens coincidem com a referência Linux, mas a entrada
tem outro hash; a apresentação real e comparação RGB são o aceite separado 4.2.
