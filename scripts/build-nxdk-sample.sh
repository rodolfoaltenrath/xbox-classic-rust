#!/usr/bin/env bash
# Compila sample do SDK ou fonte própria em homebrew/, com nxdk preparado
# por scripts/build-homebrew.sh (mesmo SDK fixado,
# sem rede) e copia o .xbe e o .exe para target/<destino>/. Uso, no WSL:
#   bash scripts/build-nxdk-sample.sh triangle homebrew2
# O XBE sai como <amostra>-raw.xbe (sem a normalização de metadados que o hello recebe).
set -euo pipefail
revision=14d5ee97e73347c973f1f57b68b79ec08c9e77f2
sample=${1:?uso: build-nxdk-sample.sh <amostra> [destino em target/]}
destination=${2:-homebrew2}
[[ "$sample" =~ ^[a-zA-Z0-9_-]+$ ]] || { echo "nome de amostra inválido" >&2; exit 2; }
[[ "$destination" =~ ^[a-zA-Z0-9._/-]+$ && "$destination" != /* &&
   ! "$destination" =~ (^|/)\.\.?(/|$) ]] || { echo "destino deve ser relativo a target/, sem . ou .." >&2; exit 2; }
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
sdk=${NXDK_BUILD_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/obsidian/nxdk-14d5ee9}
source_dir="$sdk/samples/$sample"
if [ -d "$root/homebrew/$sample" ]; then source_dir="$root/homebrew/$sample"; fi
[[ -d "$source_dir" ]] || { echo "amostra inexistente: $source_dir" >&2; exit 2; }
[[ $(git -C "$sdk" rev-parse HEAD) == "$revision" ]] || { echo "SDK deve estar em $revision" >&2; exit 1; }
git -C "$sdk" diff --quiet
git -C "$sdk" diff --cached --quiet
# Não baixar ou atualizar nada aqui: a preparação pertence a build-homebrew.sh.
modules=(lib/pdclib lib/net/lwip lib/usb/libusbohci lib/zlib/zlib lib/libpng/libpng lib/libjpeg/libjpeg-turbo)
[[ -z $(git -C "$sdk" submodule status -- "${modules[@]}" | sed -n '/^[+-U]/p') ]] || {
    echo "submódulos ausentes ou fora da revisão fixada; execute build-homebrew.sh" >&2; exit 1;
}
for module in "${modules[@]}"; do
    git -C "$sdk/$module" diff --quiet
    git -C "$sdk/$module" diff --cached --quiet
done
output="$root/target/$destination"
mkdir -p -- "$output"
# Fontes próprias são copiadas para um build isolado; SDK e fontes Git ficam intactos.
if [ "$source_dir" = "$root/homebrew/$sample" ]; then
    stage=$(mktemp -d "$output/source-XXXXXXXX")
    cp -- "$source_dir/Makefile" "$source_dir/main.c" "$stage/"
    source_dir="$stage"
fi
export LC_ALL=C TZ=UTC SOURCE_DATE_EPOCH=1704067200 NXDK_DIR="$sdk" PATH="$sdk/bin:$PATH"
{
    printf 'nxdk=%s\nsample=%s\nSOURCE_DATE_EPOCH=%s\n' "$revision" "$sample" "$SOURCE_DATE_EPOCH"
    git -C "$sdk" submodule status -- "${modules[@]}"
    sha256sum "$source_dir/Makefile" "$source_dir/main.c"
} > "$output/build-$sample-manifest.txt"
make -B -C "$source_dir" -j"${BUILD_JOBS:-4}" GEN_XISO= LDFLAGS=-timestamp:0 \
    "CFLAGS=-ffile-prefix-map=$sdk=/nxdk" "CXXFLAGS=-ffile-prefix-map=$sdk=/nxdk" > "$output/build-$sample.log" 2>&1
cp -- "$source_dir/bin/default.xbe" "$output/$sample-raw.xbe"
cp -- "$source_dir/main.exe" "$output/$sample.exe"
(cd -- "$output" && sha256sum "$sample.exe" "$sample-raw.xbe" > "SHA256SUMS-$sample")
printf 'Artefatos: %s/%s-raw.xbe e %s.exe\n' "$output" "$sample" "$sample"
