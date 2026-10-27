#!/usr/bin/env bash
# Executa um sample já traduzido em release e resume o resultado em linhas fixas,
# para que critérios de aceite possam ser conferidos sem ler o log inteiro.
#
# Uso: bash scripts/sonda.sh <hello|triangle|gamma-fade|file-read|mesh> [fatias]
#
# Sem SONDA_PROJETO, prepare-sample.sh retraduz o XBE com o emissor atual.
# SONDA_XBE seleciona outra entrada; SONDA_PROJETO reutiliza um projeto explícito.
# SONDA_TARGET_DIR seleciona o cache de build, separado do projeto preservado.
#
# Saída: log completo em target/sonda/<sample>-<data>.log e, no fim, um bloco
# "== RESUMO" com chaves estáveis:
#   parada:       última linha de erro/encerramento
#   chamadas:     N atendidas
#   recusadas:    N
#   framebuffer:  linha "Framebuffer guest: ..."
#   mmio_sem_modelo: endereços físicos distintos reportados como sem modelo
#   toca <addr>:  presente para cada endereço vigiado que aparece como sem modelo
set -euo pipefail

raiz="$(cd "$(dirname "$0")/.." && pwd)"
cd "$raiz"

sample="${1:-}"
case "$sample" in
hello)
    fatias="${2:-64}"
    ;;
triangle)
    fatias="${2:-4000}"
    ;;
gamma-fade)
    fatias="${2:-128}"
    ;;
file-read)
    fatias="${2:-512}"
    ;;
mesh)
    fatias="${2:-320}"
    ;;
*)
    echo "uso: $0 <hello|triangle|gamma-fade|file-read|mesh> [fatias]" >&2
    exit 2
    ;;
esac
[ "$#" -le 2 ] && [[ "$fatias" =~ ^[1-9][0-9]{0,8}$ ]] || {
    echo 'SONDA FALHOU: fatias deve ser inteiro positivo; argumentos excedentes recusados' >&2; exit 2;
}
proj=${SONDA_PROJETO:-}
if [ -z "$proj" ]; then
    if [ -n "${SONDA_XBE:-}" ]; then
        proj=$(bash scripts/prepare-sample.sh "$sample" "$SONDA_XBE")
    else
        proj=$(bash scripts/prepare-sample.sh "$sample")
    fi
fi
for arquivo in Cargo.toml src/image.xbe src/generated.rs src/main.rs; do
    [ -f "$proj/$arquivo" ] || { echo "SONDA FALHOU: projeto inválido: $proj/$arquivo" >&2; exit 2; }
done
if [ -n "${SONDA_PROJETO:-}" ] && [ -n "${SONDA_XBE:-}" ]; then
    cmp -s "$SONDA_XBE" "$proj/src/image.xbe" || {
        echo 'SONDA FALHOU: XBE diferente do projeto fornecido' >&2; exit 2;
    }
fi
alvo=${SONDA_TARGET_DIR:-"$raiz/target/sample-build/$sample"}

mkdir -p target/sonda
log=$(mktemp "target/sonda/$sample-$(date +%Y%m%d-%H%M%S)-XXXXXXXX.log")
echo "sonda $sample: compilando (a primeira compilação leva minutos)..."
printf 'projeto: %s\n' "$proj" | tee "$log"
sha256sum "$proj/src/image.xbe" | tee -a "$log"
if cargo build --release --offline --manifest-path "$proj/Cargo.toml" --target-dir "$alvo" >>"$log" 2>&1; then :; else
    status=$?; echo "SONDA FALHOU: compilação, status $status (log: $log)" >&2; exit "$status"
fi
binary="$alvo/release/obsidian-native"
[ ! -f "$binary.exe" ] || binary="$binary.exe"
[ -f "$binary" ] || { echo "SONDA FALHOU: executável inexistente: $binary" >&2; exit 1; }
echo "sonda $sample: executando --fatias $fatias..."
status=0
"$binary" --fatias "$fatias" >>"$log" 2>&1 || status=$?
# O main usa status 1 para orçamento. Qualquer outro erro precisa falhar a sonda.
if [ "$status" -ne 0 ]; then
    if [ "$status" -ne 1 ] || ! grep -qE '^Error: "?execução interrompida após [0-9]+ blocos; [0-9]+ chamadas atendidas, [0-9]+ recusadas;' "$log"; then
        echo "SONDA FALHOU: execução, status $status (log: $log)" >&2; tail -10 "$log" >&2; exit "$status"
    fi
fi
grep -q '^Kernel: [0-9].* chamadas atendidas, [0-9].* recusadas;' "$log" &&
    grep -q '^Framebuffer guest:' "$log" || {
    echo "SONDA FALHOU: métricas obrigatórias ausentes (log: $log)" >&2; exit 1;
}

# Endereços NV2A acompanhados pelo PLANO.md (NV_USER DMA_PUT/DMA_GET do canal 0).
vigiados="0xFD800040 0xFD800044"

{
    echo
    echo "== RESUMO ($sample, --fatias $fatias, status $status)"
    echo "parada: $(grep -E '^Error:|Execução nativa concluída|execução (interrompida|concluída)' "$log" | tail -1)"
    echo "chamadas: $(grep -oE '[0-9]+ chamadas atendidas' "$log" | tail -1 | grep -oE '^[0-9]+')"
    echo "recusadas: $(grep -oE '[0-9]+ recusadas' "$log" | tail -1 | grep -oE '^[0-9]+')"
    echo "framebuffer: $(grep -E '^Framebuffer guest:' "$log" | tail -1)"
    echo "raster: $(grep -E '^GPU raster:' "$log" | tail -1)"
    echo "gamma: $(grep -E '^DAC gamma:' "$log" | tail -1)"
    echo "cores: $(grep -E '^  regiões de cor dos vértices' "$log" | tail -1)"
    echo "mmio_sem_modelo: $(grep -E 'fis:0x[0-9A-F]+ .*(não implementado|sem modelo)' "$log" |
        grep -oE 'fis:0x[0-9A-F]+' | sort -u | wc -l)"
    for addr in $vigiados; do
        if grep -qE "fis:$addr .*(não implementado|sem modelo)" "$log"; then echo "toca $addr: sem modelo"; fi
    done
    echo "log: $log"
} | tee -a "$log"
