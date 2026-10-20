# ADR-0020 — Loader compartilhado e aceite local de memória

**Status:** aceito · **Data:** 2026-10-20 · **Fases:** 2 e preparação da 4

## Contexto

O AOT já usava o bus, mas o aceite de memória exigia provar o percurso de um XBE
real do arquivo até a MMU, incluindo imports e alias físico. O loader da prova
recusava TLS/imports e páginas RWX, presentes no NFSU2 local. Uma inspeção não
precisa executar essas páginas nem resolver imports.

## Decisão

- `xbox-kernel::loader::Image` carrega headers, seções e BSS com frames alocados,
  permissões unificadas por página e validação de faixas/recursos. A mesma validação
  e o mesmo carregamento servem à sessão AOT. O AOT acrescenta sua pilha e sentinela.
- `Mode::Inspect` aceita permissões RWX declaradas e monta a janela física em
  `0x80000000 + PA`, sem executar código. A imagem não pode ocupar essa janela.
  `obsidian check-memory` aceita XBE, pasta com default.xbe ou ISO sob demanda.
  Inspeção não monta drives graváveis nem altera a mídia.
- `Mode::Aot` recusa páginas simultaneamente graváveis e executáveis, inclusive
  páginas compartilhadas entre seções. Não mapeia o alias físico RW: ele permitiria
  alterar os bytes de código que o AOT presume imutáveis. TLS/imports continuam
  recusados na sessão até a implementação do ambiente de execução da Fase 4.
- `Image::verify` compara headers, bytes das seções e BSS pelo bus e pelo alias
  traduzido por PA. Compara quatro bytes do entry point e exige execução permitida;
  compara ordinais lidos pelo bus com `Xbe::kernel_imports`, com limites e terminador.
  Confirma também uma página ausente. Uma imagem de teste com menos de quatro
  bytes de arquivo no entry não atende essa verificação T14, embora possa carregar.
- O teste T14 usa fixture no CI e mídia local em teste ignorado explícito com
  `OBSIDIAN_MIDIA_REAL`. O arquivo comercial e bytes extraídos nunca são versionados.
- T1–T10 têm contratos com arena pequena sob Miri; T11/T12 compara 20000 cenários
  com um vetor independente. Diferencial com fastmem continua adiado até existir.
  O benchmark seguro é local, sem requisito de FPS ou limiar no CI.
- Documentar os 16 blocos unsafe existentes e o impl Send em vez de repetir a
  estimativa inicial de dois/quatro blocos. Recusar tamanho > isize::MAX antes de
  FFI/alocação, respeitando os requisitos de fatias Rust. Miri usa fallback.
- A arena mantém posse exclusiva, **Send e não Sync**, sem Clone. A proposta de
  aceitar data race no ADR-0011 fica substituída: a Fase 5 precisará de transferência
  de posse, cópia ou sincronização que também respeite o modelo de memória Rust.
  Protocolo guest sozinho não justifica referências Rust com acessos concorrentes.
- TLB e compartilhamento Vulkan permanecem trabalhos posteriores, condicionados
  a medição e ao backend. Um memfd/handle guardado ainda não comprova importação
  Vulkan nem oferece acesso concorrente seguro.

## Evidência e limites

NFSU2 local: 14 seções, 1132 páginas, 4634697 bytes de headers/seções/BSS e 152
ordinais verificados, entry 0x0021B1CE contendo 0x01180D8B. Nenhum código do jogo
é executado. Isso fecha o T14 e o aceite local de memória, sem declarar kernel,
GPU, homebrew real executando ou compatibilidade comercial.

Os 19 avisos anteriores de Clippy em memória foram corrigidos; o formato de snapshot
da memória não mudou. Detalhes de host, testes, Miri e benchmark ficam em
[docs/dev/memoria.md](../dev/memoria.md). Validação FFI Linux depende do CI/outro host.

## Como reverter

Retirar `check-memory` não altera o formato XBE ou os snapshots. Uma reversão do
loader compartilhado deve manter validação idêntica na análise e no carregamento.
Não habilitar alias RW no AOT sem invalidação/identificação do código traduzido.
