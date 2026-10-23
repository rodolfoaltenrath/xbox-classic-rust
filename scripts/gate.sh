#!/usr/bin/env bash
# Gate de entrega: fmt, clippy e testes do workspace, com contagem de testes.
#
# Uso: bash scripts/gate.sh
#
# Saída: a última linha é sempre "GATE OK: ..." ou "GATE FALHOU: ...", e o status
# de saída é 0 ou 1. O log completo fica em target/gate/<data>.log.
#
# A contagem de testes aprovados (soma de todas as linhas "test result") nunca pode
# cair abaixo do baseline da plataforma. Windows tem quatro testes de janela a
# mais que Linux: comparar a mesma contagem entre SOs criaria regressão falsa.
# O fallback anterior fica em gate-baseline.txt; incluir qualquer baseline
# atualizado no commit da tarefa. Nenhum piso conhecido pode ser reduzido.
set -uo pipefail

raiz="$(cd "$(dirname "$0")/.." && pwd)"
cd "$raiz"
mkdir -p target/gate
log=$(mktemp "target/gate/$(date +%Y%m%d-%H%M%S)-XXXXXXXX.log")
baseline_arq="scripts/gate-baseline.txt"
case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*) baseline_arq="scripts/gate-baseline-windows.txt" ;;
    Linux*) baseline_arq="scripts/gate-baseline-linux.txt" ;;
esac
baseline="$(cat "$baseline_arq" 2>/dev/null || echo 0)"
# Baseline ausente/inválido é configuração quebrada, não autorização para perder testes.
[[ "$baseline" =~ ^[1-9][0-9]*$ ]] || {
    echo "GATE FALHOU: baseline inválido/ausente: $baseline_arq"; exit 1;
}

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

aprovados=0
reprovados=0
while IFS= read -r linha; do
    if [[ "$linha" =~ ^test[[:space:]]result:.*[[:space:]]([0-9]+)[[:space:]]passed\;[[:space:]]([0-9]+)[[:space:]]failed ]]; then
        aprovados=$((aprovados + 10#${BASH_REMATCH[1]}))
        reprovados=$((reprovados + 10#${BASH_REMATCH[2]}))
    fi
done < "$log"

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
