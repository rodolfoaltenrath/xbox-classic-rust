# ADR-0078 — Iniciador e evidência do alpha contínuo

Data: 2026-10-23. Estado: aceito para PLANO 5.3. Base `1361f4a`.

## Decisão

run-alpha.sh aceita --continuo, --carregar e --salvar, sem mudar o modo finito
padrão. Resolve caminhos a partir do chamador antes do cd; recusa opções
repetidas/incompletas, arquivo de entrada ausente e destino de saída existente
ou sem diretório antes de preparar/compilar. --continuo exige janela Windows.
O próprio runtime continua validando vínculo XBE/integridade e publicação
sem sobrescrita: a verificação do script não substitui a verificação final.

Cada rodada contínua recebe diretório exclusivo `states/` para F5 e salva
`final.obsstate` ao encerrar, salvo --salvar explícito. --carregar seleciona
arquivo de uma rodada anterior; não há escolha automática por nome/timestamp.
start-alpha.ps1 encaminha argumentos com caminhos intactos e startup Normal
para a janela. Modo finito headless também suporta salvar/carregar, inclusive
Linux. Snapshots finais e logs permanecem separados de guest.bgra.
O build/cache continua validando imagem/lock/fontes do ADR-0075; estado não é
usado como identidade de código. SAVE v1 e formatos internos permanecem iguais.

*.obsstate e temporários .obsidian-state-*.part são ignorados pelo Git, mesmo
fora de target: contêm XBE e RAM guest. Não há compressão, criptografia ou
promessa de durabilidade sob falta de energia. Pausa e estados são comandos
do host, não suporte USB/XID. P2/P6/P8/P9/P12–P20 permanecem abertos.

## Método de verificação

verify-session.ps1 inicia o executável real já compilado, exige diretório novo
e envia WM_KEYDOWN/WM_CLOSE à sua própria janela. Dois trechos completos
excedem o orçamento padrão antigo. Espaço pausa, duas gerações F5 separadas
por 350 ms devem ter SHA-256 idêntico. Retoma até outro trecho, pausa com
CPU/Clock maiores, F9 restaura e novo F5 precisa reproduzir o arquivo inteiro,
incluindo RAM, kernel, GPU e threads. Não compara apenas o framebuffer.

O teste corrompe somente a terceira geração criada por ele, exige erro F9 e
conservação da sessão/gerações anteriores. Fechamento pausado salva arquivo
idêntico; outro processo carrega explicitamente a primeira geração, confirma
CPU/Clock iniciais, avança e fecha em execução. Ambos devem sair por host/zero,
sem kernel recusado, em até cinco segundos (inclui exportação final do estado).
Não constrói screenshot a partir do arquivo esperado. verify-window.ps1
continua sendo a ferramenta separada de readback RGB da superfície Win32.

No WSL/NTFS, três casos auxiliares atingiram o timeout existente de 20 s
durante builds concorrentes. O novo test-tools.py copia **os mesmos bytes**
dos scripts/testes a um Git fixture temporário no Linux, confere a cópia e
registra SHA-256. Todos os 19 testes passam sem trocar limites/asserções.
Não executa Cargo real nem escreve índice/configuração do repositório real.
O primeiro teste release foi interrompido para reduzir concorrência; repetir
release usa o mesmo target/cache e um log final separado.

Retomada em 2026-10-26: a execução anterior foi interrompida antes de emitir o
relatório final do triangle, portanto não foi usada como aceite. Repetir a
validação visual depois do gate, sem builds concorrentes. O parâmetro já
existente `-TimeoutSeconds 300` limita a espera por progresso do sample; o
prazo de WM_CLOSE permanece cinco segundos. Uma tentativa concorrente atingiu
os 120 s padrão; critérios, asserções e orçamentos guest não foram alterados.

## Evidências

Conclusão em 2026-10-26: GATE OK 586 em
target/gate/20261026-101417-ic6bJkvN.log; 261 gerados debug e 261 release
(target/phase5-native-release-final.log, 23/10); 19 ferramentas/TOOLS OK em
target/phase5-tools-20261026.log. Hash dos scripts copiados na última execução:
`a6501e01ec559941e19235823758660f987d516c77031c68912eb44f0c0ac4fa`.

| Sessão | Resultado | WM_CLOSE pausado / executando |
|---|---|---|
| Hello, 23/10 | Pausa integral, restore idêntico, falha recuperável, outro processo | 593,7587 / 722,6875 ms |
| Triangle, 26/10 | Mesmas provas; 508359498 -> 785161898 instruções antes de restaurar | 2082,6871 / 2421,4008 ms |

Ambas saíram por host/status zero e zero serviços recusados. Resultados JSON
em target/session-hello-phase5/ e
target/session-triangle-phase5-isolated-20261026/; a pasta original
session-triangle-phase5/ ficou incompleta na interrupção e não é o aceite.
Os arquivos integrais de pausa/restore têm SHA-256 respectivamente
`31b329e9b2e9302ad623436565d7c1ab2732d5b711d48786cb2870e20bd324e4`
e `47cee3811a5a96bcef21ff8b9b008e1feb020f57330d55779ce62a5717719f7d`.

Sondas de 26/10: hello 45/0, 302 pixels/12 linhas; triangle 278/0,
305922/479, R4977/G4998/B4977. Readback Win32 confirma zero diferenças RGB
em target/session-hello-frame-phase5/ e
target/session-triangle-frame-phase5-20261026/. Nenhuma imagem foi usada
para fabricar o readback. Iniciador real --continuo --carregar validado nas
rodadas hello-LAfc6doM (target/alpha-live-smoke-hello/runs) e
triangle-LqNrxSMj (target/alpha/runs): status 0 e final.obsstate criado.
Build reaproveitado em 0,42/1,46 s; close do launcher 1075,864/2509,4007 ms.
Detalhes em PLANO 5.3, CONTINUIDADE_ALPHA.md e docs/dev/alpha.md.
Não extrapolar dois homebrews para compatibilidade com jogos comerciais.
