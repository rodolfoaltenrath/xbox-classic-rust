# Enumeração do diretório guest

Consumidor próprio de teste, compilado pelo nxdk fixado. C somente no guest;
o recompilador, runtime e kernel permanecem Rust.

FindFirstFileA enumera D:\*, compara nome/atributos/tamanho com default.xbe
aberto por CreateFileA/GetFileSizeEx e exige ERROR_NO_MORE_FILES em FindNext.
Fecha a busca e reabre com *.xbe. Só então mostra FILE ENUM OK e o tamanho
real. Falhas mostram FILE ENUM FAILED. O loop conserva um diretório aberto e
reinicia a busca com NtQueryDirectoryFile entre esperas para testar snapshots.

No WSL, após preparar o SDK com scripts/build-homebrew.sh:

```bash
bash scripts/build-nxdk-sample.sh file-enum homebrew-file-enum
```

O build copia Makefile/main.c para um estágio novo em target. No Git Bash:

```bash
bash scripts/sonda.sh file-enum 128
bash scripts/run-alpha.sh file-enum --continuo
```

Este perfil enumera a mídia ImageFile declarada (um default.xbe). Não monta
partições de HD, outros arquivos do host, diretórios adicionais ou uma ISO.
Os samples WinAPI que montam C:/E: continuam bloqueados como no ADR-0091.
