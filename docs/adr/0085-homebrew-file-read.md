# ADR-0085 — Homebrew de leitura do executável

Data: 2026-10-26. Estado: aceito. PLANO 7.3, provisório P22.

## Consumidor e build

O quarto alvo é fonte própria em homebrew/file-read, sob MIT. O C é código
guest compilado pelo nxdk para x86/XBE, como os outros consumidores. Kernel,
análise, runtime e serviços permanecem em Rust. Não é um porte do SDK ao host.
O guest usa CreateFileA, GetFileSizeEx, ReadFile e CloseHandle; ABI e wrappers
do nxdk fixado são as fontes do ADR-0083. Asserts no produtor exigem
OBJECT_ATTRIBUTES=12, IO_STATUS_BLOCK=8 e FILE_NETWORK_OPEN_INFORMATION=56.

build-nxdk-sample.sh copia Makefile/main.c para um diretório novo em target,
com o mesmo SDK/submódulos fixados e limpos, sem rede. SDK e fonte própria
ficam intactos. Manifest registra revisão, submódulos e hashes das fontes;
log e hashes do PE/XBE são preservados. Não há promessa de identidade entre
builds: o XBE exato deve ser conservado para retomar seus estados.

O produtor abre D:\default.xbe, verifica XBEH, lê até EOF, compara comprimento
com GetFileSizeEx e calcula FNV-1a32 de todos os bytes. Só então escreve
FILE READ OK, bytes e hash no framebuffer. Erros mostram FILE READ FAILED.
Reabre e mantém um handle entre blocos/esperas para exercitar snapshots.
Nenhum checksum, nome de título ou endereço é reconhecido no runtime.

## Fluxo e limites

prepare-sample, sonda e run-alpha aceitam file-read sem --entry manual.
Entrada padrão: target/homebrew-file-read/file-read-raw.xbe; 512 fatias.
verify-session conserva as provas integrais de pausa, avanço, F9, recusa de
restore inválido e outro processo. verify-window compara DC Win32 e BGRA.

O serviço atende somente default.xbe imutável, não arquivos arbitrários do
host ou uma ISO completa. P22 mantém limites síncronos/readonly/metadados;
P4 continua abrangendo VFS e mídia geral. Isso não amplia GPU, áudio,
controles ou compatibilidade de jogos comerciais. FNV é um diagnóstico
calculado pelo consumidor, não autenticação.

## Método e evidências

Testes de ferramentas usam cópias exatas em fixture Git isolada. Novos casos
cobrem o quarto alvo, separação do cache, fontes próprias sem resíduos,
manifest/hashes, SDK divergente/sujo e destino inválido. Não usam SDK real,
Cargo real nem estendem timeouts para esconder falhas.

Uma sonda hello iniciada antes da edição do script encontrou erro de sintaxe
ao retomar a leitura do Bash após compilar. bash -n e os testes passaram no
arquivo final; a execução foi repetida inteira com o script estável. A
tentativa interrompida não serve de aceite. O manifest do primeiro produtor
antecedeu a adição dos hashes de fonte ao script; uma conferência separada
registra igualdade entre fontes Git e estágio realmente compilado, sem
substituir o XBE ou fabricar um manifest de execução posterior.

Com 128 fatias, o produtor parou por orçamento durante o checksum: 109 chamadas,
zero recusas e framebuffer ainda preto. Isso não é prova de sucesso de I/O.
O orçamento padrão foi aumentado para 512 para chegar ao resultado calculado;
não foi alterado nenhum estado guest ou progresso de I/O para forçar o aceite.

A verificação de sessão expôs uma corrida no leitor de stderr: a regex podia
aceitar EIP parcialmente escrito e acusar CPU divergente, embora a linha
completa e o snapshot conservassem os valores. Wait-Log agora usa somente
linhas terminadas por newline. test-session-log.ps1 extrai a função real pela
AST e verifica três fluxos parciais (EIP, ciclos e segunda ocorrência). Não
foram removidas as comparações de CPU/Clock nem os hashes integrais do helper.

## Resultado do produtor

Build WSL com SDK 14d5ee97e73347c973f1f57b68b79ec08c9e77f2, saída preservada
em target/homebrew-file-read. O estágio source-UJ2MSXnG coincide byte a byte
com main.c e Makefile do Git (build-source-proof.txt em target/file-read-phase7).
Hashes SHA-256:

- XBE: 72580ecc0f37078d6990bc219d4440aa98ecdd59eb7264aa260b5be3fad6eb5e.
- PE: 7cec6a28b2a81638c38050e588857d6367f46090e005ac454a660f2dc2db0e16.
- main.c: 4a5f9a445f2987e15e7cbc8ce915ec3ba869afa15c268e62994e9df625ec131d.
- Makefile: 53e0e93bdfa30dfa83df72bbff8a9fb25c07f8530c9b2f56597d23b22129e1a6.

Tradução: 12982 blocos sem --entry. Inventário: 41/69 funções com variantes,
3/3 dados representados. Não confundir inventário com chamadas observadas.
Sonda final/512: 493 atendidas, zero recusadas, 19013552 instruções,
247885680106 ciclos/338025 ms sintéticos, 1810 pixels/39 linhas, zero MMIO
sem modelo. Log target/sonda/file-read-20261026-130946-K5t5oU7W.log.

Win32 final: visual-final/ em target/file-read-phase7, 640x480 e zero
diferenças RGB. A captura mostra FILE READ OK, bytes=102400 fnv=9C261314,
conferidos por cálculo independente dos bytes do XBE. Close: 82,5002 ms.
SHA-256 BGRA bruto/apresentado:
2530dc39b8b69d9d5f2294bfc511fe91e1ac652f886c27c3f825474532d8d79a.
O PNG capturado é idêntico ao primeiro teste visual/1024. Comandos:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/verify-window.ps1 -Executable target/sample-build/file-read/release/obsidian-native.exe -OutputDirectory target/file-read-window-check -Slices 512 -Presented -SaveFinalState -StartupTimeoutSeconds 300
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/verify-session.ps1 -Executable target/sample-build/file-read/release/obsidian-native.exe -OutputDirectory target/file-read-session-check -Sample file-read -TimeoutSeconds 300
```

Session-verified/: pausa em 19046504 instruções/648285712876 ciclos; avanço
para 19077364/1026685743564. F9 e regravação mantêm o arquivo inteiro com SHA
1c85413745306214320510709f1da11808924d4a6a7b8379526cd615047c381d.
Restore inválido conserva o estado, outro processo carrega e continua;
close com save pausado/executando em 770,2522/621,276 ms, zero recusas.
CDFS contém o XBE exato e handle 48, share=1, posição=38912.

Duas continuações headless de oito fatias, a partir desse mesmo checkpoint,
preservam mídia e avançam posição para 43008. replay-g/replay-h mantêm bytes
idênticos do estado completo, SHA-256:
94e05311c560cbab22a8a4dd5bc85c3d7e105521a9014c0b7d3072dc62f10c31.
As duas imagens também coincidem. CLI de reprodução (nomes novos para a
segunda tentativa, nunca sobrescrever):

```bash
target/sample-build/file-read/release/obsidian-native.exe --carregar checkpoint-000001.obsstate --fatias 8 --salvar replay.obsstate --quadro replay.bgra --quadro-apresentado replay-display.bgra
```

O launcher real passou em headless com projeto equivalente e no comando
file-read --continuo --carregar: rodada file-read-TPl4J4VX, host/status0,
767,5781 ms para fechar com final.obsstate, zero recusas. Artefatos em
target/file-read-phase7/public/runs; public-live-result.json registra o teste.
Não editar índices para simular cache válido.

Gate final: target/gate/20261026-131313-X96CyqRI.log, 607 testes. Rust
gerado: 269 casos debug e release com -D warnings (native-release.log).
Ferramentas: 26 Python/TOOLS OK (tools-verified.log) e 3 casos PowerShell/
SESSION LOG OK. Esses logs, mídia/replay e capturas estão em target/file-read-phase7.
Regressões dos outros três alvos são registradas na evidência de PLANO 7.3.
