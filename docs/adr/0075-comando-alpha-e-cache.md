# ADR-0075 — Comando de uso do alpha e reaproveitamento do build

Data: 2026-10-23. Estado: aceito para PLANO 4.3. Base `6de0aa4`.

## Decisão

`bash scripts/run-alpha.sh <hello|triangle>` prepara, compila em release e abre
a janela no Windows/Git Bash. Orçamentos padrão: hello=64, triangle=320 fatias.
Opções: --xbe, --projeto, --fatias, --sem-janela e --help/--ajuda. Argumentos
desconhecidos, repetidos, incompletos, limites inválidos e arquivos ausentes
falham antes do build. Caminhos explícitos são resolvidos a partir do chamador,
antes de mudar para a raiz. Em Linux a janela recusa com orientação para
--sem-janela, sem apresentação simulada ou download de entrada/SDK.

Preparo sempre chama o emissor atual numa pasta nova, sem --entry manual.
Generate-lockfile offline fixa as dependências standalone antes de comparar
o projeto. A identidade é SHA-256 dos seis arquivos: Cargo.toml, Cargo.lock,
src/image.xbe, src/generated.rs, src/lib.rs, src/main.rs. Reutilizar um projeto
compilado exige identidade igual à preparação atual. Configurações/alvos extras
(build.rs, .cargo ou src/bin) impedem essa reutilização. --projeto divergente
recusa, preservando-o; o cache automático divergente reconstrói em pasta nova.
Depois do build aprovado, seu índice inválido é arquivado e substituído por
rename de arquivo completo. Não sobrescrever código/imagem de projeto anterior.

Cargo build é executado mesmo num acerto do cache: os crates do workspace,
toolchain e flags continuam sujeitos às verificações do Cargo. Não tratar
hashes dos seis arquivos como assinatura de todo o código do runtime externo.
O objetivo é reaproveitar a mesma biblioteca AOT quando seus arquivos continuam
iguais; mudanças do emissor, imagem ou lock não são ignoradas. O projeto recém
preparado também permanece para auditoria.

Estado padrão: target/alpha/cache e target/alpha/runs. Cada rodada tem pasta
exclusiva, build.log, stdout.log, stderr.log, guest.bgra, project.txt,
prepared-project.txt e build-identity.txt. Windows também grava pid.txt.
ALPHA_STATE_DIR muda a raiz de estado e ALPHA_BUILD_DIR o cache Cargo;
caminhos relativos dessas variáveis são relativos ao repositório.

## Execução e encerramento

No Windows start-alpha.ps1 inicia o executável interativo com janela normal,
preserva Handle/ExitCode conforme ADR-0074 e aguarda o processo. --sem-janela
usa startup oculto para o processo sem GUI. Interrupção do launcher limpa
somente o processo que iniciou. Esse helper não contém lógica Xbox.

Status de tradução, dependências, build e execução são propagados. Preparo,
build e execução têm mensagens/logs separados: tempo de compilação não mede
velocidade guest. BudgetExhausted continua status 1, com mensagem clara de
orçamento; não se transforma em sucesso zero nem esconde erro de kernel.
Com janela, o frame final permanece até X. Fechamento durante run retorna
status 0/host; após orçamento preserva status 1. Logs e BGRA ficam disponíveis.

Não muda serviços de kernel, ISA, modelo GPU, relógio guest ou snapshots.
Mantém P2/P6/P18 e demais provisórios. O comando demonstra dois homebrews;
não anuncia jogos comerciais, áudio, controles ou renderer completo.

## Verificação

Seis testes de launcher com ferramentas controladas cobrem cache exato,
edição preservada/reconstrução/novo acerto, lock divergente, status reais de
falhas, parâmetros Windows intactos e argumentos/caminhos inválidos. Seis
testes de preparo/sonda cobrem também erro de generate-lockfile. Quatro testes
do gate mantêm pisos e logs exclusivos. Mocks validam somente orquestração;
a evidência visual real é a do ADR-0074 e os comandos reais abaixo.

Rodadas reais sem janela inicializaram o cache com os projetos da 4.2:
target/alpha/runs/hello-90oM6GZm e triangle-k8HRUL7B. Depois os comandos padrão
sem --projeto abriram as janelas, reutilizando os projetos validados:

- `bash scripts/run-alpha.sh hello`: hello-XfFakWTu; Cargo 0,20 s; X fechou
  em 58,3149 ms, status 1/orçamento, BGRA SHA-256
  `936b66e23452af9287bf267806225de155205de0ff4ffd0e795d065e9550f0d7`.
- `bash scripts/run-alpha.sh triangle`: triangle-mM890OSS; Cargo 0,23 s;
  X fechou em 54,859 ms, status 1/orçamento, BGRA SHA-256
  `76ac9e08b6e8e7a2aeaf286d5f9e60aabd2b51c9580a0cdf025b182f9089f132`.

Entrada real `target/homebrew/hello alpha spaces.xbe` e
ALPHA_STATE_DIR="target/alpha spaces" passaram pelo backend Windows em modo
sem janela, pasta runs/hello-CU9eCXwu, com o mesmo SHA-256 BGRA do hello.
Os arquivos originais de entrada não foram alterados. As primeiras compilações
frias continuam demoradas (medições anteriores 13m58/22m03); os tempos acima
são checagens/reaproveitamento do Cargo, não primeiros builds nem FPS.

Gate final: **GATE OK: 580 testes**, target/gate/20261023-162456-sLpqiXCh.log,
257 casos gerados; validação release do código emitido com -D warnings no
ADR-0074. As suites das ferramentas somam 16 testes (6 launcher, 6 preparo/
sonda, 4 gate); foram executadas no WSL com processos controlados, separadas
do contador Cargo. README e docs/dev/alpha.md contêm comandos e limites atuais;
PLANO 4.1–4.3 concluído, sem alterar os critérios de aceite.
