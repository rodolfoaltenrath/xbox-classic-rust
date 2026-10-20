"""Generate ABI metadata from nxdk's CC0 export declarations, not implementations."""
import hashlib
import pathlib
import re
import sys

source = pathlib.Path(sys.argv[1])
output = pathlib.Path(__file__).resolve().parents[1] / "crates/xbox-kernel/src/imports/catalog.rs"
rows = []
declarations = source.read_text(encoding="utf-8")
assert hashlib.sha256(declarations.encode("utf-8")).hexdigest() == "8aa3e48417c8a60aea8d1953bcc0674cdee95ce9a11e5e861d3107c3339d2527", "source does not match the pinned nxdk revision"
for line in declarations.splitlines():
    match = re.fullmatch(r"\s*(\S+)\s+@\s+(\d+)\s+NONAME(\s+DATA)?\s*", line)
    if not match:
        continue
    symbol, ordinal, data = match.groups()
    name = symbol.lstrip("@").split("@")[0]
    if data:
        kind = "Kind::Data"
    elif "@" in symbol:
        size = int(symbol.rsplit("@", 1)[1])
        assert size % 4 == 0
        abi = "Fastcall" if symbol.startswith("@") else "Stdcall"
        kind = f"Kind::Function(Abi::{abi}({size}))"
    else:
        kind = "Kind::Function(Abi::Cdecl)"
    rows.append(f'        {ordinal} => ("{name}", {kind}),')
assert len(rows) == 371, f"unexpected nxdk export count: {len(rows)}"
output.parent.mkdir(parents=True, exist_ok=True)
output.write_text(
    "// ABI metadata generated from XboxDev/nxdk (CC0-1.0), Stefan Schmidt, 2017.\n"
    "// Revision: 14d5ee97e73347c973f1f57b68b79ec08c9e77f2. See docs/dev/homebrew.md.\n"
    "// Regenerate: python3 scripts/generate-kernel-catalog.py <nxdk>/lib/xboxkrnl/xboxkrnl.exe.def\n"
    "use super::{Abi, Export, Kind};\n\n"
    "#[allow(clippy::too_many_lines)] // Generated ordinal table, kept in numeric order.\n"
    "pub fn lookup(ordinal: u32) -> Option<Export> {\n"
    "    let (name, kind) = match ordinal {\n" + "\n".join(rows) +
    "\n        _ => return None,\n    };\n    Some(Export { ordinal, name, kind })\n}\n",
    encoding="utf-8",
)
