#!/usr/bin/env bash
# Gate de entrega: fmt, clippy e testes do workspace, com contagem de testes.
#
# Uso: bash scripts/gate.sh
#
# Saída: a última linha é sempre "GATE OK: ..." ou "GATE FALHOU: ...", e o status
# de saída é 0 ou 1. O log completo fica em target/gate/<data>.log.
#
# A contagem de testes aprovados (soma de todas as linhas "test result") nunca pode
# cair abaixo de scripts/gate-baseline.txt. Quando sobe, o baseline é atualizado e
# deve entrar no mesmo commit da tarefa.
set -uo pipefail

raiz="$(cd "$(dirname "$0")/.." && pwd)"
cd "$raiz"
mkdir -p target/gate
log="target/gate/$(date +%Y%m%d-%H%M%S).log"
baseline_arq="scripts/gate-baseline.txt"
baseline="$(cat "$baseline_arq" 2>/dev/null || echo 0)"

falhou() {
    echo "GATE FALHOU: $1 (log: $log)"
    exit 1
}

fixado="$(sed -nE 's/^channel *= *"([^"]+)".*/\1/p' rust-toolchain.toml)"
ativo="$(rustc --version | cut -d' ' -f2)"
if [ "$fixado" != "$ativo" ]; then
    echo "aviso: rustc $ativo ativo, rust-toolchain.toml fixa $fixado (sem rustup?)" | tee -a "$log"
fi

echo "== cargo fmt" | tee -a "$log"
cargo fmt --all --check >>"$log" 2>&1 || falhou "cargo fmt --all --check"

echo "== cargo clippy" | tee -a "$log"
cargo clippy --workspace --all-targets --offline -- -D warnings >>"$log" 2>&1 ||
    falhou "cargo clippy com avisos ou erros"

echo "== cargo test (o teste native.rs leva ~80 s)" | tee -a "$log"
cargo test --workspace --offline --no-fail-fast >>"$log" 2>&1
status_testes=$?

read -r aprovados reprovados < <(grep -E '^test result:' "$log" | awk '
    { for (i = 1; i < NF; i++) {
        if ($(i + 1) ~ /^passed/) p += $i
        if ($(i + 1) ~ /^failed/) f += $i } }
    END { print p + 0, f + 0 }')

[ "$status_testes" -eq 0 ] && [ "$reprovados" -eq 0 ] ||
    falhou "$reprovados teste(s) falharam; $aprovados aprovados"
[ "$aprovados" -ge "$baseline" ] ||
    falhou "$aprovados testes aprovados, abaixo do baseline $baseline (algum teste sumiu?)"

if [ "$aprovados" -gt "$baseline" ]; then
    echo "$aprovados" >"$baseline_arq"
    echo "GATE OK: $aprovados testes (baseline $baseline -> $aprovados, atualizado em $baseline_arq) (log: $log)"
else
    echo "GATE OK: $aprovados testes (baseline $baseline) (log: $log)"
fi
