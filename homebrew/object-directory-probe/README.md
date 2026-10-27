# Sonda de diretório de objetos do Xbox

Fonte guest própria, MIT. A ABI vem do nxdk fixado. Executar em Xbox ou ambiente
de referência cujo kernel tenha origem conhecida, e registrar a saída em vídeo/log.
Não usar os resultados do próprio Obsidian como evidência do comportamento esperado.

```bash
# No WSL, com SDK/toolchain já preparados:
bash scripts/build-nxdk-sample.sh object-directory-probe homebrew-object-directory-probe
```

O XBE fica em target/homebrew-object-directory-probe/object-directory-probe-raw.xbe.
Nenhum arquivo de console é lido pelo build, e os artefatos ficam fora do Git.

A sonda abre o diretório real de DOS devices pelo pseudo-handle -3 e enumera no
máximo 16 entradas com buffers de 256 bytes. Registra status, Context de entrada/
saída, ReturnLength, Length/MaximumLength, campo Type cru, nome e offset do buffer
da string, conferindo seus limites antes de ler. Depois mede buffers de tamanho
0/11/12/14 com RestartScan e fecha o handle. Não presume o conteúdo dos drives.
Cada registro fica visível por 1,5 s. Informar versão do kernel, origem da mídia,
hash do XBE e fonte da captura junto ao resultado.

O Obsidian ainda recusa NtOpenDirectoryObject/NtQueryDirectoryObject: esta sonda
prepara a obtenção da evidência faltante, não anuncia suporte às duas funções.
Não foi executada em hardware nesta sessão.
