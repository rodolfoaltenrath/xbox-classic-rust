#!/usr/bin/env bash
# Build a pinned, external nxdk sample. No SDK sources or binaries are vendored.
set -euo pipefail
revision=14d5ee97e73347c973f1f57b68b79ec08c9e77f2
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
sdk=${NXDK_BUILD_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/obsidian/nxdk-14d5ee9}
output="$root/target/homebrew"
for tool in git make clang clang++ lld llvm-dlltool llvm-lib bison flex cmake gcc g++ python3 sha256sum; do
    command -v "$tool" >/dev/null || { echo "Missing build tool: $tool" >&2; exit 1; }
done
export LC_ALL=C TZ=UTC SOURCE_DATE_EPOCH=1704067200
if [[ ! -e "$sdk" ]]; then
    mkdir -p -- "$(dirname -- "$sdk")"
    git -c core.autocrlf=false clone --filter=blob:none --no-checkout https://github.com/XboxDev/nxdk.git "$sdk"
    git -C "$sdk" -c core.autocrlf=false checkout --detach "$revision"
fi
[[ $(git -C "$sdk" rev-parse HEAD) == "$revision" ]] || { echo "SDK must be at $revision" >&2; exit 1; }
git -C "$sdk" diff --quiet
git -C "$sdk" diff --cached --quiet
modules=(lib/pdclib lib/net/lwip lib/usb/libusbohci lib/zlib/zlib lib/libpng/libpng lib/libjpeg/libjpeg-turbo)
git -C "$sdk" submodule update --init -- "${modules[@]}"
for module in "${modules[@]}"; do
    git -C "$sdk/$module" diff --quiet
    git -C "$sdk/$module" diff --cached --quiet
done
mkdir -p -- "$output"
export NXDK_DIR="$sdk" PATH="$sdk/bin:$PATH"
{
    printf 'nxdk=%s\nSOURCE_DATE_EPOCH=%s\n' "$revision" "$SOURCE_DATE_EPOCH"
    git -C "$sdk" submodule status -- "${modules[@]}"
    clang --version
    lld-link --version
    make --version
    gcc --version
    uname -srvmo
} > "$output/build-manifest.txt"
# -B avoids reusing objects produced with other flags. nxdk builds its own libs.
make -B -C "$sdk/samples/hello" -j"${BUILD_JOBS:-4}" GEN_XISO= \
    LDFLAGS=-timestamp:0 \
    "CFLAGS=-ffile-prefix-map=$sdk=/nxdk" \
    "CXXFLAGS=-ffile-prefix-map=$sdk=/nxdk" > "$output/build.log" 2>&1
cp -- "$sdk/samples/hello/main.exe" "$output/hello.exe"
cp -- "$sdk/samples/hello/bin/default.xbe" "$output/hello-raw.xbe"
# cxbe ignores SOURCE_DATE_EPOCH and copies uninitialized padding after the NUL
# in its placeholder library names. Normalize only those metadata bytes.
# Code, section data, imports and TLS are unchanged. Keep the raw XBE for audit.
python3 - "$output" "$SOURCE_DATE_EPOCH" <<'PY'
import pathlib, struct, sys
output = pathlib.Path(sys.argv[1])
data = bytearray((output / "hello-raw.xbe").read_bytes())
assert data[:4] == b"XBEH"
base, = struct.unpack_from("<I", data, 0x104)
certificate, = struct.unpack_from("<I", data, 0x118)
certificate -= base
assert 0 <= certificate <= len(data) - 8
stamp = int(sys.argv[2])
struct.pack_into("<I", data, 0x114, stamp)
struct.pack_into("<I", data, certificate + 4, stamp)
count, address = struct.unpack_from("<II", data, 0x160)
assert count == 1  # pinned cxbe placeholder library, not an arbitrary input XBE
offset = address - base
assert 0 <= offset <= len(data) - 16
name = data[offset:offset + 8]
assert name[:6] == b"CXBE0\0"
data[offset + 6:offset + 8] = b"\0\0"
(output / "hello.xbe").write_bytes(data)
PY
(cd -- "$output" && sha256sum hello.exe hello.xbe > SHA256SUMS)
cat -- "$output/SHA256SUMS"
printf 'Artifacts: %s\n' "$output"
