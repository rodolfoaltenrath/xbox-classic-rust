# Auditoria de unsafe da memória

Data: 2026-10-20. Escopo: `crates/xbox-mem/src/arena.rs`.

Há **16 blocos unsafe**, um `unsafe impl Send`, dois blocos de declarações FFI e
quatro funções unsafe internas, contando simultaneamente Linux, Windows e fallback.
Não são dois/quatro blocos como previsto em textos iniciais. O crate nega unsafe
fora de `arena` e exige comentários SAFETY pelo Clippy.

| Local em arena.rs | Blocos | Invariante | Evidência |
|---|---:|---|---|
| `com_tamanho`, alocação fallback | 1 | tamanho não nulo, múltiplo de página e <= isize::MAX; layout válido; ponteiro nulo é erro | T1/T2 sob Miri; limites antes de alocar |
| `bytes` / `bytes_mut` | 2 | ponteiro válido/alinhado por toda a vida da arena; tamanho dentro da alocação; exclusividade de `&mut self` | T1/T2/T3/T5/T10 sob Miri |
| `Drop` | 2 | origem determina a liberação; ponteiro/layout ou mapping correspondem à criação; liberar uma vez | criação/drop nos contratos e testes de arena |
| Linux `criar` | 5 | string C válida, fd verificado, ftruncate antes de mmap, MAP_FAILED/nulo tratados, fechar fd nas falhas | inspeção de SAFETY; teste nativo Linux previsto no CI; FFI não executada no Miri |
| Linux `destruir` | 1 | munmap recebe mapping/tamanho originais; fechar fd uma vez | contrato interno de Drop; teste nativo Linux previsto no CI |
| Windows `criar` | 3 | handle verificado; MapViewOfFile cobre seção RW; fechar handle se view falhar | testes nativos Windows e T14 local |
| Windows `destruir` | 1 | unmap da view original e fechamento único do handle | Drop nos testes nativos Windows |
| wrapper `backing::destruir` | 1 | repassar mapping/objeto originais à implementação correta | contrato de Drop |

As quatro funções unsafe são os três destruidores por plataforma e o wrapper;
o destruidor de sistema sem suporte é vazio e inalcançável, pois `criar` retorna
None nessa plataforma. Declarações extern apenas descrevem a ABI; cada chamada
efetiva está incluída nos blocos acima.

`unsafe impl Send` depende de posse exclusiva: mover a arena transfere a região
inteira e seu recurso de SO. **Arena não implementa Sync nem Clone**. O descritor
exposto é emprestado conceitualmente e só vale durante a vida da arena; não deve
ser fechado ou usado para criar acesso concorrente às fatias Rust.

Miri executa o fallback de alocação, seguido dos mesmos acessos de arena, MMU, bus
e snapshot. Seis testes de contrato cobrem T1–T10 com 64 KiB; o traço T11/T12 de
20000 cenários roda nativamente. Não há diagnóstico de UB nessa execução.
Isso não comprova ausência de UB para todos os caminhos, nem valida FFI de SO,
compartilhamento Vulkan, processos externos ou concorrência CPU/GPU.

Comandos e resultados: [validação de memória](../dev/memoria.md).
