# ADR-0100: serviços comuns com contrato no nxdk fixado

Data: 2026-11-04. Estado: aceito. PLANO 13.1 (inclui parte das listas de 12.3 e 12.6).

## Contexto e evidência

Um ranking das importações ainda não implementadas, somando o hello, o triangle e os dois
programas de dashboard fornecidos pelo mantenedor (só a lista de importações, via
`ordinal-report`), mostrou quatro funções que **todos** os programas usam e cujo
contrato está no
[xboxkrnl.h](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/xboxkrnl/xboxkrnl.h)
do nxdk fixado:

- `AvSetSavedDataAddress(PVOID Address)`, `VOID`; par de `AvGetSavedDataAddress`.
- `DbgPrint(PCSTR Format, ...)`, cdecl, "printf-style output ... @return A status-code".
- `HalReturnToFirmware(FIRMWARE_REENTRY Routine)`, `DECLSPEC_NORETURN`; a enumeração vai
  de `HalHaltRoutine` (0) a `HalFatalErrorRebootRoutine` (4), com `HalMaximumRoutine` (5).
  O mesmo cabeçalho documenta `HalRegisterShutdownNotification`: "the higher the priority,
  the earlier the callback function is called".
- `IoDeleteSymbolicLink(POBJECT_STRING SymbolicLinkName)`, `NTSTATUS`.

Nenhuma depende de valor de hardware desconhecido: são operações sobre estado que o
próprio kernel já mantém (o link D:, o endereço salvo, a lista de notificações).

## Decisão

**IoDeleteSymbolicLink.** Apaga `\??\D:`, o único nome que o perfil cria. Handles abertos
antes continuam válidos até o `NtClose` (o objeto sobrevive ao nome); depois de apagar,
`NtOpenSymbolicLinkObject` devolve `STATUS_OBJECT_NAME_NOT_FOUND` e o nome pode ser criado
de novo. Apagar um nome ausente devolve o mesmo status; nomes fora de `\??\D:` seguem
recusados como variante, como na criação. Passive, como o resto do namespace.

**AvSetSavedDataAddress.** Guarda o ponteiro cru (AVST v4, quatro bytes; v1–v3 migram para
zero). `AvGetSavedDataAddress` devolve o valor guardado, zero no cold boot. Nenhum acesso à
memória apontada: quem a interpreta é o guest.

**DbgPrint.** Escreve num transcrito limitado do host (64 KiB; o excesso é descartado e
contado), consultável pela API do kernel. Formatador de 32 bits: `%d %i %u %x %X %c %s %p %%`,
flags `-` e `0`, largura (até 512) e o modificador `l`. `%p` imprime oito dígitos
hexadecimais maiúsculos (x86 de 32 bits). Retorna `STATUS_SUCCESS`. Fora do perfil, como
variante recusada **sem imprimir nada**: ponto flutuante, 64 bits, `%n`, strings largas,
`%s` nulo ou fora da RAM e formato incompleto. O transcrito é diagnóstico do host e **não
entra em snapshot**: restaurar não reproduz o que foi impresso antes.

**HalReturnToFirmware.** Aceita 0..=4 (5 em diante é argumento inválido). A chamada não
retorna: o serviço só registra o pedido, a sessão **não aplica o retorno** e `conduzir`
chama as notificações de desligamento registradas — da maior prioridade para a menor,
empates na ordem do registro, cada uma como chamada guest aninhada `stdcall(registro)` — e
então devolve o novo desfecho `Desfecho::RetornoAoFirmware(rotina)`. O estado fica intacto e
salvável. Quem não conduz (`atender_kernel`) recebe `Error::FirmwareReturn`, e dentro de ISR
ou DPC a chamada é recusada (`BlockedInInterrupt`). O `main` gerado imprime a rotina e
termina. O significado de cada rotina (parar, reiniciar, reinício rápido...) não é emulado:
o desfecho informa qual foi pedida e quem hospeda decide.

## Validação

Oito testes de kernel (`common_services.rs`): apagar e recriar o link, handle aberto
sobrevivendo ao nome, nome fora do perfil; valor guardado, zero inicial e snapshot de ida e
volta; formatação de todas as conversões, preenchimento com zero depois do sinal, recusas sem
meia linha, e o limite do transcrito; pedido de firmware, rotina inválida e ordem das
notificações por prioridade. No Rust emitido, três casos: um programa x86 que guarda e lê o
endereço, imprime `n=7`, registra duas notificações (prioridades 1 e 5) e chama
`HalReturnToFirmware(1)`; o desfecho é `RetornoAoFirmware(1)`, a de prioridade 5 roda antes,
o código depois da chamada nunca executa, o transcrito é `n=7`, e um chamador que não
conduz recebe o erro. Dois testes de migração de snapshot do AVST precisaram descartar
também o campo novo.

Hello 57/69, triangle 61/75 (`ordinal-report`).

## Limites e P28

- O formatador cobre só conversões de 32 bits. Um `DbgPrint` com ponto flutuante falha com a
  mensagem exata em vez de imprimir lixo.
- A formatação de `%p` e o descarte além de 64 KiB são decisões do projeto, não medidas no
  console. O transcrito não é persistido.
- `HalReturnToFirmware` não distingue, nem emula, as cinco rotinas.
- `IoDeleteSymbolicLink` só conhece `\??\D:`.

Condição de saída: um consumidor real de `%f`/64 bits no `DbgPrint`, um host que trate
cada rotina de firmware, e links além de D: quando houver disco.

## Reversão

Os quatro ordinais saem de `IMPLEMENTADOS` e do `match`. AVST v4 seria lida e descartada se a
versão voltasse a 3; `Desfecho::RetornoAoFirmware` é aditivo.
