# ADR-0016: Rotações e shifts duplos na recompilação AOT

- **Status:** aceito
- **Data:** 2026-10-19
- **Fase:** 3-AOT

## Contexto e alternativas

ROL/ROR/RCL/RCR preservam flags que shifts simples modificam. RCL/RCR precisam
incluir CF em um anel de até 33 bits. SHLD/SHRD possuem três operandos e, para word,
contagens efetivas 17..31 deixam o resultado inteiro indefinido na ISA.

As alternativas eram reproduzir o host, inventar um resultado determinístico ou
recusar a execução indefinida. O host moderno não especifica o comportamento do
Pentium III do Xbox; normalizar somente flags não resolve um destino indefinido.

## Decisão

- Adicionar IRs próprias para `Rotate` e `DoubleShift`. Capturar contagem, fonte e
  destino antes de comprometer o resultado, cobrindo alias com CL/CX/ECX.
- Rotações usam um anel em `u64`: 8/16/32 bits para ROL/ROR e 9/17/33 para RCL/RCR.
  Primeiro mascarar a contagem por 31; depois reduzir pelo tamanho do anel para
  calcular o valor. CF participa apenas do anel das rotações com carry.
- Usar a contagem mascarada para FLAGS. Zero preserva todas as flags. ROL/ROR com
  uma volta completa de byte/word ainda atualizam CF; RCL/RCR com volta completa
  preservam o valor e CF. SF/ZF/AF/PF sempre são preservados. OF é definido somente
  com contagem mascarada um; nos demais casos não zero, normalizar OF para zero.
- SHLD/SHRD operam sobre word/dword. Os cálculos em `u64` evitam shifts Rust por 32
  em um `u32`, inclusive quando zero preserva o valor e quando 16 transfere a fonte
  inteira para o destino word. SF/ZF/PF/CF seguem o resultado; AF indefinido e OF
  indefinido com contagem maior que um são zero, conforme a política do ADR-0015.
- Se a contagem mascarada exceder a largura em SHLD/SHRD, retornar
  `Error::UndefinedDoubleShift { pc, count, bits }` antes de acessar memória ou
  escrever estado. Isso é uma limitação explícita do AOT, não uma exceção x86 nem
  uma previsão do comportamento de um Xbox. A sessão deve ser descartada como nos
  outros erros de execução já documentados.
- Operandos de memória continuam read-modify-write, inclusive com contagem zero;
  FLAGS só muda depois de uma escrita bem sucedida.

## Validação e invariantes

O oráculo de rotações reutiliza os três blocos de assembly de shifts do ADR-0015,
agora com mais quatro mnemônicos fixos. O novo módulo `xbox-cpu::oracle::double`
contém dois blocos `unsafe`, para word/dword: somente registradores, pilha
balanceada e controle de entrada limitado a CF/PF/AF/ZF/SF/OF. RFLAGS do host é
capturado antes de substituir esses bits; DF/TF/IF/IOPL/AC não recebem dados de
entrada. Scratch é saída antecipada, fonte é entrada e CL tem registrador explícito.
Nenhum ponteiro guest, binário fornecido pelo usuário ou opcode dinâmico é executado.

- 70488 comparações físicas de rotações em 8/16/32 bits por CL/imediato/um implícito.
- 524288 comparações esgotam os valores e contagens de byte com dois carries.
- 196988 comparações de SHLD/SHRD cobrem somente resultados definidos pela ISA.
- Testes independentes de memória, aliases, voltas completas, erros, diagnóstico
  das contagens indefinidas e snapshot entre RCL, RCR e SHLD.

## Consequências e reversão

O runtime continua Rust seguro e o formato de snapshot não muda. Resultados
indefinidos exigirão evidência específica de hardware se um título depender deles.
Retirar essas instruções da análise restaura a rejeição explícita. Uma política
futura para resultados indefinidos deve substituir o erro e atualizar este ADR.

## Referência

[Intel SDM, volume 2B: RCL/RCR/ROL/ROR e SHLD/SHRD](https://www.intel.com/content/dam/www/public/us/en/documents/manuals/64-ia-32-architectures-software-developer-vol-2b-manual.pdf).
