# Guest de teste de leitura

Fonte própria para o nxdk público, separada do runtime Rust. Usa somente
CreateFileA, GetFileSizeEx, ReadFile e CloseHandle, pela implementação do SDK.
Abre D:\default.xbe, exige XBEH, lê até EOF e mostra comprimento e FNV-1a32
calculados. Falhas mostram FILE READ FAILED. Depois mantém um handle aberto
e lê blocos entre esperas, para testar posição e retomada em snapshots.

Com o SDK preparado por scripts/build-homebrew.sh, no WSL:

```bash
bash scripts/build-nxdk-sample.sh file-read homebrew-file-read
```

O script fixa a mesma revisão do nxdk, copia Makefile/main.c para uma pasta
nova em target e grava manifest, hashes e log. Não deixa objetos compilados
nesta pasta. O resultado é target/homebrew-file-read/file-read-raw.xbe.
Execute no Git Bash do Windows:

```bash
bash scripts/run-alpha.sh file-read --continuo
bash scripts/sonda.sh file-read 512
```

Para conferir o diagnóstico contra o XBE local, no WSL, raiz do repositório:

```bash
python3 - <<'PY'
from pathlib import Path
data = Path('target/homebrew-file-read/file-read-raw.xbe').read_bytes()
value = 2166136261
for byte in data:
    value = ((value ^ byte) * 16777619) & 0xffffffff
print(f'bytes={len(data)} fnv={value:08X}')
PY
```

O kernel publica somente os bytes originais do executável, não o filesystem
do computador. Não prova suporte a arquivos adicionais, diretórios, escrita,
ISO ou I/O assíncrona. FNV é diagnóstico, não autenticação. O valor esperado
depende do XBE exato deste build, nunca de um valor fixo no runtime.
