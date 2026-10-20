# ADR-0021 — TLS nxdk e fronteira explícita de chamadas HLE

Data: 2026-10-20. Estado: aceito para o perfil inicial de uma thread AOT.

## Contexto

O `hello` compilado pelo nxdk real importa 69 funções e 3 dados, tem TLS e cria
uma thread para `main`. Fixtures sem imports não verificavam nenhuma dessas
convenções. O esquema genérico de TLS do Windows não descreve o índice negativo
e o slot relativo ao topo da pilha usados pelo nxdk.

## Decisão

Inicializar um perfil limitado de NT_TIB/TLS com referências públicas fixadas;
representar FS por um endereço guest em `State`; manter LEA sem base de segmento.
Reservar regiões de startup contra sobreposição, limitar TLS e recusar callbacks
ou alinhamentos ainda não implementados. Snapshot AOTC v3 preserva FS; versões
anteriores recebem base zero.

Gerar catálogo de ABI a partir das declarações CC0 do nxdk. Resolver funções para
slots guest RX com marcador e ordinal, aceitos explicitamente pelo código Rust
emitido. Não usar ponteiros de função host nem pilha nativa como continuação.
Sair por `Exit::KernelCall`; ler argumentos e completar o retorno pela ABI
correspondente, sem inventar valores de sucesso para serviços faltantes.
Manter uma chamada pendente inteiramente em CPU/RAM para permitir snapshot.

Recusar dados exportados até que possuam representação e semântica reais. Por
isso o `hello` completo ainda não pode executar nesta entrega. Os três dados,
serviços, scheduler, ISA restante e framebuffer continuam no aceite do alpha.

## Evidência e consequências

Build, hashes locais, fontes públicas e limites estão em
[`homebrew.md`](../dev/homebrew.md). Testes de Rust gerado cobrem TLS/FS, LEA,
stdcall/cdecl/fastcall, orçamento, tail call, snapshot pendente e falhas sem
commit parcial. O caminho de inspeção do loader permanece independente da
preparação AOT. Não foram adicionados unsafe nem dependências Cargo externas.

A build utiliza SDK externo C/C++ como entrada de teste. Os componentes do
Obsidian continuam em Rust. Nenhum código de BIOS é necessário para esta etapa.
