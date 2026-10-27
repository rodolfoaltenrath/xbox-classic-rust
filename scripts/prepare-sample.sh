#!/usr/bin/env bash
# Traduz com o emissor atual para uma pasta nova; stdout contém só o projeto.
# Uso: bash scripts/prepare-sample.sh <hello|triangle|gamma-fade|file-read|mesh|led> [arquivo.xbe]
set -euo pipefail
raiz="$(cd "$(dirname "$0")/.." && pwd)"
sample=${1:-}
case "$sample" in
    hello) padrao="$raiz/target/homebrew/hello.xbe" ;;
    triangle) padrao="$raiz/target/homebrew2/triangle-raw.xbe" ;;
    gamma-fade) padrao="$raiz/target/homebrew-gamma/gamma-fade-raw.xbe" ;;
    file-read) padrao="$raiz/target/homebrew-file-read/file-read-raw.xbe" ;;
    mesh) padrao="$raiz/target/homebrew-mesh/mesh-raw.xbe" ;;
    led) padrao="$raiz/target/homebrew-led/led-raw.xbe" ;;
    *) echo 'uso: prepare-sample.sh <hello|triangle|gamma-fade|file-read|mesh|led> [arquivo.xbe]' >&2; exit 2 ;;
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
# O lock standalone também faz parte da identidade de um build reutilizado.
if cargo generate-lockfile --offline --manifest-path "$projeto/Cargo.toml" >>"$log" 2>&1; then :; else
    status=$?; echo "DEPENDÊNCIAS FALHARAM: $log" >&2; tail -20 "$log" >&2; exit "$status"
fi
sha256sum "$xbe" >"$projeto/input.sha256"
{
    rustc --version; cargo --version; git rev-parse HEAD
    if git -c core.safecrlf=false diff --quiet && git -c core.safecrlf=false diff --cached --quiet; then echo 'tracked_tree=clean'; else echo 'tracked_tree=dirty'; fi
} >"$projeto/toolchain.txt"
sha256sum "$projeto/src/generated.rs" "$projeto/src/main.rs" "$projeto/src/lib.rs" >"$projeto/sources.sha256"
echo "Projeto traduzido: $projeto (log: $log)" >&2
printf '%s\n' "$projeto"
