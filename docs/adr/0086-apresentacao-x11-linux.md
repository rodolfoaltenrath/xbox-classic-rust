# ADR-0086 — Apresentação no Linux desktop pelo protocolo X11

Data: 2026-10-26. Estado: aceito. PLANO 8.1, provisório P6 (parte desktop).

## Contexto

A janela existia só no Win32 (ADRs 0052 e 0074). O mantenedor passou a
desenvolver no Fedora 44 (GNOME em Wayland, com Xwayland em `DISPLAY=:0`), e
nesta máquina `--janela` falhava com "Fases 8 e 10". O `Apresentador` já
isolava o sistema operacional: o `main` gerado só chama `criar_janela`,
`apresentar`, `eventos` e `definir_titulo`.

## Alternativas consideradas

1. **winit + softbuffer/wgpu.** É a direção do ROADMAP §4.6 para a Fase 8
   completa, mas exige crates que não estão no cache offline desta máquina e
   puxaria dezenas de dependências só para copiar 1,2 MB por quadro.
2. **libX11/libxcb por FFI.** Exige `unsafe`, cabeçalhos e a biblioteca de
   sistema; o crate já evita isso no Win32 declarando só o necessário.
3. **Protocolo X11 core direto no socket Unix (escolhida).** Rust puro, sem
   `unsafe` e sem dependência nova. Funciona em Xorg e em Xwayland, ou seja,
   também nas sessões Wayland comuns.
4. **Wayland nativo (wl_shm).** Igualmente possível sem dependência, mas exige
   xdg-shell e decorações do lado do cliente no GNOME; adiado até haver motivo
   além do que o Xwayland já cobre.

## Decisão

`crates/xbox-platform/src/x11.rs` abre `/tmp/.X11-unix/X<n>` (ou o nome
abstrato equivalente), autentica com o MIT-MAGIC-COOKIE-1 do `XAUTHORITY` (ou
`~/.Xauthority`) e usa apenas requisições do protocolo core: `CreateWindow`,
`ChangeProperty`, `MapWindow`, `CreateGC`, `PutImage`, `InternAtom`,
`GetKeyboardMapping` e `GetInputFocus`. A codificação pura vive em
`x11/protocolo.rs`, testada sem servidor.

- **Imagem:** exige o visual raiz TrueColor de 24 bits em 32 bpp com máscaras
  RGB888, o formato de praticamente todo servidor atual. Em LSBFirst os bytes
  `B,G,R,A` do guest são o próprio pixel X, então a cópia é direta. MSBFirst é
  reordenado. Outros visuais são recusados com o que o servidor ofereceu.
- **Tamanho de requisição:** sem BIG-REQUESTS, o máximo é 256 KiB e um quadro
  640×480 tem 1,2 MB. A imagem vai em faixas (102 linhas por PutImage no
  máximo usual), calculadas a partir do valor anunciado no setup.
- **Janela fixa:** `WM_NORMAL_HINTS` com mínimo igual ao máximo, como o estilo
  sem borda redimensionável do Win32, para o quadro não ser esticado.
- **Eventos:** a mesma política do Win32. `WM_DELETE_WINDOW` (ou
  `DestroyNotify`) gera `Evento::Fechar`, que é prioritário e permanente.
  Espaço/F5/F9 são lidos pelo mapa de teclado do servidor e entram numa fila
  limitada a 32. A autorrepetição é ignorada nas duas formas que o servidor
  produz (soltura e pressão com o mesmo instante, ou pressões sem soltura).
  `Expose` reenvia o último quadro, guardado no formato do servidor.
- **Título:** `_NET_WM_NAME` em UTF-8 e `WM_NAME` em Latin-1. Só é reenviado
  quando muda, porque o modo contínuo redefine o título a cada volta do laço.
- **Identificação:** `WM_CLASS = obsidian/Obsidian`, usado pela verificação
  externa.
- **Erros:** sem `DISPLAY`, display remoto (TCP), socket inexistente, setup
  recusado ou visual incompatível, a criação falha com o motivo. Uma ida e
  volta após `MapWindow` traz erros como BadMatch para a criação, em vez do
  primeiro quadro.

`xbox_platform::verificacao_x11` abre **outra** conexão, encontra a janela
pelo `WM_CLASS` (QueryTree recursivo), lê o conteúdo com `GetImage` e envia
`WM_DELETE_WINDOW` e teclas por `SendEvent`. Prova o que chegou ao servidor,
como o `verify-window.ps1` prova o DC do Win32.

## Evidência

- 21 testes novos de protocolo, sem servidor: DISPLAY, Xauthority (escolha e
  truncamento), setup aceito/recusado/truncado/visual incompatível, cada
  requisição, divisão de PutImage, conversão de pixels, extração de pacotes,
  mapa de teclado, atalhos/autorrepetição/prioridade do fechar, Expose e
  limite da fila. Mais três testes de erro do `Janela::nova_em`.
- Teste com servidor real, opt-in por `OBSIDIAN_X11_REAL=1` (o CI não tem
  display e pular em silêncio esconderia regressões). No Xwayland do Fedora
  44 o quadro de padrão XOR 640×480 voltou pelo GetImage com **zero pixels RGB
  diferentes**. Espaço, F5 e F9 chegaram na ordem e WM_DELETE_WINDOW virou
  `Fechar`.
- O primeiro GetImage logo após o mapeamento devolveu BadMatch. A
  especificação define esse erro para janela ainda não visível. A verificação
  repete a captura, em vez de tratar a janela como pronta.

## Validação com os samples (PLANO 8.2)

`tools/verificar-janela` executa o binário gerado e verifica a janela por outra
conexão. Fedora 44, Xwayland, binários release traduzidos com o emissor atual
(`target/samples/hello-KIVd2QrP`, `target/samples/triangle-Jn3L0uSF`):

| Prova | Hello | Triangle |
|---|---|---|
| Captura × `--quadro-apresentado` | 0 diferenças RGB | 0 diferenças RGB |
| Pixels/linhas na captura | 302/12 (igual à sonda) | 305922/479 (igual à sonda) |
| Fechar após o orçamento | 33,0 ms, status 1 | 18,6 ms, status 1 |
| Fechar durante a execução | 6,3 ms, status 0, parada pelo host | 31,0 ms, status 0, parada pelo host |
| `--continuo` com Espaço/Espaço/F5/F9 | pausa e retomada no mesmo contador de instruções; checkpoint salvo e restaurado; fechou em 20,6 ms, status 0 | — |

Depois do orçamento, o guest já parou: fechar só encerra o laço que mantém a
janela, e o processo sai com o status 1 do orçamento, cuja linha é preservada.
"Interrompida pelo host" aparece quando o fechamento interrompe a execução
(modos durante e contínuo). A captura do triangle mostra o triângulo RGB, o
fundo interpolado e o texto "Triangle Demo / Frames: 8", como a referência do
ADR-0069. As capturas ficam em `target/visual-linux-*/display.{bgra,ppm,png}`
junto com `guest.bgra`, os logs e o `resultado.txt` de cada modo.

`run-alpha.sh` passa a abrir a janela no Linux quando há `DISPLAY` (repassa
`--janela` e, no contínuo, `--estados` da rodada); sem `DISPLAY` continua
recusando antes do build.

## Consequências

P6 deixa de ser "apresentação só no Windows": o Linux desktop tem janela por
X11/Xwayland. Continuam pendentes Wayland nativo, DRM/KMS (Fase 10), display
remoto e visuais não RGB888. A validação com os samples reais é o PLANO 8.2.

## Como reverter

Remover o módulo `x11` e o `criar_janela` Linux devolve o erro "Fase 10". Nada
fora de `xbox-platform` depende do backend.
