# ADR-0093 — Consumidor próprio de enumeração de arquivos

Data: 2026-10-27. Estado: aceito. PLANO 11.2; P4/P22 permanecem abertos.

## Decisão

O teste próprio homebrew/file-enum usa os wrappers públicos do nxdk fixado:
FindFirstFileA, FindNextFileA e FindClose. A entrada é D:, já montada pelo CRT.
Não altera os samples que requerem C:/E: nem reconhece título no runtime.
O C é código guest; a implementação de kernel/recompilação continua Rust.

O consumidor exige nome default.xbe, atributo readonly e tamanho igual ao
retornado por CreateFileA/GetFileSizeEx sobre o próprio arquivo. Exige
ERROR_NO_MORE_FILES em FindNextFileA, fecha a busca e reabre com *.xbe.
Somente então escreve FILE ENUM OK e os dados conferidos no framebuffer;
qualquer divergência escreve FILE ENUM FAILED. Depois conserva um handle
de diretório, verifica o fim da busca e reinicia com NtQueryDirectoryFile
entre esperas. O filtro inicial deve sobreviver ao reinício/restauração.

Um assert no produtor confere FileName no offset 64 de
FILE_DIRECTORY_INFORMATION. As assinaturas e o consumidor do SDK estão
referenciados no ADR-0092. Não copiar a estrutura Unicode/11 argumentos do NT
desktop. Fonte do guest própria sob MIT, build isolado com o SDK/submódulos
fixados e limpos; manifest/hashes/estágio em target, sem baixar dependências.

prepare-sample, sonda e run-alpha aceitam file-enum. Entrada padrão:
target/homebrew-file-enum/file-enum-raw.xbe; orçamento 128. verify-session aceita
o mesmo sample e orçamento. As fontes e scripts ficam no Git; XBE/PE/estados
e imagens ficam ignorados, como nos demais consumidores.

## Reprodução

Depois de preparar o SDK público, no WSL:

```bash
bash scripts/build-nxdk-sample.sh file-enum homebrew-file-enum
```

No Git Bash do Windows:

```bash
bash scripts/sonda.sh file-enum 128
bash scripts/run-alpha.sh file-enum --continuo
```

Para o executável já compilado, com diretórios de saída novos:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/verify-window.ps1 -Executable target/sample-build/file-enum/release/obsidian-native.exe -OutputDirectory target/enum-window -Slices 128 -Presented -SaveFinalState -StartupTimeoutSeconds 300
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/verify-session.ps1 -Executable target/sample-build/file-enum/release/obsidian-native.exe -OutputDirectory target/enum-session -Sample file-enum -TimeoutSeconds 300
```

## Evidência de entrada e limites

13135 blocos sem --entry manual. XBE 102400 bytes, SHA-256
495be3273f4d9fdf20174dfd5bd5a7b91acda8efb2d1302271aeb06cb1b077cd.
PE SHA-256 34811ece1c1879b8a92fbca09c34607bf1b63296e3caa34d3710c8df9e878090.
Manifest/log/estágio em target/homebrew-file-enum, SDK
14d5ee97e73347c973f1f57b68b79ec08c9e77f2. Resultados finais de execução,
janela, replay e regressões estão na evidência de PLANO 11.2.

O recorte continua sendo o diretório do manifesto da mídia declarada, com
um único arquivo imutável. Não é enumeração de uma ISO completa, host FS,
disco rígido ou namespace geral de objetos. A recusa do ADR-0091 permanece;
este consumidor é independente desse bloqueio. CDFS v1 restaura sem ativar
o novo recurso, inclusive depois de regravar. Perfis/limites no ADR-0092/P22.

O gate no Windows também encontrou uma referência incorreta em um teste x87
recebido do outro computador. Foi corrigida só em #[cfg(test)], sem mudar o
runtime ou os limites (adendo ADR-0088). As sondas iniciadas antes dessa
correção exercitam o mesmo código de produção; o build final do consumidor
foi refeito pelo Cargo após ela, antes da prova visual.

## Resultados finais

- Gate Windows: 653 testes, target/gate/20261027-102930-iyaGb0Oi.log.
- Rust emitido: 273 casos debug/release com -D warnings; 29 testes Python de
  ferramentas/TOOLS OK (target/file-enum-phase11/{native-release,tools-final}.log).
- Sonda/128: 109 atendidas, zero recusas, 17788048 instruções e 26417788036
  ciclos (36024 ms sintéticos), 2692 pixels/51 linhas, zero MMIO sem modelo;
  target/sonda/file-enum-20261027-102715-De3Xwisy.log.
- Win32: 640x480, zero diferenças RGB entre captura e exportação; a imagem
  mostra FILE ENUM OK/name=default.xbe/bytes=102400 e ERROR_NO_MORE_FILES.
  Close 82,847 ms. Quadro BGRA bruto/apresentado SHA-256
  ecaaa9189cea09e216cc8340b1763c7684ec6d857ac74cbff0a1f0799e61f85b.
- Sessão: pausa em 18075469 instruções/114418075417 ciclos; avanço
  para 18299489/181884966073 antes de F9. Restaurar/regravar mantém o arquivo
  integral SHA-256 5dba321460b3bc4d852f13fbb3672c2b03cfd0e9e0ec98dcdedf5d63f00e0cc3.
  Falha de restore preserva estado e gerações; outro processo carrega e continua;
  close com save pausado/executando 527,6366/524,676 ms, zero recusas.
- Snapshot contém a mídia exata e um diretório: handle 36, share 1, filtro *.xbe,
  cursor esgotado. Duas continuações headless/8 fatias reproduzem fim/restart,
  quadro de sucesso e estado integral, SHA-256
  349cb4090ed587ccd73fb6022f0a99ad7b9c6e1fd13d833e42b7d2ff863a0a22.

Artefatos em target/file-enum-phase11/{visual,session,replay-g.*,replay-h.*,
directory-proof.json}. A inspeção de CDFS/DIRE confere igualdade da mídia antes
de ler o registro. O replay também exige que o quadro continue igual ao
diagnóstico de sucesso, não apenas que duas execuções falhem da mesma forma.

Launcher real: headless file-enum-Aep4sHS4 e contínuo/carregar
file-enum-9jfjF6ja, em public/runs daquela pasta. O segundo fecha por host/status0
em 590,7692 ms e publica final.obsstate, zero recusas. Build e execução são
etapas distintas; cache equivalente é conferido pelo launcher, sem edição do índice.

Regressões: hello/64 mantém 45 chamadas/zero e 302 pixels/12 linhas; file-read/512
mantém 493/zero e 1810/39. Triangle/320 mantém 278/zero, 305922/479 e
R4977/G4998/B4977, 9 draws/27 triângulos/4147200 fragmentos. Logs de 27/10:
hello-095630-K1GyPAov, file-read-095630-GXEHE2Tz e triangle-095630-5kzvCPav
em target/sonda (prefixo completo e números em PLANO 11.2).

As fontes main.c/Makefile coincidem com o estágio compilado (source-proof.txt).
Inventário do consumidor: 43/70 funções com variantes, 3/3 dados representados;
esse número não substitui as chamadas observadas. O HD permanece sem modelo.
