# ADR-0018 — Strings REP com continuação e orçamento limitado

**Status:** aceito · **Data:** 2026-10-20 · **Fase:** 3-AOT

## Contexto

Depois da ALU inteira, o roadmap prevê loops por contador e cópia, preenchimento
e comparação de memória. Emitir um laço Rust sem limite para REP bloquearia o host
com ECX muito grande e impediria snapshots no meio de uma instrução longa.

## Decisão

- Traduzir JCXZ/JECXZ, LOOP/LOOPE/LOOPNE com contador CX ou ECX conforme o tamanho
  de endereço. LOOP decrementa antes do teste, com wrap, sem modificar flags;
  JCXZ/JECXZ só testa. Alvos continuam sendo branches diretas de 32 bits.
- Traduzir MOVS/STOS/LODS/CMPS/SCAS de byte, word e dword; CLD/STD controlam DF.
  Endereços de 16/32 bits são explícitos na IR, separados da largura do elemento.
  Address16 usa SI/DI/CX e preserva as metades altas dos registradores.
- REP MOVS/STOS/LODS e REPE/REPNE CMPS/SCAS têm bloco próprio, separado de setup e
  continuação. Cada passagem executa **no máximo um elemento** e consome uma unidade
  do orçamento de blocos. Sem laço oculto, decodificação ou dependência do relógio.
  ECX/CX zero termina sem acessar memória nem avaliar ZF. Nas comparações, ZF é
  testado depois de cada elemento, inclusive o primeiro.
- EIP permanece na instrução enquanto há repetição. Índices e contador são
  comprometidos após os acessos bem sucedidos do elemento. `retired` aumenta uma
  única vez, quando a instrução inteira termina. O orçamento não representa ciclos.
- Sobreposição de MOVS segue a ordem do guest e DF; não usar `memcpy`/`memmove`
  para substituir a sequência. LODS preserva as partes de EAX fora do destino.
- Uma falta mantém elementos anteriores, índices e contador apontando para o
  elemento pendente. REPE/REPNE CMPS/SCAS restauram as flags anteriores ao início
  da instrução conforme o SDM, mesmo após pausas por orçamento.
- `State::repeat` guarda PC e flags originais apenas para a comparação repetida.
  É metadado serializável sem ponteiros. Snapshot `AOTC` passa a v2; v1 é lido com
  `repeat=None`. Na conclusão o metadado é removido. CPU **e bus** devem ser salvos.
- O bus valida toda a faixa de escrita antes de alterar RAM/dispositivos. Isso
  corrige stores que atravessavam páginas e deixavam bytes modificados antes de
  uma falta na segunda página. Bits de acesso/sujo e telemetria não são transações.

## Validação

O Rust gerado é compilado e executado. Há 72 variantes de strings (operação,
largura, endereço e prefixo) e oito de branches por contador. Os testes cobrem
zero, ECX enorme, wrap de CX/SI/DI, DF, sobreposição, divisão de blocos, faltas,
stores atravessando página, snapshot parcial e retomada após remapear memória.

3456 casos com endereço de 32 bits são comparados com strings fixas na CPU física
x86_64: registradores, índices, contador, flags definidas e destino completo.
O oráculo valida os limites dos slices host antes de usar assembly; RSI/RDI/RCX
e EAX são operandos declarados. DF é limpo dentro do mesmo assembly antes de
retornar ao Rust. Não executa ponteiros, bytes ou faltas do guest no host.

Address16, faults e snapshots são verificados separadamente, pois o modo longo
do host não oferece strings com endereço de 16 bits. A referência física não
comprova segmentação, entrega de exceções, temporização ou compatibilidade comercial.

## Limites e reversão

Ainda são recusados LOCK, overrides de segmento, REP em instruções não suportadas,
REPNE fora de CMPS/SCAS, INS/OUTS e branches com destino de 16 bits. Segmentos
continuam planos; TLS/FS e entrega de faltas ao kernel pertencem à Fase 4.

Uma passagem por elemento favorece correção e pausa previsível. Otimizar por lotes
exigirá medição e manter exatamente os mesmos pontos de falta e snapshot.
Retomada após erro em outras instruções no meio de um bloco continua sem suporte.
Remover as variantes da análise restaura a rejeição explícita dessas instruções.

## Referências

- [Intel SDM, volume 2B: REP, MOVS, STOS e SCAS](https://www.intel.com/content/dam/www/public/us/en/documents/manuals/64-ia-32-architectures-software-developer-vol-2b-manual.pdf).
- [Intel SDM: referências de CMPS, LODS, LOOP e JCXZ/JECXZ](https://www.intel.com/content/www/us/en/developer/articles/technical/intel-sdm.html).
