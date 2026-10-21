# ADR-0052 — Apresentação do framebuffer em janela

Data: 2026-10-21. Estado: aceito para Windows; Linux pendente.

## Contexto

Depois do ADR-0051 o hello executa seu `main` e o framebuffer guest contém "Hello nxdk!"
legível — mas só como bytes na RAM e arte ASCII no terminal. A entrega 6 do alpha exige
*apresentar* a imagem. Este ADR registra como, e o que a imagem apresentada **não** é.

## Decisão

### Fronteira em `xbox-platform`, sem `windows-sys`

`display::Apresentador` (`apresentar(&Quadro)`, `eventos() -> Evento`) é a fronteira; a
implementação Win32 vive em `windows.rs`, atrás de `criar_janela`. Fora do Windows,
`criar_janela` devolve `NotImplemented` citando as Fases 8 e 10 — não cai num
apresentador que "apresenta" para lugar nenhum, que seria pior que um aviso.

As funções do Win32 são declaradas à mão (`user32`, `gdi32`, `kernel32`), como já faz a
arena (ADR-0008): o toolchain `windows-gnu` do host não linka `windows-sys`, mas
`libuser32.a` e `libgdi32.a` vêm no MinGW empacotado. **Nenhuma dependência nova.**

### `Quadro` possui seus bytes

A arena do guest é `Send`, não `Sync` (ADR-0011), então nenhuma referência à RAM guest
atravessa para código de janela. `FramebufferView::copiar_visivel` (nova, construída
sobre o `row` existente — herda sua validação de limites, não tem segunda leitura com
regra própria) copia as 480 linhas **sem o padding de pitch** num `Vec`. Custo: 1,2 MB
por quadro. O apresentador não sabe o que é pitch, arena ou endereço físico.

### Política: apresentar quando o guest cede a vez

Apresenta-se depois de um serviço de kernel que **avançou o relógio guest** (uma espera),
e como rede de segurança no fim de cada fatia de orçamento. Detectar pelo Clock — e não
pelo serviço — mantém a política independente de quais esperas existam.

Uma premissa minha foi refutada aqui e fica registrada: eu esperava que o fim de fatia
fosse o ritmo natural. **Não é, neste alvo**: houve 17 fins de fatia, todos antes do modo
de vídeo, que só é configurado a 98,6% da execução (instrução 17.213.022 de 17.452.569).
Apresentar só em fim de fatia nunca mostraria a imagem. O ponto certo é a espera.

### Fechar a janela interrompe a execução

`WM_CLOSE` não destrói a janela: marca o pedido, e `eventos()` devolve `Evento::Fechar`.
O laço então encerra de forma controlada (`Ok`, não erro). Isso entrega, de passagem, a
interrupção controlada **pelo host** que a entrega 5 pedia.

Com `--janela`, depois do fim da execução a janela é mantida aberta, repintando a cada
30 ms (~3% de CPU medidos), até o usuário fechar: sem isso o orçamento do `main` gerado
acaba em ~2 s e a imagem sumiria antes de alguém vê-la. Sem `--janela` o comportamento é
idêntico ao anterior, e isso foi verificado (mesmos 1208 pixels, 17.452.569 instruções).

## O que a imagem apresentada NÃO é — dívida provisória declarada

É a cópia fiel dos bytes `B,G,R,A` que o guest escreveu. **Não é** a saída analógica do
console. Três efeitos reais não são aplicados:

| Efeito | Estado | Condição de saída |
|---|---|---|
| rampa de gamma do DAC | escrita em MMIO, só **registrada** pela telemetria (ADR-0051) | modelar o DAC (índice com autoincremento, 3 componentes, snapshot) e aplicá-lo na apresentação |
| filtro de flicker / soften | só pedidos lógicos registrados (ADR-0050) | decidir se e como o filtro entra na apresentação |
| entrelaçamento, overscan, proporção de pixel de TV | não modelados | decisão de produto, junto da Fase 8 |

**Isto é provisório e não deve ser promovido a definitivo por omissão.** Um título que use
a rampa de gamma para fade — comum — vai aparecer diferente aqui, e ninguém deve tomar a
janela por "a saída do console". O registro está também na documentação do módulo
`display`, onde quem for usar a API a vê.

O hello não consome a rampa: ele a configura com identidade (`i, i, i`) e nunca a lê de
volta, então a imagem dele não muda. Isso é uma propriedade **deste** título, não da
apresentação.

## Verificação

- **A imagem na janela é a imagem na RAM**, capturada de fora do processo pelo próprio
  sistema (`PrintWindow`): área cliente 640×480 (1:1), **1208 pixels não-pretos em
  x=26..119, y=26..88**, idêntico ao relatório do framebuffer guest. A captura mostra
  "Hello nxdk!" quatro vezes, branco sobre preto.
- `WM_CLOSE` enviado de fora (`CloseMainWindow`, como o botão X) encerra o processo em
  menos de 5 s.
- O sistema vê a janela do processo (`MainWindowHandle`, título "Obsidian — Xbox") e o
  laço de repintura gasta ~0,06 s de CPU em 2 s.
- 13 testes em `xbox-platform` (9 do módulo de display, 5 Win32 que abrem **janelas
  reais**) e 4 em `xbox-kernel` para `copiar_visivel`. O teste Win32 mais forte lê os
  quatro quadrantes de volta do DC da janela e confere orientação e ordem de canais.
  437 testes de workspace, estáveis em três rodadas consecutivas.

## O que os testes automáticos NÃO cobrem

- **Sem sessão gráfica**, `ler_pixel` devolve `None` e o teste de quadrantes **avisa e
  passa sem verificar** — uma lacuna real em CI sem display. Neste host a verificação
  executou (não houve o aviso). Os demais testes Win32 (criar/soltar, fechar, escalar)
  ainda exigem poder criar janela.
- **Usuário fechando a janela *durante* a execução**: o caminho existe e retorna `Ok`, mas
  o hello termina antes de haver como fechá-la, e a lógica vive no template gerado, que é
  texto. Só foi exercitado o fechamento **depois** do fim. Registrado como lacuna.
- **Linux**: `xbox-platform` compila e passa no `clippy -D warnings` para
  `x86_64-unknown-linux-gnu` (target instalado só para isto), mas os testes não foram
  **executados** lá. Não existe janela Linux.
- O fundo da janela antes do primeiro quadro é indefinido (`WM_ERASEBKGND` devolve 1 para
  evitar flash a cada quadro). A janela só é criada quando há um quadro, então não aparece.

## Erros de método, registrados

1. Concluí "janela travada" ao ver o processo a 100% de CPU, 16 s depois de lançá-lo. Era
   o **build debug**: 38,5 s contra 1,85 s em release (21×). Meu "~1 s" anterior era o
   release, nunca cronometrado. Diagnóstico por amostragem de CPU e de estado de threads
   **não distingue** "girando em laço" de "trabalhando devagar" — só o tempo da execução
   sem janela distinguiu.
2. O teste de quadrantes falhou porque a janela de 64 px de cliente sai com 130 px: o
   Windows impõe largura mínima por causa da barra de título, e o quadro era escalado ~2×.
   Não era espelhamento. O teste agora usa 640×480 e **afirma** a premissa (cliente 1:1)
   em vez de assumi-la.
3. Um `sed` que não casou (`\n` literal dentro da string) deixou `let platform` declarado e
   sem uso, e o `Cargo.toml` gerado sem a dependência — sem erro nenhum. Só a conferência
   com `grep -n` mostrou.
4. O teste `fora_do_windows...` nunca compilou no Windows (`cfg(not(windows))`) e tinha um
   erro de tipo (`unwrap_err` exige `Debug`). Só apareceu depois de instalar o target
   Linux para checar.

## Como reverter

A janela é opt-in (`--janela`); sem ela o `main` gerado se comporta como antes. Remover
`xbox-platform` das dependências geradas e o braço de apresentação do template devolve o
estado do ADR-0051. `copiar_visivel` e as constantes de dimensão são aditivas e
independentes.
