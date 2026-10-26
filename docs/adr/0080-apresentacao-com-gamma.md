# ADR-0080 — Aplicar o DAC à cópia apresentada

Data: 2026-10-26. Estado: aceito. Complementa ADRs 0052 e 0079.

## Problema e decisão

A rampa escrita pelo guest já existe no BUS0 v10/GAMM v1, mas a janela ainda
mostrava bytes anteriores ao DAC. Isso escondia fades mesmo com a execução
correta. O frontend passa a capturar uma cópia própria do framebuffer e, para
PublicReferenceV1, substituir R, G e B pelas respectivas componentes da tabela
RGB8. Alfa permanece intacto. Não há curva calculada pelo host, reconhecimento
de título, shader ou ajuste da RAM. O apresentador recebe apenas um Quadro
com bytes próprios; xbox-platform continua sem dependência de xbox-mem.

`--quadro caminho` conserva BGRA bruto, sem padding. A opção independente
`--quadro-apresentado caminho` exporta os mesmos bytes usados na janela após
o DAC. Ambos exigem arquivo novo e funcionam sem janela. BUS0 antigo migra
para LegacyUnmodeled e continua mostrando/exportando bytes brutos; restaurar
um estado nunca ativa uma paleta implicitamente. Não muda formato de snapshot.

As fontes públicas e limites físicos do dispositivo estão fixados no ADR-0079.
A transformação digital é a seleção independente por canal dessa paleta.
Flicker/soften, entrelaçamento, sinal analógico e gestão de cores do monitor
host não são modelados. P2 fica limitado aos efeitos restantes; P21 permanece.

## Verificação

Teste de Quadro percorre 256 níveis, identidade, canais diferentes e alfa.
Uma variante da fixture display gera instruções x86 reais que programam
cinco entradas do DAC após configurar o framebuffer: Rust emitido executa
essas escritas, captura RAM inalterada e produz pixels de saída distintos.
Replay desde o componente R compara sessão integral e imagem; BUS0 v9 sem
GAMM mantém apresentação bruta mesmo depois de novas escritas guest.
Opções incompletas/repetidas recusam; subprocesso do main emitido exporta
as duas imagens e recusa sobrescrever cada uma. Hello/triangle identidade
são novamente sondados na tarefa 6.3 com o runtime final.

Não se mede fidelidade analógica por uma igualdade de pixels. Uma política
futura exige perfil/migração explícitos, sem redefinir PublicReferenceV1.
