# ADR-0056 — Evidência pública do NV2A e reprodução dos alvos

Data: 2026-10-22. Base: `9ad331e`. Estado: aceito como método de investigação;
nenhum registrador ou valor novo foi implementado nesta entrega.

## Contexto e decisão

A passagem de bastão deixou a origem dos valores do NV2A como decisão pendente.
O próximo passo não exige extrair uma BIOS: existem fontes públicas que podem
ser verificadas e fixadas. Para cada registrador, separar três evidências:

1. O que o **guest exige**: fonte nxdk fixada, desassemblagem do executável e
   telemetria produzida pelo caminho realmente executado.
2. O que uma **implementação pública modela**: arquivo, revisão, valor e
   semântica identificáveis. Isso prova a escolha daquela implementação;
   não é automaticamente medição de hardware ou estado de um console real.
3. O que foi **medido no hardware**, se houver: revisão do console, condições
   de inicialização e procedimento reproduzível. Ainda não temos essa medição.

Uma futura implementação pode declarar um perfil de compatibilidade baseado
em referências públicas, com limites e migração de snapshot explícitos. Não
deve anunciar valores de outro emulador como reset físico comprovado. Nenhum
default novo foi escolhido para o runtime aqui; P1/P12 continuam abertos.

## PLL: evidência localizada

O nxdk está fixado em `14d5ee97e73347c973f1f57b68b79ec08c9e77f2`:
[pbkit.c](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/pbkit/pbkit.c#L2173).
Em `pb_init`, M vem dos bits 0..7, N dos 8..15 e P dos 16..18 do PLL. M zero
leva a `pb_kill` e retorno -5; do contrário, a frequência calculada configura
o PTIMER. Isso descreve o consumidor e a fórmula, não fornece o estado inicial
que o kernel/firmware teria programado.

Na revisão pública do xemu **`03b988c9d76553b33ed712ba00822ffdf49f0e7d`**:

- [nv2a.c, nv2a_reset](https://github.com/xemu-project/xemu/blob/03b988c9d76553b33ed712ba00822ffdf49f0e7d/hw/xbox/nv2a/nv2a.c#L288)
  atribui `core_clock_coeff=0x00011C01` e `core_clock_freq=233333324`.
- [pramdac.c](https://github.com/xemu-project/xemu/blob/03b988c9d76553b33ed712ba00822ffdf49f0e7d/hw/xbox/nv2a/pramdac.c#L85)
  decodifica M/N/P e calcula a frequência ao escrever o coeficiente.

Com o cristal inteiro de 16666666 Hz usado pelo pbkit, M=1, N=28 e P=1 dão
233333324 Hz. O comentário de dúvida sobre MHz no reset do xemu não é usado
como evidência; a conta e as atribuições são verificáveis separadamente.
O número antes lembrado foi localizado em uma fonte, mas isso não demonstra
o valor de todos os Xbox. `NV_PFB_CFG0=0x03070103` continua sem validação nesta
investigação e não foi adotado.

## Reprodução e ferramentas

O executável local do triangle com dez raízes reproduziu a falta em VA=0x40,
EIP=0x16EA2. A telemetria registra três leituras de 32 bits em 0xFD680500 sem
modelo. Logo, o PLL inválido é uma explicação consistente com o fonte do SDK;
o zero é o placeholder do Bus, não um valor observado em hardware.

Os scripts da passagem de bastão passam a preservar essa evidência:

- `build-nxdk-sample.sh` confere revisão e limpeza dos fontes/submódulos, reaproveita
  o SDK sem rede, aplica prefix maps C/C++ e grava manifesto e hashes. Saída XBE
  permanece raw: não promete igualdade de metadados entre builds.
- `find-translation-roots.sh` exige pasta nova e preserva `attempt-NN/`, logs,
  `roots.txt` e `final-project.txt`. Nunca remove o diretório recebido. Só adiciona
  destinos efetivamente reportados e normaliza hexadecimal para evitar duplicatas.
  O analisador continua validando cada raiz. Entry point é incluído automaticamente.
- Falha de tradução/build, erro diferente de destino ausente, alvo repetido,
  timeout e limite de tentativas saem com erro. Retorno normal do guest sai com zero.
  O programa é encontrado com ou sem `.exe`, conforme o host.

Limites configuráveis por ambiente: `ROOT_SEARCH_MAX_ROUNDS` (40),
`ROOT_SEARCH_TIMEOUT_SECONDS` (120 por execução) e `ROOT_SEARCH_SLICES` (4000).
Isso automatiza raízes observadas em execução; não resolve jump tables na análise
e não fecha P5. O orçamento esgotado de um sample infinito continua sendo parada
controlada com status de erro do CLI, não retorno normal do guest.

Teste do orquestrador, sem toolchain Rust no Linux/WSL:

```sh
python3 scripts/tests/test_translation_roots.py
```

Sete testes com Cargo/guest controlados cobrem dois diagnósticos de destino,
preservação de arquivos, caminhos com espaços, status de falha/timeout, limite,
raízes repetidas, nomes com/sem `.exe` e entradas inválidas.

Gate reproduzido nesta revisão: **486 testes workspace/doctests**, incluindo o
gerador que compilou/executou **206 casos nativos**, fmt e Clippy all-targets com
`-D warnings`. Os **sete testes Python** são adicionais a essa contagem.
O build revisado do triangle rodou no WSL, gerou manifesto/hashes em
`target/handoff-build-review/` e não utilizou rede. Logs locais:
`target/handoff-workspace.log`, `target/handoff-clippy.log`,
`target/handoff-build-review.log` e `target/homebrew2/review-run.log`.

O script final também foi executado no Git Bash com Cargo Windows e as dez
raízes do triangle. Traduziu **17883 instruções/blocos**, compilou em release
e preservou `target/homebrew2/roots-review-20261022/attempt-01/` e seus logs.
Parou com **status 1**, corretamente, na mesma falta em VA=0x40/PC=0x16EA2;
não confundiu o endereço de dados com uma nova raiz nem anunciou sucesso.
`roots.txt` manteve os dez destinos conhecidos, e `final-project.txt` aponta
para a tentativa preservada. Log: `target/handoff-roots-real.log`. A compilação
inicial desse módulo em release levou vários minutos; o timeout do script limita
a execução do guest, não a compilação.

## Próxima entrega de hardware

Implementar um recorte de PRAMDAC com perfil de clock nomeado/versionado,
semântica explícita de leitura/escrita/larguras e snapshot/migrações. O reset
escolhido precisa ser descrito como perfil, com a procedência acima. PTIMER deve
derivar do Clock único da sessão; não criar outro relógio host.

Rodar o triangle de novo e registrar a próxima dependência. PFB/PBUS/PTIMER,
PRAMIN e MmClaimGpuInstanceMemory devem ser atendidos conforme aparecerem;
PFIFO/pushbuffer/PGRAPH ainda não têm implementação. Nenhuma imagem 3D foi gerada.
