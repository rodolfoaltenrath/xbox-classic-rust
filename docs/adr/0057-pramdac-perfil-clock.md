# ADR-0057 — Perfil de clock PRAMDAC e NVPLL

Data: 2026-10-22. Base de código: `9ad331e`, com a investigação local do ADR-0056.
Estado: aceito para o recorte NVPLL de 32 bits; não representa o PRAMDAC completo.

## Decisão e procedência

O triangle/pbkit chegou à leitura de NV_PRAMDAC_NVPLL_COEFF, recebia zero do Bus
e entrava no caminho de rejeição que prevê -5 após pb_kill. A investigação anterior localizou o coeficiente
inicial em uma implementação pública. Esta entrega adota **PublicReferenceV1**,
um perfil de compatibilidade, sem alegar que seja um reset físico medido.

Fontes fixadas:

- [xemu nv2a_reset](https://github.com/xemu-project/xemu/blob/03b988c9d76553b33ed712ba00822ffdf49f0e7d/hw/xbox/nv2a/nv2a.c#L288):
  coeficiente inicial `0x00011C01` e frequência nominal 233333324 Hz.
- [xemu pramdac.c](https://github.com/xemu-project/xemu/blob/03b988c9d76553b33ed712ba00822ffdf49f0e7d/hw/xbox/nv2a/pramdac.c#L85):
  registro programável e cálculo da frequência pelos campos M/N/P.
- [nxdk pbkit.c](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/pbkit/pbkit.c#L2173):
  consumidor real, fórmula e rejeição de M zero, usando cristal de 16666666 Hz.

Não se copiou o dispositivo de outro emulador. O recorte Rust armazena somente
perfil e coeficiente; frequência é derivada, sem outra cópia autoritativa.
PFB, PBUS, MPLL/VPLL, PLL_TEST_COUNTER e PTIMER não receberam valores nesta entrega.

## Contrato do Bus

Bus novo inicia com PublicReferenceV1. Somente NVPLL_COEFF, offset 0x00680500
na janela NV2A (PA=0xFD680500), ganha leitura/escrita de **32 bits alinhados**.
VA continua passando pela MMU; aliases e acesso físico observam o mesmo estado.
O valor bruto de 32 bits tem readback integral. Para calcular frequência:

```text
M = valor & 0xFF
N = (valor >> 8) & 0xFF
P = (valor >> 16) & 7
Hz = ((16666666 * N) / (1 << P)) / M
```

M zero produz frequência nominal zero, sem divisão no host. Bits fora de M/N/P
são preservados no readback e não têm outro efeito neste perfil. Isso não modela
lock, estabilidade, efeitos elétricos ou temporização de reprogramação do PLL.
Registrar zero é distinto de não possuir modelo.

Acesso de outra largura, desalinhado ou que apenas se sobreponha ao registro
recusa com falta NaoImplementado identificável, antes de programá-lo. O preflight
de escrita do Bus valida também esse caso em todas as páginas: RAM anterior
ao acesso inválido não é modificada. Outros offsets continuam na telemetria
com o comportamento declarado anterior. Os quatro registros de IRQ do ADR-0055
mantêm seu modelo e snapshot.

A leitura de um registro modelado não aparece como MMIO sem modelo. Consultar
perfil/frequência não avança Clock/retired. Nenhum relógio host ou oscilador novo
foi introduzido; PTIMER futuro precisa derivar do Clock único da sessão. Não há
unsafe novo, thread de GPU ou alteração da posse exclusiva/Send/!Sync da arena.

## Snapshot e migração

**BUS0 v3** acrescenta **NPLL v1** depois dos três campos de IRQ do v2:
tag/versão, u32 perfil, u32 coeficiente. Perfil 1 é PublicReferenceV1; perfil 0
é LegacyUnmodeled e exige coeficiente zero. Outros IDs/versões/truncamentos recusam.

BUS0 v1/v2 migram para **LegacyUnmodeled**, independentemente do destino:
leituras de PLL continuam devolvendo zero com telemetria, escritas são registradas
e descartadas. Não se altera silenciosamente o caminho de execução de um snapshot
antigo ao fornecer um PLL válido onde ele antes não existia. Salvar o legado no
formato novo preserva esse perfil em futuros replays. Para adotar o perfil novo,
iniciar uma sessão nova. BUS0 v1 ainda zera IRQ; v2 preserva os registradores de IRQ.

NPLL carrega metadados antes de publicá-los. O contrato global de carga do Bus
permanece: erro pode ocorrer depois da RAM/MMU carregadas; descartar a sessão em
caso de erro. AOTS/KRNL/AOTC e as outras seções não mudaram de versão.

## Verificação e limites

Seis testes do Bus cobrem reset/frequência, aliases/acesso físico, valores extremos
e M zero, larguras/sobreposição, escrita entre RAM/MMIO sem efeito parcial, snapshot
e migração v1/v2. Três casos no Rust emitido verificam o registro acessado pelo
guest, Clock/flags, snapshot integral/replay, legado e erro com reparo da CPU.

O registro de serviços do kernel permanece 37/69 no hello: esta entrega acrescenta
um registro de dispositivo, não um ordinal. Ainda sem PFIFO/pushbuffer/PGRAPH,
execução de comandos 3D ou triângulo apresentado. P1/P12 continuam abertos; o
recorte não promove os placeholders dos demais registradores a hardware funcional.

Resultado: **492 testes workspace/doctests**, **209 casos do Rust emitido em
debug e release** (193 fora de x86_64), fmt e Clippy all-targets com `-D warnings`.
O projeto emitido em release também foi compilado com `-D warnings`. Logs locais:
`target/pramdac-tests.log`, `target/pramdac-workspace.log`,
`target/pramdac-clippy.log` e `target/pramdac-native-release.log`.

## Triangle real e correção do diagnóstico anterior

As mesmas dez raízes/17883 blocos foram usados. Dois testes locais ignorados em
`target/homebrew2/roots-review-20261022/attempt-01/tests/pramdac_probe.rs`
comparam uma sessão nova com outra migrada de BUS0 v2, sem modificar o guest:

| Perfil | Leituras NVPLL | Ramo que chamou pb_kill |
|---|---|---|
| PublicReferenceV1 | 0x00011C01, sem telemetria de acesso desconhecido | caller salvo 0x18283, rejeição de PFB_CFG0 |
| LegacyUnmodeled | zero com telemetria, sem programação PTIMER | caller salvo 0x18212, rejeição do PLL |

No perfil novo, o guest calculou parâmetros e escreveu PTIMER_NUMERATOR=0xFFF4
em 0xFD009200, DENOMINATOR=0x2247 em 0xFD009210 e ALARM=0xFFFFFFFF em 0xFD009420.
Essas escritas **ainda são placeholders**, só registradas. Depois leu PFB_CFG0
em 0xFD100200; os dois bits baixos vieram zero e a comparação com 3 falhou
(0x18271..0x1827E). A próxima entrega precisa modelar PFB com procedência explícita.
Não foram adotados valores de CFG0 apenas para passar essa comparação.

**A falta final continua em VA=0x40, PC=0x16EA2, mas isso não é o primeiro
bloqueio nem prova de que pb_init retornou.** A cadeia EBP do guest confirmou
pb_start (retorno salvo 0x173F4) chamado por pb_kill, este chamado pelo ramo de
rejeição de pb_init (0x18283 no novo perfil, 0x18212 no legado). pb_start escreve
em pb_DmaUserAddr+0x40 quando esse ponteiro ainda é nulo. A limpeza falha antes
de pb_init alcançar seu retorno previsto -6/-5.

Isso corrige a inferência anterior de que o sample ignorava um retorno -5 já
observado: o fonte prevê esse retorno, mas o caminho executado não chegou a ele.
No perfil novo foram **24701938 retired/ciclos e 53 chamadas atendidas**; no
legado, **24701861 e 53**. Snapshot integral/replay do perfil novo foi conferido.
Log: `target/pramdac-triangle.log`. Testes e execução foram em debug para evitar
o custo da primeira otimização do módulo grande; os 209 casos pequenos também
foram validados em release. Não há triângulo desenhado.
