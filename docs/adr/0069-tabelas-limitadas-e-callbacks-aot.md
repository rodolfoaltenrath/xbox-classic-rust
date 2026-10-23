# ADR-0069 — Descoberta AOT de tabelas limitadas e callbacks

Data: 2026-10-23. Estado: aceito. Fase 3-AOT; PLANO 3.1; encerra P5 para o alvo.

## Contexto e evidência

O triangle precisava de 14 `--entry`, incluindo o próprio entry point. O
[ADR-0068](0068-raster-software-vertex-program.md) já produz quadro real, mas a
análise local do ADR-0019 perde índices salvos na pilha, tabelas de inicialização
e callbacks passados por memória. Buscar uma raiz a cada UnknownTarget não
satisfaz a condição de saída de P5.

Fonte executável: triangle nxdk do pin
`14d5ee97e73347c973f1f57b68b79ec08c9e77f2`, XBE SHA-256
`f6288954b7f3bc38a367a5833012fc6ba8929d0d358ee1a68f0800ce89aeaf93`.
Desmontagem do PE correspondente com llvm-objdump, mantida localmente em
`target/pfifo-evidence/triangle-full-disassembly.txt`. A semântica inteira é a
IR x86 já implementada; nenhuma descrição de hardware foi acrescentada.

| Consumidor observado | Fonte de destinos | Limite/fluxo |
|---|---|---|
| jmp eax em 0x1E9B6 | DWORDs em 0x2F32C | índice salvo em [esp+0x1C], SUB 55/JA, recarga do mesmo índice; não usar o resultado subtraído como índice |
| switch de clock em 0x2A31E | DWORDs em 0x2F458 | índice mascarado por 0x2F e salvo em [esp+4] |
| inicializadores em 0x22923 | [0x316A0,0x316AC) | cursor local, comparação unsigned com fim, passo quatro, NULL permitido |
| inicializadores em 0x22984 | [0x31694,0x3169C) | guarda unsigned propagada por flag byte zero/TEST/JNE; cursor local e passo quatro |
| inicializadores em 0x229E7 | [0x3168C,0x31690) | guarda unsigned direta e mesmo percurso |
| rotinas 0x22C10/0x22AC0/0x22790/0x1D7B0 e ISR/DPC | endereços explícitos em LEA/operandos, copiados para memória | candidatos de callbacks consumidos por controle indireto; não afirmar que RAM gravável contém sempre esse valor |

Os endereços acima documentam o alvo; **nenhum** aparece como exceção/lista de
raízes no analisador. A entrada 0x22700 já vem do header XBE.

## Decisão

Manter o passe de constantes locais e acrescentar três análises independentes:

1. **Tabelas indexadas com domínio limitado.** Reconstruir um caminho de até 64
   blocos/512 instruções, parando em joins e barreiras de call. Símbolos preservam
   identidade em cópias de registradores/slots da pilha. CMP e SUB guardam o
   operando anterior; Jcc unsigned restringe esse símbolo no caminho escolhido.
   AND/zero-extend também fornecem intervalos. Operações não descritas invalidam
   o estado; stores parciais/aliases desconhecidos invalidam slots. Só enumerar
   até 4096 escolhas cujas leituras inteiras estejam no raw/virtual de seção
   readonly. Índices sem prova, wrap de endereço ou tabela fora da faixa não
   autorizam descoberta. Conjuntos de valores iguais de origens independentes
   não recebem restrições por semelhança.
2. **Tabelas percorridas por cursor.** Reconhecer loops pequenos de entrada única
   com inicialização constante, fim unsigned explícito, único incremento de
   quatro no tail e consumo indireto pelo cursor. Conferir que stores explícitos
   não alteram/escapam o slot e que a guarda sai da região; a forma booleana do
   CRT exige flag zero no caminho cursor>=fim e TEST/JNE que não entra no corpo.
   Toda a extensão é readonly e cada DWORD é NULL ou código imutável válido.
   As entradas são candidatos da extensão identificada, não uma constante
   atribuída à RAM do cursor. Efeitos de callbacks continuam dinâmicos; uma
   alteração fora desse conjunto permanece sujeita ao UnknownTarget do runtime.
3. **Fluxo possível de endereços escapados.** Coletar endereços de código imutável
   presentes nas operações alcançadas, copiá-los por MOV/LEA/stores/pushes e
   propagar candidatos até chamadas indiretas. Stores absolutos distintos têm
   buckets distintos; endereços dinâmicos usam um conjunto de possíveis aliases.
   Endereço readonly absoluto é consultado como singleton, sem alias com stores.
   Este domínio é uma sobreaproximação de possíveis alvos: pode compilar uma
   rotina que não será chamada, e não demonstra alias exato nem cobertura geral.
   Não elimina caminhos nem alimenta o domínio de constantes/guardas. Calls
   invalidam registradores; o conjunto de escapes representa fontes possíveis,
   nunca um readback emulado. Limite de 4096 candidatos; sem varrer dados cegamente.

A descoberta continua iterativa (até 256 rodadas/65536 instruções). Só acrescenta
blocos; o Rust emitido **lê a RAM real** e valida cada alvo antes da transferência.
Nenhuma carga/índice é substituído pelo valor que ajudou a encontrar uma rotina.
Alvos sem bloco, no meio de instrução, fora de código imutável ou ISA pendente
continuam recusados. `--entry` permanece disponível para outros casos; não é
necessário para o triangle desta revisão.

## Alternativas e limites

Lista fixa dos 14 endereços, varredura de DWORDs buscando valores parecidos com
código e tratar o conteúdo inicial de RAM gravável como constante foram
rejeitados. O passe de escapes explicita possíveis aliases e mantém a verificação
dinâmica, separando descoberta de código e previsão de valor.

O domínio não pretende resolver todos os indiretos x86, self-modifying code,
construção arbitrária de ponteiros ou qualquer formato de switch. Recusa/UnknownTarget
continuam sendo a resposta para falta de evidência. Não há formato de snapshot
novo: CPU, kernel e GPU mantêm seus contratos; mudou o conjunto de blocos emitidos.

## Validação

Dez testes em `crates/xbox-recomp/tests/jump_tables.rs`: CMP/SUB/JA e slots,
recusa sem guarda/readonly, escrita parcial/sobrescrita, flags e joins, limites,
loops com guarda direta/booleana, passos inválidos e aliases absolutos distintos.
Os seis testes de indiretos anteriores permanecem, inclusive a não preservação
de constantes por calls/joins/escritas parciais.

Três casos de Rust gerado: ambos os cases e default de tabela readonly, snapshot
no meio da guarda, callback escapado com RAM modificada entre descoberta e
execução. O último também confirma que alvo sem bloco continua sendo recusado
sem alterar a CPU.

Tradução sem `--entry`: **19707 instruções/blocos**,
`target/plan-31-translate-final.log`, projeto
`target/homebrew2/auto-tables-review-20261023`. Contém os 14 endereços antigos.
Gate: **566 testes**, target/gate/20261023-023145.log; **233 casos nativos**
debug/release (target/plan-31-native-release.log). Rust/Cargo 1.99.0.

Sonda oficial `bash scripts/sonda.sh triangle 320`, agora apontando para o projeto
sem raízes: target/sonda/triangle-20261023-023144.log. Mesmos números da tradução
manual: 282 chamadas/zero recusadas, 241241306 retired/305861656 ciclos (417 ms),
EIP=0x16C96, 165 MMIO sem modelo; 305922 pixels/479 linhas, regiões R/G/B
4977/4998/4977, dez draws/30 triângulos. Nenhum UnknownTarget durante a sonda.

Probe target/plan-31-real-probe.log: scanout 0x210000, quadro da RAM e snapshot
integral com mais 20 fatias idênticos após restauração. PPM do projeto automático
em target/pfifo-evidence/triangle-auto-software.ppm, byte a byte igual à captura
manual da tarefa 2.3. P5 deixa de ser requisito manual para este alvo.
