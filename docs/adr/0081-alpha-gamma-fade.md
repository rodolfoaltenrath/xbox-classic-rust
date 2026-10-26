# ADR-0081 — Terceiro homebrew: gamma-fade do nxdk

Data: 2026-10-26. Estado: aceito. PLANO 6.3.

## Alvo e decisão

Adicionar gamma-fade ao preparo, sonda e launcher existentes, sem caminhos
de execução específicos para título. Entrada padrão:
target/homebrew-gamma/gamma-fade-raw.xbe, ignorada pelo Git. Build com
build-nxdk-sample.sh e nxdk fixado em
14d5ee97e73347c973f1f57b68b79ec08c9e77f2, sem baixar dependências.
Preservar o XBE original é necessário para retomar estados ligados ao arquivo
exato; outro build pode mudar seus metadados.

Fonte primária:
[sample gamma-fade](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/samples/gamma-fade/main.c).
O guest desenha gradientes RGB na RAM e calcula/programa a rampa pela HAL;
XVideoWaitForVBlank/GetTickCount controlam as transições. O host não calcula
o fade nem identifica o sample. DAC: perfil público limitado do ADR-0079;
transformação digital: ADR-0080.

Launcher conserva guest.bgra e acrescenta display.bgra. Sonda inclui
perfil/cursor/entradas distintas da identidade. A verificação Win32 compara
o DC capturado por BitBlt com a saída apresentada; RequireDifference exige
também diferença da RAM. O teste de sessão conserva pausa/save/restore/erro
recuperável/outro processo e exige gamma não identidade na sessão restaurada.

## Entrada desta validação

- XBE: 9e7032f58279b2d7291313ddb45a9b845c947252bdfa0ae3260795997beddeb8.
- PE: 7114d78ff8ea8d210dd51f2595a850f94c7fab77bcf6a0b0bc00c4427cb8d8b1.
- Análise inicial: 13229 blocos, sem --entry manual e sem bloqueio de tradução.
- Inventário: 75 imports, 71 funções/4 dados, 4/4 dados representados e
  38/71 serviços com variantes implementadas. Isso não mede os serviços chamados.

O primeiro orçamento de 64 fatias terminou em identidade, com 44 chamadas/zero
recusadas. Avançar esse estado encontrou o callback ausente em 0x115B4,
alvo 0x116B0, com 53/zero. ADR-0082 corrige a perda de candidatos através de
MOVSS; nova tradução tem 13255 blocos sem entradas manuais. O Rust emitido
de hello/triangle permanece byte a byte idêntico após essa correção.

## Evidência final

Gate: target/gate/20261026-111413-Waq1DjEq.log, GATE OK/595 testes;
266 casos gerados debug e release (native-release-final.log em
target/gamma-phase6). Dois testes de análise mais um caso nativo verificam
o ADR-0082; todos os testes anteriores continuam ativos.

Investigação com executável Rust nativo debug, sem interpretador: restaurar
second.obsstate e avançar oito fatias chegou a 69 chamadas/zero recusadas,
22099097 instruções/73406772 ciclos/100 ms, 255 entradas não identidade.
RAM: 299222 pixels/480 linhas. Duas continuações independentes de uma fatia
do mesmo estado produziram arquivos integrais idênticos, SHA-256
e650acbc3d3764b800bb31717083ed49bd6dfb7cb82a102f3e15fdd61f611f55.
A saída também coincidiu, 22cc6a75dbbca44bfceda771a1e11a0557854104816b410131b6763bd47b0839,
e difere da RAM bruta (7b7ecd1b5845998c3dcc89b1c89ce7107b8ad656dceff89b181fac16a143869b).

verify-window com -LoadState/-Slices 1/-Presented/-RequireDifference,
target/gamma-phase6/visual-gamma-debug: captura real BitBlt 640×480,
298294 pixels/480 linhas, zero diferenças RGB, fechamento 148,6615 ms.
Imagem inspecionada: gradientes RGB escurecidos, bordas cinza e título
"Mix fade to black (Linear)". O snapshot final coincide com o replay acima.
Não usar isso como medida de desempenho do console.

Release, sonda gamma-fade/128: 161 chamadas/zero recusadas, 23199969 instruções,
354799260 ciclos/483 ms sintéticos, 255 entradas não identidade e zero MMIO
sem modelo. Log target/sonda/gamma-fade-20261026-112938-2cwi0vUS.log.
Win32 em visual-gamma-release: zero diferenças RGB, close 71,7051 ms,
saída SHA 39ee2ca0802bb26a703a82780105f2769f4f81e0ef6206d6affa705d74a6ef1a,
RAM mantém o SHA anterior. Restaurar debug-next em release e avançar uma
fatia também reproduz exatamente snapshot e imagem dos dois replays debug;
visual-gamma-restored-release, close 122,3156 ms/zero diferenças.

Controle contínuo release: session-gamma-release/result.json valida pausa
sem avanço, F5/F9, recusa de arquivo corrompido mantendo sessão e carregamento
em outro processo. Snapshot pausado/restaurado SHA
6bd6a5fb7261155c09a77522d3543f3f4acec1ac5ef62e752267a9dfa53ba723,
26646187 -> 30056597 instruções antes do restore; zero recusadas.
WM_CLOSE com save final: 1160,5467 ms pausado e 959,7528 ms executando.
Saída restaurada continua distinta da RAM (SHA
02b280494e121a26e84324362312a1b7f6056fd2542b6b852481f7961d0b23a5).

Comando público --continuo --carregar em target/alpha/runs/gamma-fade-SMskuCZc:
30820 chamadas/zero recusadas, fechamento por host/status0 em 804,7775 ms,
final.obsstate de 68287494 bytes; build Cargo reutilizado em 0,18 s.
Modo finito headless também validado em gamma-phase6/alpha-public/runs/
gamma-fade-J3BxRvny, orçamento/status1 preservado e ambos os BGRA exportados.

Hello/64 mantém 45/zero, 302 pixels/12 linhas, zero MMIO sem modelo;
sonda hello-20261026-111845-AhleASQU.log. Win32 hello/triangle mantém SHA
das imagens de referência, zero diferenças RGB e igualdade bruto/DAC;
close 92,4166/161,7448 ms. Sonda final triangle/320 preserva 278 chamadas/zero
recusadas, 231556968 instruções/296627592 ciclos/404 ms, 305922 pixels/479
linhas, nove draws/27 triângulos/4147200 fragmentos. Gamma identidade,
163 MMIO sem modelo (antes 165, duas portas do DAC agora modeladas).
Log target/sonda/triangle-20261026-111855-dp7h9SQX.log.
Launcher público também validado para hello/triangle, em
target/alpha/runs/hello-n7hEyUKC e triangle-5TDnX1Gt; builds reutilizados.
Gamma-fade sem --projeto usa o índice existente e conserva os mesmos resultados,
target/alpha/runs/gamma-fade-HVCh0Ks4. Os três estão prontos no cache local;
outra máquina precisa preparar entradas/SDK e fazer o primeiro build.
Ferramentas: 21 casos/TOOLS OK, tools-final-source.log, SHA dos scripts
cb57af9101500705aa85747b5e802de6cdf10ebac907e1bccff50c72d932823e.

## Limites

Não valida filtros do encoder, sinal analógico, GPU completa ou jogos
comerciais. Clock/vblank seguem os modelos determinísticos já declarados;
não se infere FPS físico ou desempenho do console a partir deles. Linux
continua sem apresentação. Firmware proprietário não é usado.
