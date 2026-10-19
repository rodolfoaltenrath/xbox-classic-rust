# ADR-0006: Renderer atrás de um trait, wgpu agora e ash previsto

- **Status:** aceito
- **Data:** 2026-10-19
- **Fase:** define a Fase 5f e a Fase 10

## Contexto

O tradutor do NV2A precisa de uma API gráfica moderna. `wgpu` dá Vulkan, validação e
portabilidade com pouco código. Mas o alvo final do projeto é apresentar **direto no
KMS/DRM**, sem compositor, e `wgpu` não usa `VK_KHR_display`. Além disso, o NV2A tem
estados legados (register combiners, formatos exóticos, blends antigos) que podem não
ter equivalente no modelo do `wgpu`.

## Alternativas consideradas

1. **Só wgpu** — menos código; impede a Fase 10 e limita a cobertura de estado.
2. **Só ash (Vulkan puro) desde já** — resolve os dois problemas; muito mais código
   antes do primeiro triângulo, e sem validação nem portabilidade de graça.
3. **Trait `GpuBackend` com implementação wgpu agora e ash previsto.**

## Decisão

Opção 3. O tradutor NV2A **nunca** referencia tipos do `wgpu`.

Decisão associada: **Vulkan é a API desde a Fase 5, nos dois sistemas operacionais.**
No Windows fixamos o backend Vulkan do `wgpu` (DX12 só como reserva), para que não
exista um caminho gráfico que apareça apenas na máquina final.

## Consequências

- Positivas: a Fase 10 troca apresentação, não API nem shaders; bugs de driver aparecem
  na máquina de desenvolvimento, não só no appliance.
- Negativas / dívida aceita: uma camada de indireção no caminho de submissão; risco de
  o trait vazar conceitos do `wgpu` se não houver disciplina.
- Provável: `ash` acontece de qualquer forma (KMS **ou** cobertura de estado), então o
  trait não é especulação.

## Como reverter

Remover o trait e acoplar ao `wgpu` seria abandonar a Fase 10. Ir direto para `ash` é
caro mas possível, e o trait torna isso incremental.
