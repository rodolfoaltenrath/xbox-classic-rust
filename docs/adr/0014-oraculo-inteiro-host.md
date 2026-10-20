# ADR-0014: Validar a ALU AOT contra a CPU física do host

- **Status:** aceito
- **Data:** 2026-10-19
- **Fase:** 3-AOT

## Contexto e alternativas

A fixture inicial comprova compilação/execução, mas não valida independentemente
FLAGS, overflow e subregistradores. Tabelas manuais têm cobertura limitada. KVM
continua planejado para uma referência mais ampla, mas não atende ao host Windows
atual sem outra infraestrutura de virtualização.

## Decisão

Executar instruções inteiras fixas sobre registradores da CPU física, capturar
resultado/RFLAGS e comparar com o Rust traduzido. `xbox-cpu::oracle` fica atrás de
`isa-oracle` e `target_arch = "x86_64"`. Não é backend de execução guest: a API aceita
enums e inteiros, sem ponteiros, opcodes ou arquivos.

Os três blocos `unsafe` do macro de ALU estão em `xbox-cpu`, crate autorizado
pelo ROADMAP, com comentários SAFETY. Cada caso faz BT para definir CF, uma operação
fixa, e PUSHFQ/POP balanceado. Não toca DF nem executa endereços guest. O runtime
AOT continua Rust seguro e não depende dessa feature de teste.

O teste compila 33 casos AOT (11 operações × 3 larguras) e verifica 7986 combinações
de entradas/carry contra a CPU. AF indefinido nas operações lógicas é excluído da
comparação; NOT exige preservação das flags. Outros 3872 casos validam os 16 Jcc
curtos/near contra relações assinadas/não assinadas, overflow, sinal e paridade.
Os testes de Jcc não são apresentados como execução de branches guest no host.

### Ampliação de ADC/SBB (2026-10-19)

A mesma referência passa a executar ADC/SBB fixos, com BT inicializando CF antes
da instrução. São 39 casos AOT (13 operações × 3 larguras), com 9438 combinações na
matriz geral. Mais 262144 casos esgotam os operandos de byte e os dois carries para
ADC/SBB, comparando resultado, CF/PF/AF/ZF/SF/OF e preservação dos demais bits.
Não há novos blocos `unsafe` ou alteração do formato de snapshot.

O [ADR-0015](0015-flags-shifts-inteiros.md) amplia a referência com shifts e
documenta os três blocos de assembly adicionais para controlar flags de entrada.

## Consequências e limites

- A implementação e a referência não compartilham o cálculo de resultado/FLAGS.
- Comparar o registrador completo também detecta corrupção dos bits superiores.
  Testes próprios exercitam AH, extensões e memória estreita.
- A dependência do oráculo entra apenas nos testes do projeto gerado.
- Não cobre x87/MMX/SSE, segmentação, exceções, IRQL, temporização ou efeitos de
  memória da CPU original. Não substitui KVM ou validação em hardware Xbox.
- Não exige BIOS. O arquivo local disponibilizado pelo mantenedor não foi aberto
  para derivar código ou comportamento desta implementação.

## Referências

- [Intel SDM, vol. 2 — instruções e flags](https://www.intel.com/content/www/us/en/developer/articles/technical/intel-sdm.html).
- [Rust Reference — inline assembly](https://doc.rust-lang.org/reference/inline-assembly.html).

## Como reverter

Desabilitar a feature remove o oráculo. A tradução e a ABI de estado/snapshot não
dependem dele e não precisam mudar.
