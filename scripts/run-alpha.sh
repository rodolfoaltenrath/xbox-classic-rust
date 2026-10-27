#!/usr/bin/env bash
# Um comando por sample. Janela no Windows (Git Bash) e no Linux com servidor X (DISPLAY).
set -euo pipefail
usage() { echo 'uso: run-alpha.sh <hello|triangle|gamma-fade|file-read> [--xbe arquivo] [--projeto projeto] [--fatias N] [--sem-janela] [--continuo] [--carregar estado] [--salvar novo-estado]'; }
if [[ "${1:-}" == --help || "${1:-}" == --ajuda ]]; then usage; exit 0; fi
sample=${1:-}; shift || true
case "$sample" in hello) slices=64 ;; triangle) slices=320 ;; gamma-fade) slices=128 ;; file-read) slices=512 ;; *) usage >&2; exit 2 ;; esac
raiz="$(cd "$(dirname "$0")/.." && pwd -P)"
xbe=''; supplied=''; window=1; continuous=0; load_state=''; save_state=''; seen=' '
while [ "$#" -gt 0 ]; do
    option=$1; shift
    [[ "$seen" != *" $option "* ]] || { echo "opção repetida: $option" >&2; exit 2; }
    seen+="$option "
    case "$option" in
        --xbe|--projeto|--fatias|--carregar|--salvar)
            [ "$#" -gt 0 ] && [ -n "$1" ] || { echo "valor ausente: $option" >&2; exit 2; }
            [[ "$1" != --* ]] || { echo "valor ausente: $option" >&2; exit 2; }
            case "$option" in --xbe) xbe=$1 ;; --projeto) supplied=$1 ;; --fatias) slices=$1 ;; --carregar) load_state=$1 ;; --salvar) save_state=$1 ;; esac
            shift ;;
        --sem-janela) window=0 ;;
        --continuo) continuous=1 ;;
        --help|--ajuda) usage; exit 0 ;;
        *) echo "opção desconhecida: $option" >&2; exit 2 ;;
    esac
done
[[ "$slices" =~ ^[1-9][0-9]{0,8}$ ]] || { echo 'fatias deve estar em 1..999999999' >&2; exit 2; }
if [ "$continuous" -eq 1 ] && [ "$window" -ne 1 ]; then
    echo '--continuo exige janela; remova --sem-janela.' >&2; exit 2
fi
# Resolver caminhos do chamador antes de trocar para a raiz do repositório.
if [ -n "$xbe" ]; then
    [ -f "$xbe" ] || { echo "XBE inexistente: $xbe" >&2; exit 2; }
    xbe="$(cd "$(dirname "$xbe")" && pwd -P)/$(basename "$xbe")"
fi
if [ -n "$supplied" ]; then
    [ -d "$supplied" ] || { echo "projeto inexistente: $supplied" >&2; exit 2; }
    supplied=$(cd "$supplied" && pwd -P)
fi
if [ -n "$load_state" ]; then
    [ -f "$load_state" ] || { echo "estado inexistente: $load_state" >&2; exit 2; }
    load_state="$(cd "$(dirname "$load_state")" && pwd -P)/$(basename "$load_state")"
fi
if [ -n "$save_state" ]; then
    [ -d "$(dirname "$save_state")" ] && [ ! -e "$save_state" ] && [ ! -L "$save_state" ] || {
        echo "destino de estado deve ser novo, em diretório existente: $save_state" >&2; exit 2;
    }
    save_state="$(cd "$(dirname "$save_state")" && pwd -P)/$(basename "$save_state")"
fi
windows=0
case "$(uname -s)" in MINGW*|MSYS*|CYGWIN*) windows=1 ;; esac
if [ "$window" -eq 1 ] && [ "$windows" -eq 0 ] && [ -z "${DISPLAY:-}" ]; then
    echo 'Janela exige servidor X (DISPLAY vazio); use --sem-janela.' >&2; exit 2
fi
cd "$raiz"
echo "Preparo: traduzindo $sample com o emissor atual..."
if [ -n "$xbe" ]; then fresh=$(bash scripts/prepare-sample.sh "$sample" "$xbe"); else fresh=$(bash scripts/prepare-sample.sh "$sample"); fi
identity() {
    (
        cd "$1" || exit 2
        # Configurações/alvos extras não fazem parte do projeto gerado comparado.
        [ ! -e build.rs ] && [ ! -e .cargo ] && [ ! -e src/bin ] || exit 2
        for file in Cargo.toml Cargo.lock src/image.xbe src/generated.rs src/lib.rs src/main.rs; do
            [ -f "$file" ] || exit 2
        done
        sha256sum Cargo.toml Cargo.lock src/image.xbe src/generated.rs src/lib.rs src/main.rs | sha256sum | cut -d' ' -f1
    )
}
key=$(identity "$fresh")
state=${ALPHA_STATE_DIR:-"$raiz/target/alpha"}
mkdir -p "$state/cache" "$state/runs"
state=$(cd "$state" && pwd -P)
pointer="$state/cache/$sample-$key.project"
project=$fresh
invalid_cache=0
if [ -n "$supplied" ]; then
    if supplied_key=$(identity "$supplied") && [ "$supplied_key" == "$key" ]; then project=$supplied; else
        echo 'Projeto fornecido difere da tradução/lock atuais; nenhum arquivo alterado.' >&2; exit 2
    fi
elif [ -f "$pointer" ]; then
    cached=$(cat "$pointer")
    if cached_key=$(identity "$cached" 2>/dev/null) && [ "$cached_key" == "$key" ]; then
        project=$cached
    else
        invalid_cache=1
        echo "Cache inválido preservado: $pointer; usando projeto novo." >&2
    fi
fi
run=$(mktemp -d "$state/runs/$sample-XXXXXXXX")
if [ "$continuous" -eq 1 ] && [ -z "$save_state" ]; then save_state="$run/final.obsstate"; fi
target=${ALPHA_BUILD_DIR:-"$raiz/target/sample-build/$sample"}
printf '%s\n' "$project" >"$run/project.txt"
printf '%s\n' "$fresh" >"$run/prepared-project.txt"
printf '%s\n' "$key" >"$run/build-identity.txt"
echo "Build: projeto $project; log $run/build.log"
if cargo build --release --offline --manifest-path "$project/Cargo.toml" --target-dir "$target" >"$run/build.log" 2>&1; then :; else
    status=$?; echo "BUILD FALHOU: $run/build.log" >&2; tail -20 "$run/build.log" >&2; exit "$status"
fi
binary="$target/release/obsidian-native"
[ ! -f "$binary.exe" ] || binary="$binary.exe"
[ -f "$binary" ] || { echo 'BUILD FALHOU: executável ausente' >&2; exit 1; }
# Projetos são imutáveis para a ferramenta. Só trocar índice inválido após build
# aprovado, arquivando seus bytes e publicando um arquivo completo por rename.
if [ "$invalid_cache" -eq 1 ]; then
    archived=$(mktemp "$pointer.invalid-XXXXXXXX")
    cat "$pointer" >"$archived"
    replacement=$(mktemp "$state/cache/$sample-index-XXXXXXXX")
    printf '%s\n' "$project" >"$replacement"
    mv -- "$replacement" "$pointer"
elif [ ! -e "$pointer" ]; then
    if (set -o noclobber; printf '%s\n' "$project" >"$pointer"); then :; else
        echo "Índice de cache não gravado: $pointer; build e logs foram preservados." >&2
    fi
fi
echo "Execução: $sample, $slices fatias; logs em $run"
[ "$continuous" -eq 0 ] || echo "Modo contínuo: Espaço pausa; F5 salva em $run/states; F9 restaura. Estado final: $save_state"
status=0
if [ "$windows" -eq 1 ]; then
    args=(-NoProfile -ExecutionPolicy Bypass -File "$(cygpath -w "$raiz/scripts/start-alpha.ps1")"
          -Executable "$(cygpath -w "$binary")" -RunDirectory "$(cygpath -w "$run")" -Slices "$slices")
    [ "$window" -eq 1 ] || args+=(-Headless)
    [ "$continuous" -eq 0 ] || args+=(-Continuous)
    [ -z "$load_state" ] || args+=(-LoadState "$(cygpath -w "$load_state")")
    [ -z "$save_state" ] || args+=(-SaveState "$(cygpath -w "$save_state")")
    powershell.exe "${args[@]}" || status=$?
else
    args=(--fatias "$slices" --quadro "$run/guest.bgra" --quadro-apresentado "$run/display.bgra")
    [ "$window" -eq 0 ] || args+=(--janela)
    [ "$continuous" -eq 0 ] || args+=(--continuo --estados "$run/states")
    [ -z "$load_state" ] || args+=(--carregar "$load_state")
    [ -z "$save_state" ] || args+=(--salvar "$save_state")
    "$binary" "${args[@]}" >"$run/stdout.log" 2>"$run/stderr.log" || status=$?
fi
if [ "$status" -eq 1 ] && grep -qE '^Error: "?execução interrompida após [0-9]+ blocos;' "$run/stderr.log"; then
    echo "Orçamento alcançado (status 1); logs e quadro preservados em $run"
elif [ "$status" -ne 0 ]; then
    echo "EXECUÇÃO FALHOU: status $status; $run/stderr.log" >&2
    [ ! -f "$run/stderr.log" ] || tail -20 "$run/stderr.log" >&2
else
    echo "Execução encerrada (status 0); logs em $run"
fi
exit "$status"
