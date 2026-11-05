# ADR-0101: contador de performance e conversões de calendário

Data: 2026-11-04. Estado: aceito. PLANO 13.2.

## Contexto e evidência

Entre as importações mais comuns ainda ausentes (hello, triangle e os dois dashboards, só pela
lista de importações), quatro dependem apenas de estado que o projeto já tem:

- `KeQueryPerformanceCounter` (126) e `KeQueryPerformanceFrequency` (127): `ULONGLONG f(void)`
  no [xboxkrnl.h](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/xboxkrnl/xboxkrnl.h)
  fixado do nxdk. A frequência de 3,375 MHz (relógio ACPI) é a que o Cxbx-Reloaded fixado usa
  (`XBOX_ACPI_FREQUENCY = 3375000`) e a que `xbox_core::time::PERF_COUNTER_HZ` já tinha.
- `RtlTimeToTimeFields` (305) e `RtlTimeFieldsToTime` (304): o cabeçalho documenta os campos
  de `TIME_FIELDS`; o tempo é o `LARGE_INTEGER` de 100 ns desde 1601-01-01 UTC, definição
  pública do NT.

## Decisão

**Contador e frequência.** Retornos de 64 bits seguem a convenção x86 de 32 bits: palavra
baixa em `EAX`, alta em `EDX`. O despachante guarda a palavra alta (`return_high`) e a sessão a
aplica ao `EDX` junto com o retorno (`take_return_high`). O contador lê o **mesmo Clock** do
RDTSC e do `KeTickCount`, então é monotônico e determinístico. Sem o Clock da sessão o
serviço é recusado (`ContextoTemporal`), como os demais serviços de tempo.

**Calendário.** Gregoriano proléptico. `RtlTimeFieldsToTime` valida ano >= 1601, mês, dia do
mês (bissextos de 4/100/400), hora, minuto, segundo e milissegundo; fora de faixa devolve
`FALSE` sem escrever a saída; o dia da semana de entrada é ignorado. `RtlTimeToTimeFields`
preenche os oito campos (0 = domingo; 1601-01-01 é segunda-feira). `TIME_FIELDS` ocupa 16 bytes
(oito `SHORT`). Tempo negativo, ou com ano que não cabe em `SHORT`, e ponteiros fora da RAM
são recusados como variante sem escrever nada.

## Validação

Quatro testes de unidade (datas conhecidas — 1601, 1970, 2000 —, bissextos e limites, ida e
volta diária por 700 anos com o dia da semana avançando de um em um, tempo negativo); seis
testes de kernel (`tests/time_services.rs`: EDX:EAX, contador acompanhando o Clock, recusa sem
Clock, ABI de `TIME_FIELDS`, campos inválidos sem escrita, ponteiros e tempo negativo); um caso
no Rust emitido (`kernel_time_services`) em que EDX chega ao guest e o calendário faz a ida e
volta pela ABI. A primeira versão de `days_before` contava um dia a mais por século e foi
pega pelos testes de data conhecida.

## Limites e P29

- A frequência vem do Cxbx-Reloaded fixado, não de medida no console.
- O calendário é o de definição pública do NT, sem fuso nem segundos intercalares.
- Tempo negativo e anos acima de 32767 são recusados em vez de ter comportamento inventado.

Condição de saída: medida da frequência em hardware real ou comportamento documentado do
NT para tempo fora do domínio.

## Reversão

Os quatro ordinais saem de `IMPLEMENTADOS` e do `match`; `return_high` e a aplicação do `EDX`
são aditivos e podem ficar.

## Adendo (PLANO 13.3): KeQueryInterruptTime (125)

`ULONGLONG f(void)` no `xboxkrnl.h` fixado. Devolve o tempo desde o boot em intervalos de
100 ns, `clock.nanos() / 100` (arredondado para baixo), em EDX:EAX, a partir do mesmo Clock
único. É a definição pública do NT de "interrupt time"; o valor exato no console não foi
medido (P29). Sem o Clock da sessão é recusado. Teste: `tests/time_services.rs`
(0, 1 s e 3 s guest).
