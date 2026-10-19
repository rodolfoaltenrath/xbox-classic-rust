# ADR-0012: Sandbox das pastas montadas na VFS

- **Status:** aceito (parcialmente implementado)
- **Data:** 2026-10-19
- **Fase:** 1 (mitigação), 7 (correção completa)

## Contexto

Os drives graváveis do console (`T:`, `U:`, `Z:`) são pastas do host. O título pede
caminhos como `T:\save.bin`, e um título malformado ou malicioso pode pedir
`T:\..\..\..\Windows\System32\config`.

A implementação da Fase 1 recusava componentes `..`, recusava componentes com `:` e
conferia que o caminho resolvido continuava sob a raiz. Revisão externa apontou,
corretamente, que isso **não é sandbox**:

- **link simbólico**: um link dentro da pasta montada aponta para fora, e nenhum
  componente do caminho é `..`;
- **corrida entre verificar e abrir** (TOCTOU): o caminho é validado, e entre a
  validação e a abertura o alvo é trocado por um link.

## Alternativas consideradas

1. **Manter como estava e documentar** — custo zero, deixa aberta uma fuga realizável
   sem corrida nenhuma.
2. **Canonicalizar antes de abrir** — fecha a fuga por link simbólico; não fecha a
   corrida. Portável, poucas linhas.
3. **Abertura relativa a descritor de diretório** com `openat2(RESOLVE_BENEATH)` no
   Linux e verificação do caminho real do handle depois de abrir no Windows — fecha as
   duas; é código específico de SO, que pertence a `xbox-platform`.

## Decisão

Fazer 2 agora e 3 na Fase 7, quando `xbox-platform` ganhar corpo.

Dimensionamento do risco, que justifica a dívida: o atacante é um XBE que o próprio
usuário escolheu executar, e o alcance é o sistema de arquivos do usuário que já rodou
aquele binário. Não é serviço exposto nem conteúdo de terceiro não confiável. Isso
justifica agendar a corrida — mas não justificava deixar o link simbólico aberto, que
custava pouco para fechar.

## Consequências

- Positivas: a fuga realizável sem corrida está fechada, com teste; o caminho para a
  correção completa está escolhido e registrado.
- Negativas / dívida aceita: a janela TOCTOU continua aberta até a Fase 7.
- `std::fs::canonicalize` falha em caminho que não existe, então a escrita (Fase 9) vai
  precisar canonicalizar o diretório pai, não o alvo.
- No Windows a canonicalização devolve caminhos `\?\`, então a raiz também é
  canonicalizada antes da comparação de prefixo.

## Como reverter

Não há motivo para reverter a mitigação. A correção completa pode ser adiada sem
bloquear nada além do próprio item da Fase 7.
