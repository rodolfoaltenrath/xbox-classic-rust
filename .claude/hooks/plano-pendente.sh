#!/usr/bin/env bash
# Hook Stop do Claude Code: impede que o agente encerre o turno enquanto o
# PLANO.md estiver em modo autônomo e ainda tiver tarefas abertas.
#
# Deixa encerrar quando:
#   - não há PLANO.md, ou a linha "Modo: autônomo" não está presente;
#   - alguma tarefa está marcada BLOQUEADO;
#   - não sobra nenhuma tarefa "- [ ]" ou "- [~]";
#   - o PLANO.md não mudou nas últimas 3 tentativas de encerrar (sem progresso,
#     evita laço infinito).
set -uo pipefail

cd "${CLAUDE_PROJECT_DIR:-$(pwd)}" || exit 0
plano="PLANO.md"
[ -f "$plano" ] || exit 0
grep -qE '^Modo: autônomo' "$plano" || exit 0
grep -qE '^\s*(- \[.\] .*)?BLOQUEADO: [^<]' "$plano" && exit 0

pendentes="$(grep -cE '^\s*- \[( |~)\]' "$plano")"
[ "$pendentes" -gt 0 ] || exit 0

estado=".git/plano-hook-estado"
hash_atual="$(sha1sum "$plano" | cut -d' ' -f1)"
read -r hash_antigo repeticoes <"$estado" 2>/dev/null || true
if [ "${hash_antigo:-}" = "$hash_atual" ]; then
    repeticoes=$((${repeticoes:-0} + 1))
else
    repeticoes=1
fi
echo "$hash_atual $repeticoes" >"$estado"
[ "$repeticoes" -le 3 ] || exit 0

proxima="$(grep -m1 -E '^\s*- \[( |~)\]' "$plano" | sed -E 's/^\s*//')"
echo "PLANO.md está em modo autônomo e ainda tem $pendentes tarefa(s) aberta(s). \
Próxima: \"$proxima\". Siga o ciclo do AGENTS.md (implementar, gate, aceite, marcar [x], \
commit) e continue. Se houver um bloqueio real, marque a tarefa com BLOQUEADO: <motivo> e encerre." >&2
exit 2
