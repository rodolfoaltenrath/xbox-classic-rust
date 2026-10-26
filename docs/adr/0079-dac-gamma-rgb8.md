# ADR-0079 — DAC gamma RGB8 com perfil explícito

- Status: aceito.
- Data: 2026-10-26.
- Fase: PLANO 6.1 / GPU, P2/P21.

## Contexto e evidência

Hello e triangle já escrevem gamma: suas sondas anteriores registram os
endereços FD6813C8/FD6813C9 como os dois acessos sem modelo do hello.
No nxdk fixado em `14d5ee97e73347c973f1f57b68b79ec08c9e77f2`,
[hal/video.c](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/hal/video.c)
e [pbkit_gamma.c](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/pbkit/pbkit_gamma.c)
escrevem índice zero e 768 bytes R/G/B. O HAL mantém cache das 256 entradas,
inclusive para atualização parcial, e reenvia a tabela inteira. Seu comentário
registra instabilidade ao mudar o índice no hardware; não prova escrita parcial
arbitrária como um recurso confiável. O caminho normal usado é a tabela inteira.

Na referência xemu `03b988c9d76553b33ed712ba00822ffdf49f0e7d`,
[prmdio.c](https://github.com/xemu-project/xemu/blob/03b988c9d76553b33ed712ba00822ffdf49f0e7d/hw/xbox/nv2a/prmdio.c)
posiciona cursor por índice × 3, escreve cada componente imediatamente e usa
paleta circular de 768 bytes. Há TODO explícito de confirmação do wrap.
[nv2a.c](https://github.com/xemu-project/xemu/blob/03b988c9d76553b33ed712ba00822ffdf49f0e7d/hw/xbox/nv2a/nv2a.c)
inicializa identidade e serializa cursor/paleta. Isso justifica um perfil público;
não é captura de reset nem validação física/analógica do Xbox.
Não foi usado o DAC VGA genérico, cujo cache de três componentes tem outra
semântica de publicação. A implementação Rust é própria, baseada no contrato.

O novo alvo gamma-fade foi compilado sem rede com build-nxdk-sample.sh,
SDK/submódulos fixados. AOT inicial: 13229 blocos, nenhuma raiz manual.
Artefatos ignorados em target/gamma-phase6; hashes desta compilação:

- XBE: `9e7032f58279b2d7291313ddb45a9b845c947252bdfa0ae3260795997beddeb8`.
- PE: `7114d78ff8ea8d210dd51f2595a850f94c7fab77bcf6a0b0bc00c4427cb8d8b1`.

## Alternativas e decisão

Manter escritas descartadas impede fades. Copiar reset físico desconhecido ou
usar DAC VGA com latch não é justificado pelo caminho NV2A observado.
Escolhido GammaProfile::PublicReferenceV1: identidade inicial; índice de 8 bits;
três canais RGB8; cursor circular limitado a 768; publicação imediata por canal.
Reset de índice conserva a paleta já escrita. Ler índice é permitido; leitura
de dados da paleta, formatos/canais extras e outras larguras recusam. Nenhum
modo de leitura foi inventado. Índice/data exigem bytes; acesso sobreposto maior
é erro antes de qualquer store, inclusive numa transação que começa em RAM.

Não implementa I/O de porta VGA nem filtros, LCD, overscan, sinal analógico ou
latência do DAC. Não avança Clock/IRQ nem altera RAM. Wrap, programação parcial,
reset de firmware e latência permanecem recorte declarado em P21.

## Snapshots e migração

BUS0 v10 acrescenta GAMM v1 depois de PGRF. Guarda perfil, cursor de componente
e 768 bytes. Snapshot entre R/G/B mantém o canal seguinte. Cursor fora da faixa,
perfil/versão inválidos, truncamento e conteúdo programado no perfil legado
recusam. BUS0 v1–v9 migra para LegacyUnmodeled, sem paleta aplicada, mantendo
as escritas descartadas/telemetria e a imagem bruta anteriores. Regravar e
restaurar não promove o perfil. As outras versões de subsistemas não mudam.

Fixtures antigas removem GAMM antes de montar layouts anteriores, preservando
PGRF/PFIFO/kernel posteriores e todas as asserções existentes. Não basta reduzir
o número de versão com o payload novo ainda presente.

## Verificação e reversão

Seis testes novos cobrem identidade/alias, RGB parcial, wrap/índice, largura e
preflight sem efeitos, replay entre componentes, corrupção e migração BUS0 v9.
O teste de BUS0 v1/v2 também verifica ausência de gamma; migrações dos outros
subscritos seguem na suíte completa. Gate/contagens na evidência de PLANO 6.1.
O primeiro gate encontrou a expectativa histórica de descarte do DAC no teste
de MMIO do loader. O teste de descarte foi mantido com portas vizinhas ainda
sem modelo, conservando contagem, largura, valor e ausência de escrita na RAM;
o efeito das portas do DAC é coberto pelos testes novos de gamma.
A aplicação da paleta à apresentação é a tarefa 6.2, ainda não entregue aqui.

Para reverter a política sem perder estados, manter leitor GAMM e o perfil
legado; uma mudança de comportamento exige outro perfil/versão, nunca redefinir
silenciosamente PublicReferenceV1 de arquivos existentes.
