#!/usr/bin/env bash
# Compila OUTRO sample do nxdk já clonado por scripts/build-homebrew.sh (mesmo SDK fixado,
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
[[ -d "$sdk/samples/$sample" ]] || { echo "amostra inexistente: $sdk/samples/$sample" >&2; exit 2; }
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
export LC_ALL=C TZ=UTC SOURCE_DATE_EPOCH=1704067200 NXDK_DIR="$sdk" PATH="$sdk/bin:$PATH"
{
    printf 'nxdk=%s\nsample=%s\nSOURCE_DATE_EPOCH=%s\n' "$revision" "$sample" "$SOURCE_DATE_EPOCH"
    git -C "$sdk" submodule status -- "${modules[@]}"
} > "$output/build-$sample-manifest.txt"
make -B -C "$sdk/samples/$sample" -j"${BUILD_JOBS:-4}" GEN_XISO= LDFLAGS=-timestamp:0 \
    "CFLAGS=-ffile-prefix-map=$sdk=/nxdk" "CXXFLAGS=-ffile-prefix-map=$sdk=/nxdk" > "$output/build-$sample.log" 2>&1
cp -- "$sdk/samples/$sample/bin/default.xbe" "$output/$sample-raw.xbe"
cp -- "$sdk/samples/$sample/main.exe" "$output/$sample.exe"
(cd -- "$output" && sha256sum "$sample.exe" "$sample-raw.xbe" > "SHA256SUMS-$sample")
printf 'Artefatos: %s/%s-raw.xbe e %s.exe\n' "$output" "$sample" "$sample"
