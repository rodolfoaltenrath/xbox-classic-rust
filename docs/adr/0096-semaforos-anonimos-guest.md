# ADR-0096: semáforos anônimos e espera pelo scheduler guest

Data: 2026-10-27. Estado: aceito. Subentrega 12.3a do PLANO.

## Contexto e evidência

O hello importa NtCreateSemaphore (193) e NtReleaseSemaphore (222).
O contrato e a ABI são documentados pelo nxdk fixado
[xboxkrnl.h](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/xboxkrnl/xboxkrnl.h):
stdcall/16 e /12, contagem LONG, máximo positivo, liberação positiva,
PreviousCount opcional; NtWaitForSingleObject decrementa a contagem ou espera.
Objetos e bloqueio devem pertencer à sessão, não a semáforos ou threads host.

## Decisão

Tabela limitada a 4096 semáforos anônimos em Passive, com handle do contador
global, contagem atual e limite. Atributos nulos ou estrutura zerada são aceitos;
nome/diretório/flags recusam. Contagem inicial admite zero; valores negativos,
máximo zero e contagem maior que máximo devolvem INVALID_PARAMETER.

Liberação positiva confere o limite antes de qualquer store, devolve contagem
anterior quando solicitada e soma unidades reais. Exceder o máximo devolve
SEMAPHORE_LIMIT_EXCEEDED, sem alterar contagem ou saída. Outputs inválidos são
preflighted sem MMIO/A/D. NtClose remove o handle e não reusa seu número.

NtWaitForSingleObject consome uma unidade se disponível. Sem sinal, o scheduler
estaciona a thread sem concluir a chamada; liberação por outra thread permite
reatender o wait e consumir a unidade. Não há sucesso de wait sem consumo.
Variantes alertáveis/com timeout continuam recusadas pelo perfil existente.
O recorte de duas threads e uma espera pendente do ADR-0072 permanece.

KRNL v16 acrescenta SEMA v1 depois de SMBC: count u32 e entradas ordenadas
handle/count/maximum (12 bytes). KRNL <=15 migra para tabela vazia, sem herdar
estado da sessão destino. O load valida geometria/limites/ordem e colisões com
mutants, eventos, arquivos/diretórios, links e thread. SEMA não possui ponteiros
host nem duplica memória guest. Referência Ob* a esses objetos ainda não existe.
O load monta candidato e valida relações antes de publicar o Dispatcher;
colisão com evento deixa o estado anterior integralmente intacto.

## Validação

Quatro testes de kernel verificam consumo/liberação/limite/fechamento, parâmetros
e saídas inválidas sem consumir handle, contagem anterior, IRQL, replay,
migração v15 e corrupção/colisão com evento vivo. No Rust emitido, uma filha
dorme, o pai bloqueia no semáforo zero, a filha libera e retorna, o pai consome
e fecha. Snapshot no bloqueio reproduz sessão integral. Outro caso garante que
migração v15 não herda semáforos de destino e regrava estado canônico.

Fixtures legadas removem SEMA no intervalo correto, conservando o scheduler e
demais seções posteriores. Testes de corrupção SMBC/SYML passam a localizar o
campo pelo layout da seção, não pelo fim do arquivo, preservando sua finalidade.

Gate final: GATE OK 667, target/gate/20261027-121229-3BnVpOCJ.log. Rust emitido:
279 casos, target/recomp-tests/8404-1793114089218984500/native-test-results.txt.
Relatório real: hello 48/69, triangle 52/75. Não se repetiram sondas visuais.

## Consequências e reversão

Hello 48/69 variantes disponíveis; 21 pendentes. Isso não representa chamadas
adicionais no laço original. O restante de 12.3 (namespace/referências) não está
entregue. P25 conserva as limitações, e nenhuma tabela vazia é promovida em load.
Reverter exige manter leitor de KRNL v16/SEMA v1 ou migração explícita: não
reduzir a versão reinterpretando snapshots que já contêm semáforos.
