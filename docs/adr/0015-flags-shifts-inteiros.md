# ADR-0015: Flags e validação dos shifts inteiros AOT

- **Status:** aceito
- **Data:** 2026-10-19
- **Fase:** 3-AOT

## Contexto

SHL/SHR/SAR precisam de contagens mascaradas a cinco bits, larguras de 8/16/32 bits
e distinção entre flags definidas, preservadas e indefinidas. Copiar as flags de
uma CPU moderna daria uma promessa de compatibilidade que o oráculo não comprova.

## Alternativas consideradas

1. Preservar flags indefinidas: determinístico, mas cria dependências do estado
   anterior que não são garantidas pela ISA.
2. Copiar o host: depende do modelo de CPU e não especifica o comportamento do Xbox.
3. Normalizar flags indefinidas para zero: política explícita, como AF das operações
   lógicas existentes; permite comparar separadamente o que a ISA define.

## Decisão

Usar a terceira alternativa. A contagem efetiva é `count & 31`. Com zero, preservar
valor e todas as flags; acesso à memória continua read-modify-write. Com contagem
não zero, calcular SF/ZF/PF e normalizar AF para zero. OF só é definido com contagem
um; CF de SHL/SHR só é comparável quando a contagem é menor que a largura. Nas
demais situações indefinidas, usar zero. SAR mantém CF definido também quando a
contagem alcança ou ultrapassa a largura, propagando o sinal.

O emissor produz as expressões Rust do resultado, captura CL antes da escrita e
atualiza FLAGS depois de uma escrita bem sucedida. A IR recebe `Shift` separado das
operações binárias: CL tem largura de byte mesmo para um destino de word/dword.

O oráculo de teste em `xbox-cpu` executa shifts fixos por CL. Seus três novos blocos
`unsafe` preservam os bits de controle de RFLAGS: capturam o valor do host, trocam
somente CF/PF/AF/ZF/SF/OF por bits de entrada mascarados e restauram esse valor antes
do shift. PUSHFQ/POP e PUSH/POPFQ são balanceados; o scratch é saída antecipada e CL
é entrada explícita. Não há ponteiro, opcode ou endereço guest executado. DF, TF,
IF, IOPL e AC não recebem dados do chamador. A feature `isa-oracle` continua sendo
somente de teste; o runtime AOT permanece Rust seguro.

## Consequências

- A matriz compara resultados e flags definidas contra a CPU física em 52470 casos
  de CL, imediato e contagem um implícita. Mais 393216 casos esgotam valores e
  contagens de byte com dois estados iniciais de flags.
- Os testes verificam separadamente a normalização, a preservação em contagem zero,
  AH/CL/CX/ECX, largura de memória, erros de acesso e snapshot entre shift e ADC.
- Não há mudança de formato de snapshot ou nova dependência.
- Não representa comportamento exato das flags indefinidas do Pentium III do Xbox,
  nem valida segmentação, MMIO, exceções reais ou temporização. As regras próprias
  de rotações e shifts duplos estão no [ADR-0016](0016-rotacoes-shifts-duplos.md).

## Como reverter

Retirar a aceitação dos shifts na análise restaura a rejeição explícita. Alterar a
política de flags exige atualizar `shift_flags`, os testes e este ADR; snapshots
continuam guardando EFLAGS sem formato novo.

## Referência

[Intel SDM, volume 2B, SAL/SAR/SHL/SHR, páginas 4-582 a 4-586](https://www.intel.com/content/dam/www/public/us/en/documents/manuals/64-ia-32-architectures-software-developer-vol-2b-manual.pdf#page=583).
