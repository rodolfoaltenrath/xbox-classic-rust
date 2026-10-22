#!/usr/bin/env bash
# Executar da raiz do repositório (Git Bash ou Linux):
#   bash scripts/find-translation-roots.sh <xbe> <diretório NOVO> [raiz...]
# O entry point já é incluído pelo analisador; raízes iniciais são opcionais.
# Só acrescenta destinos observados na execução, sujeitos à validação AOT.
# Não é descoberta estática de jump tables. Mantém tentativas e logs, sem apagar.
set -euo pipefail
xbe=${1:?uso: find-translation-roots.sh <xbe> <diretório NOVO> [raiz...]}
project=${2:?}
shift 2
max_rounds=${ROOT_SEARCH_MAX_ROUNDS:-40}
seconds=${ROOT_SEARCH_TIMEOUT_SECONDS:-120}
slices=${ROOT_SEARCH_SLICES:-4000}
for value in "$max_rounds" "$seconds" "$slices"; do
    [[ "$value" =~ ^[1-9][0-9]{0,8}$ ]] || { echo "limites devem ser inteiros positivos" >&2; exit 2; }
done
[[ -f "$xbe" ]] || { echo "XBE inexistente: $xbe" >&2; exit 2; }
for tool in cargo timeout; do
    command -v "$tool" >/dev/null || { echo "ferramenta ausente: $tool" >&2; exit 2; }
done
normalize() {
    [[ "$1" =~ ^0[xX]([[:xdigit:]]{1,8})$ ]] || { echo "raiz inválida: $1" >&2; return 2; }
    printf '0x%08X' "$((16#${BASH_REMATCH[1]}))"
}
roots=()
for value in "$@"; do
    value=$(normalize "$value")
    if [[ " ${roots[*]} " != *" $value "* ]]; then roots+=("$value"); fi
done
# mkdir sem -p recusa também diretório vazio ou symlink já existente.
mkdir -p -- "$(dirname -- "$project")"
mkdir -- "$project" || { echo "o diretório de saída deve ser novo; nenhum arquivo removido" >&2; exit 2; }
project=$(cd -- "$project" && pwd -P)
target_dir="$project/build"
write_roots() {
    printf '%s\n' "${roots[@]}" > "$project/roots.txt"
}
write_roots
for ((i=1; i<=max_rounds; i++)); do
    printf -v attempt '%s/attempt-%02d' "$project" "$i"
    args=(); for value in "${roots[@]}"; do args+=(--entry "$value"); done
    printf '%s\n' "$attempt" > "$project/final-project.txt"
    if cargo run -q --offline -p xberecomp -- translate "$xbe" "${args[@]}" --out "$attempt" \
        > "$attempt.translate.log" 2>&1; then :; else
        status=$?; echo "TRADUÇÃO FALHOU: $attempt.translate.log" >&2
        cat -- "$attempt.translate.log" >&2; exit "$status"
    fi
    if cargo build -q --release --offline --manifest-path "$attempt/Cargo.toml" --target-dir "$target_dir" \
        > "$attempt.build.log" 2>&1; then :; else
        status=$?; echo "BUILD FALHOU: $attempt.build.log" >&2
        tail -20 -- "$attempt.build.log" >&2; exit "$status"
    fi
    binary="$target_dir/release/obsidian-native"
    if [[ -f "$binary.exe" ]]; then binary="$binary.exe"; fi
    status=0
    timeout "$seconds" "$binary" --fatias "$slices" > "$attempt.run.out" 2> "$attempt.run.err" || status=$?
    echo "volta $i: status=$status; projeto=$attempt"
    echo "RAÍZES: ${roots[*]}"
    if [[ "$status" == 0 ]]; then
        echo "FIM: programa retornou. Logs: $attempt.run.out e $attempt.run.err"; exit 0
    fi
    if [[ "$status" == 124 || "$status" == 137 || "$status" == 143 ]]; then
        echo "TEMPO ESGOTADO ou execução interrompida; veja $attempt.run.err" >&2; exit "$status"
    fi
    target=''
    while IFS= read -r line || [[ -n "$line" ]]; do
        if [[ "$line" =~ (alvo|endereço)[[:space:]](0x[[:xdigit:]]{1,8})[[:space:]]não[[:space:]](foi[[:space:]]recompilado|tem[[:space:]]bloco[[:space:]]recompilado) ]]; then
            target=$(normalize "${BASH_REMATCH[2]}"); break
        fi
    done < "$attempt.run.err"
    if [[ -z "$target" ]]; then
        echo "PARADA: erro diferente de destino ausente (status=$status)." >&2
        tail -c 3000 -- "$attempt.run.err" >&2; exit "$status"
    fi
    if [[ " ${roots[*]} " == *" $target "* ]]; then
        echo "PARADA: alvo $target já informado; veja $attempt.run.err" >&2; exit 1
    fi
    roots+=("$target"); write_roots
    echo "nova raiz observada: $target (evidência: $attempt.run.err)"
done
echo "LIMITE de $max_rounds tentativas; última raiz ainda não traduzida. Veja $project/roots.txt" >&2
exit 1
