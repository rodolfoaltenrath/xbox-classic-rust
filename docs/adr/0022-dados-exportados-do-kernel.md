# ADR-0022 — Região de dados exportados do kernel

Data: 2026-10-20. Estado: aceito para os três dados que o `hello` do nxdk importa.

## Contexto

O ADR-0021 deixou `prepare_aot` recusando qualquer dado exportado, e isso bloqueava a
tradução do alvo no ordinal 164 antes de qualquer execução. Os três dados que o
homebrew importa não são variações de uma mesma coisa: lidos nas declarações públicas
do nxdk (`__declspec(dllimport)`, entradas `DATA` no `.def`), eles têm **três formas
diferentes**.

| Export | Declaração | Forma |
|---|---|---|
| `LaunchDataPage` (164) | `PLAUNCH_DATA_PAGE LaunchDataPage;` | ponteiro de 4 bytes, escrito pelo título |
| `PsThreadObjectType` (259) | `OBJECT_TYPE PsThreadObjectType;` | struct de 28 bytes, só o endereço circula |
| `XeImageFileName` (326) | `ANSI_STRING XeImageFileName[1];` | struct de 8 bytes **mais** buffer com o caminho |

Preencher os três com zeros falharia de três maneiras distintas:

- `LaunchDataPage = 0` é o estado inicial **correto** (`XGetLaunchInfo` testa nulo e
  aloca a página), mas o slot precisa ser **gravável**: a rotina escreve o ponteiro de
  volta na variável;
- `PsThreadObjectType` zerado passa enquanto só a identidade do endereço importar, e
  falha em silêncio quando algum serviço seguir um ponteiro de procedimento;
- `XeImageFileName` com `Length = 0` **não** causa falta: o `strncpy` do pdclib deste
  SDK curto-circuita em `n` antes de desreferenciar `Buffer`, e `nxGetCurrentXbeNtPath`
  devolve caminho vazio. É resultado errado sem erro — o pior dos casos.

## Decisão

Instalar uma região de dados do kernel em memória guest, em **duas páginas**, porque a
permissão da MMU é por página de 4 KiB e o título escreve um dos três dados:

| Região | Permissão | Conteúdo |
|---|---|---|
| `0x7FFF0000` | somente leitura | `PsThreadObjectType`, `XeImageFileName`, buffer do caminho |
| `0x7FFF1000` | leitura e escrita | `LaunchDataPage` |

O slot do thunk de um dado recebe o **endereço do próprio símbolo**, não um trampolim:
com `dllimport`, o código compilado carrega o slot e desreferencia a partir do valor.
Funções continuam recebendo o trampolim do ADR-0021.

Os ponteiros de procedimento de `OBJECT_TYPE` recebem **endereços envenenados** numa
página nunca mapeada, um por campo, em vez de zero. Qualquer uso falha na hora, e o
endereço da falta diz qual campo foi seguido. `PoolTag` fica zero porque não é ponteiro
e inventar a etiqueta do console não acrescentaria nada verificável.

Dado que o catálogo conhece mas ninguém modelou continua recusando a imagem, com nome e
ordinal na mensagem. As duas páginas são reservadas na validação de layout sempre, mas
instaladas só quando a imagem importa algum dado.

## Consequências

- A tradução do alvo avança do ordinal 164 para o próximo bloqueio real, que é de ISA:
  `LOCK CMPXCHG` em `0x209C3`, **em tempo de tradução**, antes de qualquer execução.
- `XeImageFileName` expõe um caminho NT bem formado (`\Device\CdRom0\default.xbe`) que é
  **declarado, não derivado de mídia**: não há montagem ligando a VFS ao kernel ainda.
  Fica registrado como pendência em vez de virar adivinhação silenciosa.
- O relatório de ordinais passou a derivar os contadores do estado real, separando dado
  com representação de serviço implementado. O número anterior era literal e não media
  nada.
- Nada disto implementa serviço: `0/69` funções seguem pendentes. Resolver os dados
  destrava a próxima investigação, não a execução do homebrew.

## Como reverter

A região é instalada em um ponto só (`loader::data::install`) e consultada em um ponto
só (`imports::thunk_target`). Voltar a recusar dados exportados é remover o ramo
`Kind::Data` de `plan` e `thunk_target` — mas isso devolveria o bloqueio no ordinal 164.
