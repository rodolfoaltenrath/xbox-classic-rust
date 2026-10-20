# ADR-0017: Multiplicação, divisão e diagnóstico de #DE no AOT

- **Status:** aceito
- **Data:** 2026-10-19
- **Fase:** 3-AOT

## Contexto e alternativas

MUL/IMUL e DIV/IDIV usam registradores implícitos e resultados de largura dupla.
Divisão por zero e quociente não representável geram #DE. O runtime ainda não tem
IDT/kernel para entregar essa exceção ao guest; um panic Rust ou SIGFPE do host
não seria um resultado recuperável.

As alternativas eram depender de divisão nativa com recuperação de sinal, usar
aritmética ampliada com validação antes da escrita ou limitar a divisão a casos
pré-selecionados. A segunda opção mantém o runtime seguro e portátil.

## Decisão

- IRs explícitas para produto/divisão implícitos, IMUL truncado e extensões de
  sinal. MUL/IMUL implícitos e DIV/IDIV cobrem 8/16/32 bits; IMUL de dois e três
  operandos cobre 16/32 bits, incluindo memória e imediatos com sinal.
- Produtos sem sinal usam `u64`; produtos com sinal usam `i64`. O resultado
  completo é escrito em AX, DX:AX ou EDX:EAX; IMUL truncado escreve somente o
  destino. CF/OF indicam perda de informação na largura do operando. SF/ZF/AF/PF
  indefinidos são zero. Capturar a fonte antes da escrita cobre AH/EAX/EDX aliases.
- Divisão usa `i128` para o dividendo de até 64 bits, com sinal explícito quando
  necessário. Validar divisor e faixa do quociente antes de escrever resultados;
  isso também evita overflow Rust em MIN64/-1. Resto acompanha o sinal do
  dividendo; AL/AH, AX/DX e EAX/EDX recebem quociente/resto somente no sucesso.
- Divisor zero e overflow retornam `Error::Divide { pc, reason }`, com motivo
  `DivideFault::ZeroDivisor` ou `QuotientOverflow`. EIP aponta para a instrução;
  registradores, FLAGS e contador de instruções concluídas não são comprometidos.
  O operando de memória é lido antes da aritmética: falta de memória tem precedência.
- Flags aritméticas de DIV/IDIV, indefinidas após sucesso, são zero. Bits de
  controle são preservados. CBW/CWDE/CWD/CDQ preparam operandos com sinal sem
  modificar flags. O formato de snapshot permanece o mesmo.

## Referência física e invariantes de unsafe

`xbox-cpu::oracle::muldiv` contém oito blocos de assembly fixo, atrás da feature
de teste já existente:

- Três blocos para produtos: AL/AX/EAX e CL/CX/ECX inicializados, EAX/EDX declarados
  como saídas e captura de flags com PUSHFQ/POP balanceado. Multiplicação em
  registradores não entrega #DE. IMUL truncado é comparado com os bits baixos e
  CF/OF do produto físico implícito equivalente.
- Dois blocos de pré-checagem de divisão em 64 bits: unsigned usa RDX=0; signed
  usa CQO. Divisor zero e MIN64/-1 são excluídos antes do assembly. O quociente
  físico passa por `TryFrom` para a largura do guest.
- Três blocos executam DIV/IDIV exatos de 8/16/32 bits apenas depois dessa prova
  de divisor não zero e quociente representável. EAX/EDX são saídas explícitas e
  a fonte fica em ECX. Não há ponteiros ou opcodes guest no host.

O runtime usa i128, enquanto a referência obtém o quociente com DIV/IDIV físicos
de 64 bits. A classificação de erros usa faixa e divisor, sem executar #DE nativo.
**Esses testes não validam entrega de exceção por IDT, sinais ou kernel do Xbox.**

## Validação e limites

- 133228 comparações de produtos com referência física, incluindo todos os pares
  de byte para MUL/IMUL e as formas truncadas de 16/32 bits.
- 1351268 entradas de divisão, com 350688 execuções físicas estreitas bem sucedidas
  e 1000580 diagnósticos classificados. Inclui todos os dividendos AX com dez
  divisores selecionados, vetores de borda e entradas pseudoaleatórias determinísticas.
- Testes de memória read-only, aliases, leitura inválida, partes preservadas,
  extensões de sinal, resto negativo e snapshot entre produto e divisão.

Não há promessa de temporização ou compatibilidade comercial. A normalização de
flags indefinidas não tenta reproduzir um modelo específico do Pentium III.

## Como reverter

Retirar as instruções da análise restaura a rejeição explícita. A futura entrega
de #DE ao guest pode consumir `DivideFault` sem mudar a representação de CPU.

## Referências

- [Intel SDM volume 2A — DIV, IDIV, IMUL e extensões de sinal](https://cdrdv2-public.intel.com/812383/253666-sdm-vol-2a.pdf).
- [Intel SDM volume 2B — MUL](https://www.intel.com/content/dam/www/public/us/en/documents/manuals/64-ia-32-architectures-software-developer-vol-2b-manual.pdf).
