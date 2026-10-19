# Documentação

| Pasta | Conteúdo |
|---|---|
| `adr/` | Architecture Decision Records. `0000-template.md` é o modelo. |
| `arch/` | Um documento por subsistema, descrevendo como ele funciona de fato. |
| `compat/` | Matriz de compatibilidade: um arquivo por título testado. |
| `dev/` | Build, depuração, ferramentas, dependências de terceiros e licenças. |

O processo de documentação por melhoria está em [../ROADMAP.md](../ROADMAP.md) §12.

A direção de recompilação nativa está no [ADR-0013](adr/0013-recompilacao-estatica-rust.md).
Consulte [arquitetura](arch/recompilacao.md) e [demonstração e comandos](dev/recomp.md).
O [ADR-0014](adr/0014-oraculo-inteiro-host.md) documenta a referência de ALU na CPU física.
