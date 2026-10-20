# ADR-0019 — Controle indireto com descoberta conservadora de alvos

**Status:** aceito · **Data:** 2026-10-20 · **Fase:** 3-AOT

## Contexto

Callbacks, tabelas de saltos e chamadas por registrador não têm todos os destinos
codificados na própria instrução. Decodificar toda seção linearmente confundiria
dados com código. Executar um endereço guest como ponteiro host violaria o modelo
de estado e memória do projeto.

## Decisão

- Aceitar CALL/JMP near de 32 bits por registrador e memória, com os modos de
  endereço de 32 bits já suportados, incluindo SIB e ESP. CALL termina um bloco e
  inclui a continuação como raiz; JMP termina sem decodificar fallthrough.
- Descobrir fluxo direto a partir do entry point e de entradas adicionais. A API
  `analyze_with_entries(bytes, entries)` e a CLI `--entry <VA>` aceitam raízes
  explícitas, sujeitas às mesmas verificações de código executável, instruções
  completas e ausência de sobreposição. A opção pode ser repetida e aceita decimal
  e hexadecimal com `0x`/`0X`; duplicatas não alteram o resultado.
- Inferir destinos dentro de um bloco, começando com registradores desconhecidos.
  Propagar MOV de 32 bits entre registradores, imediatos e ponteiros imutáveis;
  LEA pode formar endereço com base/índice constantes. NOP/CLD/STD preservam fatos.
  Outras operações invalidam todos os fatos, deliberadamente. Não assumir ABI,
  valores entre blocos, preservação após CALL nem resultado de escrita parcial.
- Uma fonte de ponteiro precisa ter os quatro bytes no raw/virtual de uma mesma
  seção não gravável. Não inferir a partir de headers, padding ou valores iniciais
  de seções graváveis. A leitura em execução continua passando pelo bus, mesmo
  quando o destino foi inferido estaticamente.
- Repetir a descoberta até estabilizar. Novos destinos devem estar nos bytes de
  seção executável imutável; código inválido ou sobreposto é erro. Valores que não
  apontam para código não criam raízes e serão rejeitados na execução. Limites:
  65536 instruções, 65536 entradas adicionais e 256 rodadas de inferência.
- Se há CALL/JMP indireto, promover cada início de instrução decodificada a bloco.
  Isso permite saltar dinamicamente para um início conhecido no interior de uma
  rotina. Aumenta o despacho e muda a granularidade do orçamento nesse programa;
  nenhuma instrução é decodificada durante a execução. Programas sem CALL/JMP
  indireto mantêm seus blocos anteriores, incluindo a demonstração de 37 instruções.

## Transferência em execução

1. Ler o alvo pelo registrador ou bus, usando ESP anterior ao CALL. Capturar o valor
   antes de escrever o retorno cobre inclusive `CALL [ESP-4]`, em que as posições
   da fonte e da escrita coincidem.
2. Exigir que o endereço pertença ao conjunto de blocos gerados. Caso contrário,
   devolver `UnknownTarget { pc, target }` antes de mudar CPU/pilha/`retired`.
3. Verificar permissão de execução no bus. Falta de leitura do ponteiro precede
   validação do destino; falta de escrita da pilha só é testada após destino válido.
4. CALL grava a continuação na pilha guest e só então compromete ESP/EIP. JMP só
   troca EIP. A execução do próximo bloco continua sujeita ao orçamento.

RET também valida o destino antes de desempilhar. Apenas RET reconhece
`RETURN_SENTINEL` como retorno ao host; CALL/JMP não podem saltar para ela.
FLAGS são preservadas. Falhas não contam a instrução como concluída. Essa ordem
inclui a guarda AOT e não pretende modelar todas as prioridades de exceção do Xbox.

Não há chamada guest na pilha nativa, `transmute`, ponteiro de função guest ou
decoder no runtime. Snapshots continuam em `AOTC` v2, sem novos campos: chamadas,
recursão e retornos pendentes são representados por EIP/ESP e memória guest.

## Validação e limites

Testes de análise cobrem constantes, LEA, ponteiros RO/RW, fontes parciais,
invalidação em joins/calls, cadeias, raízes extras inválidas e variantes recusadas.
O Rust emitido é compilado e executado para registradores, SIB, callbacks mutáveis,
tabelas de saltos, aliases de ESP, recursão, orçamento, flags, faltas de memória,
alvos desconhecidos, RET e snapshot com chamada pendente. A CLI é exercitada com
raízes decimais/hexadecimais, duplicatas e erros antes de criar artefatos.

Não há garantia de descobrir todas as funções de um jogo. Tabelas mutáveis e
valores calculados entre blocos precisam de entradas declaradas até haver análise
mais ampla/perfis. Ainda faltam imports/kernel, TLS, código automodificável,
invalidação AOT por remapeamento e verificação de identidade imagem/módulo.
CALL/JMP far ou de 16 bits, FS/segmentos e address16 geral continuam recusados.

## Como reverter

Retirar as variantes `Call_rm32`/`Jmp_rm32` restaura a rejeição explícita. A guarda
de RET pode permanecer. Otimizações de inferência e despacho devem manter alvos
validados, orçamento limitado, erros antes do commit e snapshots determinísticos.

## Referências

- [Intel SDM volume 2A — CALL e JMP](https://cdrdv2-public.intel.com/812383/253666-sdm-vol-2a.pdf).
- [Intel SDM — conjunto de manuais de instruções](https://www.intel.com/content/www/us/en/developer/articles/technical/intel-sdm.html).
