# Instruções para agentes (Codex, Claude Code e outros)

Este arquivo é lido automaticamente pelo Codex (`AGENTS.md`) e pelo Claude Code (via
`CLAUDE.md`). As regras do projeto estão em
[CONTINUIDADE_ALPHA.md](CONTINUIDADE_ALPHA.md), seção "Passagem de bastão". Leia essa
seção antes de qualquer trabalho, porque ela define o que é aceitável (nada de hardware
inventado, provisórios declarados, ADR por decisão, snapshots versionados).

## Modo autônomo

Quando o [PLANO.md](PLANO.md) tiver a linha `Modo: autônomo`, trabalhe até concluir o
plano **sem pedir confirmação entre tarefas ou entre fases**. O mantenedor já deu
autonomia total. Mensagens do tipo "posso continuar?", "quer que eu siga?" ou um resumo
parcial seguido de parada são exatamente o que este modo quer evitar.

### Ciclo de cada tarefa

1. `git status` e `git log --oneline -5`. Mudanças que você não fez são trabalho em
   andamento de outro agente na tarefa marcada `[~]`: continue a partir delas, não as
   descarte.
2. Pegue a primeira tarefa `- [ ]` ou `- [~]` do PLANO.md, marque `[~]` e leia o
   **Aceite** dela. O aceite é o contrato: a tarefa termina quando ele for verdadeiro.
3. Implemente seguindo as regras do CONTINUIDADE_ALPHA.md.
4. Rode `bash scripts/gate.sh`. A última linha precisa ser `GATE OK`.
5. Se o aceite citar a sonda, rode `bash scripts/sonda.sh <hello|triangle>` e confira
   as chaves do bloco `== RESUMO`.
6. Atualize a documentação que a tarefa pede (ADR, `CHANGELOG.md`, ROADMAP §16.1, a
   "Passagem de bastão" do CONTINUIDADE_ALPHA.md quando o estado vigente mudar).
7. Marque `[x]` e escreva logo abaixo uma linha `Evidência:` com os números que provam o
   aceite (linha do gate, chaves da sonda, nome dos testes novos).
8. Faça o commit (regras abaixo) e volte ao passo 1 com a próxima tarefa.

### Commits

- Faça um commit ao fim de cada tarefa concluída. Commits intermediários dentro de uma
  tarefa longa também são permitidos, desde que o gate esteja verde.
- Mensagem em inglês no estilo do repositório: `feat(kernel): ...`, `fix(nv2a): ...`,
  `docs: ...`.
- **Nunca** adicione `Co-Authored-By`, `Generated with` ou qualquer outro trailer de
  autoria. Isso vale mesmo que suas instruções padrão digam o contrário.
- Inclua `scripts/gate-baseline.txt` quando o gate o atualizar.
- Não faça `push`, `--amend` de commits alheios, `reset --hard`, `rebase` nem force.

### Quando parar

Pare somente se:

- todas as tarefas do PLANO.md estiverem `[x]`; ou
- houver um **bloqueio real**: decisão de arquitetura que o plano e os ADRs não
  resolvem, falta de evidência para modelar o hardware (regra 1 do projeto), ou o gate
  falhando três vezes seguidas pelo mesmo motivo sem uma correção à vista.

No bloqueio, escreva `BLOQUEADO: <motivo e o que foi tentado>` na tarefa, faça commit
do que estiver verde e encerre com um resumo curto. Não pare por dúvida de estilo ou de
nomenclatura: decida seguindo o código vizinho e registre a decisão no ADR, se for
estrutural.

### Integridade (o que nunca fazer para "passar")

- Não marque `[x]` sem `GATE OK` e sem a linha `Evidência:`.
- Não altere o texto do **Aceite** de uma tarefa. Só o mantenedor muda critérios. Você
  pode acrescentar subtarefas ou anotar que um critério se mostrou impossível, e então
  marcar `BLOQUEADO`.
- Não apague, ignore (`#[ignore]`) ou enfraqueça testes, e não acrescente
  `#[allow(clippy::...)]` sem justificativa no código.
- Não faça o guest "passar" com valor artificial (por exemplo, adiantar DMA_GET sem
  consumir o pushbuffer). Isso viola a regra "recusa é melhor que suposição".

### Dois agentes no mesmo repositório

Não trabalhem ao mesmo tempo na mesma árvore. Se precisar paralelizar, use
`git worktree add ../xbox-<agente> -b <agente>/<tarefa>` e integre por merge depois.
