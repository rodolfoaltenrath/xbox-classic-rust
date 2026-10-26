# ADR-0082 — Candidatos de callbacks atravessam MOVSS

Data: 2026-10-26. Estado: aceito. Bloqueio concreto de PLANO 6.3.

## Evidência e decisão

Gamma-fade com 64 fatias executou 18236259 instruções sem recusas, ainda em
identidade. Avançar o mesmo estado até 21907721 chegou a call eax em 0x115B4,
com alvo dinâmico 0x116B0 não traduzido. O PE original e o Rust emitido mostram
mov eax,[ebp+0x14], movss xmm0,[ebp-8], movss [esp],xmm0, call eax.
É a preparação de argumento float para o interpolador do sample nxdk fixado
no ADR-0081. Nenhum endereço de título é incorporado ao analisador.

O passe existente de escapes (ADR-0069) perdia todos os candidatos GPR em
qualquer SSE. MOVSS não escreve GPR: passa a preservar esses conjuntos durante
essa operação específica, tanto nas cargas quanto nas escritas XMM/memória.
Não substitui RAM por constante nem afirma que o candidato será chamado.
O executor continua lendo o endereço real e pode recusar UnknownTarget antes
de alterar a CPU/pilha. Conversões SSE para GPR e operações não classificadas
continuam invalidando; não é uma preservação genérica para toda SSE.

## Verificação e limites

Dois testes de análise exercitam escape para RAM, carga dinâmica, dois MOVSS e
chamada; o negativo sobrescreve EAX com CVTTSS2SI e exige ausência da raiz.
Testes existentes conservam aliases absolutos distintos e ausência de escape.
Um caso no Rust emitido executa dois candidatos após MOVSS lendo RAM alterada;
um terceiro endereço válido mas não traduzido recusa com snapshot integral
intacto. São 266 casos nativos em debug e release, gate 595.
O alvo real é retraduzido sem --entry manual; números finais no ADR-0081.
O domínio continua limitado a 4096 candidatos e as mesmas validações de
seção executável imutável, limites e sobreposição. Não há varredura linear
de dados como instruções ou fallback de interpretação.
