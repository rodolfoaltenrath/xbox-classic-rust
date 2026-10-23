# ADR-0074 — Evidência visual e encerramento do alpha Win32

Data: 2026-10-23. Estado: aceito para PLANO 4.2. Base `319dbea`.

## Decisão

O triangle já produz pixels na RAM, e o hello foi reexecutado no Windows
(ADR-0073). Este recorte verifica o caminho framebuffer → Quadro → janela sem
acrescentar dispositivo, shader ou instrução guest.

O main emitido mantém o Desfecho da sessão. ParadoPeloHost encerra sem entrar
na espera de janela congelada; retorno e erro/orçamento mantêm a política
anterior de apresentar o estado final até fechar. Não executar novamente o
guest após WM_CLOSE. Processamento da janela permanece nas fronteiras da
sessão; o sleep host de 30 ms só repinta depois do fim, sem avançar Clock guest.

O executável aceita `--janela`, `--fatias N` e `--quadro caminho`. O parser
valida todos os argumentos antes da sessão, incluindo desconhecidos após
--fatias, repetidos, ausentes, zero e overflow. --quadro grava os bytes BGRA
visíveis do framebuffer final em arquivo novo, sem padding e sem sobrescrever
arquivo existente. É diagnóstico opcional, não snapshot ou imagem substituta.
Falha de exportação é relatada; não pode esconder HLT, falta de memória ou
recusa do kernel que causaram a parada. Se a execução teve sucesso, erro de
exportação também faz o processo falhar.

## Biblioteca AOT e CLI separadas

Projetos novos têm src/lib.rs, que exporta run do generated.rs, e main separado
que importa obsidian_native. Não muda operações x86, ABI, imagens ou snapshots.
Isso permite corrigir a CLI sem recompilar/otimizar novamente milhares de
blocos na biblioteca, desde que a entrada e a biblioteca não tenham mudado.
Não se presume isso para traduções antigas: regenerar após mudança do emissor.
O preparo registra hashes dos três fontes e estado dirty/clean do código
rastreado, além da imagem e toolchain, para não confundir revisão base com
artefato construído de alterações locais.

Quatro testes no Rust gerado cobrem o parser; o main real de uma fixture de
vídeo exporta dois pixels conhecidos no arquivo completo, incluindo caminho
com espaços, recusa de sobrescrita e opção desconhecida após --fatias.
HLT e INT3 são executados com --quadro para garantir que a indisponibilidade
de framebuffer não apague seu diagnóstico. Suites anteriores de CPU, GPU,
threads e snapshot continuam obrigatórias.

## Verificação pelo sistema

`scripts/verify-window.ps1` recebe executável já compilado e diretório novo.
Compilação fica fora dos deadlines de execução. A ferramenta abre a aplicação
interativa, espera a janela e o modo final congelado, captura sua área cliente
640×480 pelo DC/BitBlt e compara todos os canais RGB com o arquivo guest.bgra.
Alfa é ignorado: GDI não preserva a semântica desse byte do guest. A captura
PNG vem do sistema, nunca dos bytes esperados. Dimensão, leitura impossível,
diferença de RGB ou ausência de captura falham sem substituir a imagem.

Um segundo modo, -DuringExecution com orçamento longo, fecha na primeira
janela disponível, exige término por host com status zero em até cinco
segundos e recusa o log de janela congelada. O modo final também verifica
WM_CLOSE após o orçamento, com status 1/diagnóstico de orçamento preservado.
Timeout limpa somente o processo iniciado pela ferramenta e preserva logs.
As medições, hashes, estatísticas de cor e close_ms ficam em result.json.

Referências das APIs host:

- [Microsoft BitBlt](https://learn.microsoft.com/en-us/windows/win32/api/wingdi/nf-wingdi-bitblt):
  captura do retângulo de cor do DC da janela via SRCCOPY.
- [Microsoft BitmapData](https://learn.microsoft.com/dotnet/api/system.drawing.imaging.bitmapdata):
  acesso por Scan0/Stride e leitura das linhas sem assumir padding/orientação.
- [Microsoft PostMessageW](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-postmessagew):
  WM_CLOSE enfileirado no handle específico, sem bloquear esperando atendimento.
- [Microsoft ShowWindow](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-showwindow):
  a primeira chamada pode usar STARTUPINFO. Um protótipo lançado como Hidden
  ficou oculto apesar de ShowWindow no apresentador; isso não era falha do guest.
  A verificação da aplicação interativa usa lançamento visível; não mudar o
  comportamento de janela apenas para esconder o console de um teste.

## Limites preservados

Quadro possui a cópia dos bytes: a ferramenta e o apresentador não recebem
ponteiro ou referência à arena guest. Não há worker de GPU, relógio host no
kernel, unsafe Rust novo ou versão de snapshot alterada. C#/PowerShell é
somente automação externa da verificação Win32; runtime/recompilador continuam Rust.

A comparação RGB prova correspondência com a RAM daquele ponto. P2 continua
aberto para gamma/filtros; P6 continua aberto para Linux; P18 conserva o raster
software parcial, sem textura/blend e sem comparação física com NV2A. Não é
evidência de fidelidade analógica, jogos comerciais ou compatibilidade gráfica geral.

## Evidências finais no Windows

Gate: **580 testes**, `target/gate/20261023-154903-qFHAXziu.log`.
**257 casos gerados debug/release** (241 fora de x86_64); release compilado com
-D warnings, `target/window-native-release-final.log`. Suites de snapshot e
replay continuam passando, sem mudança de formato.

| Aplicação | Cliente/RGB divergente | Pixels/linhas | WM_CLOSE após orçamento | WM_CLOSE durante execução |
|---|---|---|---|---|
| hello, 64 fatias | 640×480 / **0** | 302/12 | 95,7878 ms, status 1/orçamento | 191,1977 ms, status 0/host |
| triangle, 320 fatias | 640×480 / **0** | 305922/479; R4977/G4998/B4977 | 52,2096 ms, status 1/orçamento | 173,9636 ms, status 0/host |

Relatórios, captura PNG e BGRA: `target/visual-hello-verified/`,
`target/visual-triangle-verified/`, `target/visual-hello-during/` e
`target/visual-triangle-during/`. Durante execução usou 1000000 fatias e a
janela foi fechada antes de entrar no modo congelado. Não houve retomada nem
mensagem de janela mantida aberta nesses dois casos. Capturas foram inspecionadas:
Hello nxdk! e triângulo RGB com fundo/texto Triangle Demo/Frames.

SHA-256 BGRA final: hello
`936b66e23452af9287bf267806225de155205de0ff4ffd0e795d065e9550f0d7`, triangle
`76ac9e08b6e8e7a2aeaf286d5f9e60aabd2b51c9580a0cdf025b182f9089f132`.
Sondas sem janela no código final mantiveram as contagens do ADR-0073:
`target/sonda/hello-20261023-160356-rJQpEReD.log` e
`target/sonda/triangle-20261023-160449-hr2Dp29o.log`.

Projetos locais: hello-NBgtYtJT e triangle-8OqkeisD em target/samples. A CLI
final foi sincronizada com o template preservando main-initial.rs.txt;
a biblioteca gerada ficou intacta. Builds frios: 9m42/17m59. Rebuild só da
CLI final: 8,82 s/3,86 s, logs target/window-{hello,triangle}-cli-build.log.
Não atribuir essas durações à velocidade da CPU guest ou comparar hashes de
entrada distintos como se fossem o mesmo binário.

Reprodução (Git Bash, pasta de saída de captura sempre nova):

```bash
project=$(bash scripts/prepare-sample.sh triangle)
cargo build --release --offline --manifest-path "$project/Cargo.toml" --target-dir target/sample-build/triangle
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/verify-window.ps1 \
  -Executable target/sample-build/triangle/release/obsidian-native.exe \
  -OutputDirectory target/minha-captura-triangle -Slices 320
# Repetir com outra pasta, -Slices 1000000 -DuringExecution para fechar durante run.
# Para hello: prepare-sample.sh hello, cache /hello e -Slices 64.
```

Correções de método da ferramenta: leitura de stderr vivo exige FileShare.ReadWrite;
ReadAllText com o compartilhamento padrão falhou. O Process do Windows PowerShell
precisa manter Handle desde o início para obter ExitCode após sair; código ausente
é erro, nunca zero implícito. As tentativas falhas ficaram em target/visual-hello-final
e target/visual-hello-checked. Copy-Item preserva mtime: ao sincronizar a CLI de uma
sonda existente, atualizar o timestamp e conferir o log de recompilação; a primeira
tentativa (0,18 s sem compilar) não foi usada como evidência de rebuild. O fluxo
normal de prepare-sample.sh cria arquivos novos e não depende dessa sincronização.
