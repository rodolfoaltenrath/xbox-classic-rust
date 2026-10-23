#!/usr/bin/env bash
# Executa um sample já traduzido em release e resume o resultado em linhas fixas,
# para que critérios de aceite possam ser conferidos sem ler o log inteiro.
#
# Uso: bash scripts/sonda.sh <hello|triangle> [fatias]
#
# Projetos usados (Rust gerado pelo xberecomp; recompilam contra o código atual):
#   hello    -> target/homebrew/boot-review-20261022        (--fatias 64 por padrão)
#   triangle -> target/homebrew2/auto-tables-review-20261023   (sem --entry manual; --fatias 4000)
# Se a tarefa exigir raízes novas, retraduza (CONTINUIDADE_ALPHA.md, "Reproduzir o
# triangle") e atualize o caminho aqui no mesmo commit.
#
# Saída: log completo em target/sonda/<sample>-<data>.log e, no fim, um bloco
# "== RESUMO" com chaves estáveis:
#   parada:       última linha de erro/encerramento
#   chamadas:     N atendidas
#   recusadas:    N
#   framebuffer:  linha "Framebuffer guest: ..."
#   mmio_sem_modelo: endereços físicos distintos reportados como sem modelo
#   toca <addr>:  presente para cada endereço vigiado que aparece como sem modelo
set -uo pipefail

raiz="$(cd "$(dirname "$0")/.." && pwd)"
cd "$raiz"

sample="${1:-}"
case "$sample" in
hello)
    proj="target/homebrew/boot-review-20261022"
    alvo="target/homebrew/boot-target"
    fatias="${2:-64}"
    ;;
triangle)
    proj="target/homebrew2/auto-tables-review-20261023"
    alvo="target/homebrew2/pramin-target"
    fatias="${2:-4000}"
    ;;
*)
    echo "uso: $0 <hello|triangle> [fatias]" >&2
    exit 2
    ;;
esac

[ -f "$proj/Cargo.toml" ] || {
    echo "SONDA FALHOU: projeto $proj não existe; retraduza o sample" >&2
    exit 1
}

mkdir -p target/sonda
log="target/sonda/$sample-$(date +%Y%m%d-%H%M%S).log"
echo "sonda $sample: compilando e executando (a primeira compilação leva minutos)..."
cargo run --release --offline --manifest-path "$proj/Cargo.toml" \
    --target-dir "$alvo" -- --fatias "$fatias" >"$log" 2>&1
status=$?

# Endereços NV2A acompanhados pelo PLANO.md (NV_USER DMA_PUT/DMA_GET do canal 0).
vigiados="0xFD800040 0xFD800044"

{
    echo
    echo "== RESUMO ($sample, --fatias $fatias, status $status)"
    echo "parada: $(grep -E '^Error:|execução (interrompida|concluída)' "$log" | tail -1)"
    echo "chamadas: $(grep -oE '[0-9]+ chamadas atendidas' "$log" | tail -1 | grep -oE '^[0-9]+')"
    echo "recusadas: $(grep -oE '[0-9]+ recusadas' "$log" | tail -1 | grep -oE '^[0-9]+')"
    echo "framebuffer: $(grep -E '^Framebuffer guest:' "$log" | tail -1)"
    echo "mmio_sem_modelo: $(grep -E 'fis:0x[0-9A-F]+ .*(não implementado|sem modelo)' "$log" |
        grep -oE 'fis:0x[0-9A-F]+' | sort -u | wc -l)"
    for addr in $vigiados; do
        grep -qE "fis:$addr .*(não implementado|sem modelo)" "$log" && echo "toca $addr: sem modelo"
    done
    echo "log: $log"
} | tee -a "$log"
