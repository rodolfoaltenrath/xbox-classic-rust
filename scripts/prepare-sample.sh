#!/usr/bin/env bash
# Traduz com o emissor atual para uma pasta nova; stdout contém só o projeto.
# Uso: bash scripts/prepare-sample.sh <hello|triangle> [arquivo.xbe]
set -euo pipefail
raiz="$(cd "$(dirname "$0")/.." && pwd)"
sample=${1:-}
case "$sample" in
    hello) padrao="$raiz/target/homebrew/hello.xbe" ;;
    triangle) padrao="$raiz/target/homebrew2/triangle-raw.xbe" ;;
    *) echo 'uso: prepare-sample.sh <hello|triangle> [arquivo.xbe]' >&2; exit 2 ;;
esac
[ "$#" -le 2 ] || { echo 'argumentos excedentes' >&2; exit 2; }
xbe=${2:-$padrao}
[ -f "$xbe" ] || { echo "XBE inexistente: $xbe" >&2; exit 2; }
xbe="$(cd "$(dirname "$xbe")" && pwd)/$(basename "$xbe")"
cd "$raiz"
mkdir -p target/samples
projeto=$(mktemp -d "$raiz/target/samples/$sample-XXXXXXXX")
log="$projeto.translate.log"
if cargo run --offline -q -p xberecomp -- translate "$xbe" --out "$projeto" >"$log" 2>&1; then :; else
    status=$?; echo "TRADUÇÃO FALHOU: $log" >&2; tail -20 "$log" >&2; exit "$status"
fi
sha256sum "$xbe" >"$projeto/input.sha256"
{ rustc --version; cargo --version; git rev-parse HEAD; } >"$projeto/toolchain.txt"
echo "Projeto traduzido: $projeto (log: $log)" >&2
printf '%s\n' "$projeto"
