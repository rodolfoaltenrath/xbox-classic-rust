# ADR-0005: Fastmem, a MMU do host como MMU do guest

- **Status:** aceito, **revisado pelo ADR-0010** (fastmem passa para a Fase 3b)
- **Data:** 2026-10-19
- **Fase:** define a Fase 2

## Contexto

Todo acesso à memória do guest pode custar de ~3 instruções (acesso direto) a ~15
(checagem de faixa + dispatch de MMIO + tradução de página). Em código de jogo, acesso
à memória é a operação mais frequente que existe: essa diferença define se o emulador
roda a 60fps no hardware-alvo.

## Alternativas consideradas

1. **Slowmem** — toda leitura/escrita passa por uma função que checa faixas e traduz.
   Simples, portável, depurável; caro.
2. **Fastmem** — mapear a RAM do guest no espaço de endereços do host (com os aliases
   físico, contíguo 0x80000000 e tiled 0xF0000000 sobre o mesmo backing store), de modo
   que o acesso do guest seja um acesso de memória comum. MMIO e páginas protegidas
   ficam sem permissão, e o fault é capturado e roteado.

## Decisão

Fastmem como caminho padrão, com slowmem **sempre disponível** por configuração.

## Consequências

- Positivas: ganho grande e direto no objetivo O2; o JIT pode emitir acesso sem
  bounds check.
- Negativas / dívida aceita: exige `unsafe` e handler de SIGSEGV (Linux) / VEH
  (Windows); bugs aqui são difíceis de diagnosticar; mantém `panic = "unwind"` nos
  perfis enquanto a recuperação de fault depender disso.
- Slowmem não é código morto: é a ferramenta de bissecção quando algo corrompe memória.

## Como reverter

Trocar a configuração para slowmem. O custo é performance, não corretude, e por isso os
dois caminhos precisam continuar passando a mesma suíte de testes.
