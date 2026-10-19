# xbox-formats

Parsers dos formatos que o usuário traz: executáveis, imagens de disco e (em breve)
FATX e EEPROM. É o único crate que lê bytes de procedência desconhecida, e isso define
seu desenho.

## Duas regras não negociáveis

1. **Nenhum acesso direto a `&[u8]`.** Toda leitura passa por `bytes.rs`, que devolve
   `Err` com a posição e o tamanho pedidos. Indexar direto seria um `panic` a cada
   arquivo truncado.
2. **Nenhum `unwrap`/`expect` em caminho de parsing.** Arquivo malformado é erro
   tipado, sempre.

Essas regras são verificadas, não só declaradas: `tests/robustez.rs` trunca fixtures em
todos os pontos e aplica ~24 mil mutações determinísticas, exigindo que nada entre em
pânico. Em `fuzz/` há os alvos equivalentes para `cargo-fuzz` (nightly).

## `xbe` — executáveis

O XBE é um PE modificado com base em `0x00010000`. Três coisas merecem nota:

**Desofuscação por plausibilidade.** O entry point e o endereço da thunk table são
gravados XOR-ados com chaves diferentes para retail, debug e Chihiro. As chaves são
constantes publicadas, mas o parser testa as três e aceita a que produz endereços
**dentro da imagem**. Se nenhuma funcionar, o erro lista os três candidatos. Assim uma
constante errada aparece como diagnóstico em vez de corromper a execução silenciosamente
— e o `xbetool` mostra o valor cru ao lado do desofuscado, para conferência.

**Certificado de tamanho variável.** O certificado cresceu entre versões do SDK, então
só lemos campos que caibam no `size` declarado. Em XBE antigo, os campos ausentes vêm
zerados em vez de lerem dados de outra estrutura.

**Validação antecipada de seções.** O intervalo de cada seção no arquivo é conferido no
parse, não no uso. Quem chama `section_data` já sabe que a faixa existe.

**Logo.** RLE de nibbles em escala de cinza, 100×17. `xbetool --logo` desenha em ASCII,
o que serve de verificação visual do decodificador antes de a interface da Fase 8 usar
isso.

## `xdvdfs` — imagens de disco

Setores de 2048 bytes, descritor de volume no setor 32, diretórios como **árvore
binária** com ponteiros em unidades de 4 bytes.

**Base da partição.** Imagens cortadas têm a partição em 0; dumps completos, em
`0x18300000`. `open` detecta; `open_at` força uma base (útil para formato desconhecido e
para testar a aritmética sem alocar centenas de megabytes).

**Terminação garantida.** Os ponteiros da árvore vêm do arquivo, logo podem formar
ciclo. O caminhamento mantém um conjunto de deslocamentos visitados e limites de
entradas por diretório e de profundidade. Imagem maliciosa dá erro; nunca laço infinito.

**Empréstimo que atravessa o leitor.** `read` devolve `&'a [u8]` amarrado aos bytes da
imagem, não ao `Xdvdfs`. Por isso o `xbetool` pode extrair o `default.xbe` de uma
imagem, descartar o leitor e continuar usando a fatia — zero cópia e sem malabarismo
de lifetime no chamador.

## `fixtures` — arquivos de teste gerados por código

O ROADMAP §14 proíbe material proprietário no repositório, então as fixtures são
construídas em Rust. Vantagem lateral: um teste pode pedir exatamente o defeito que quer
(seção fora do arquivo, ciclo na árvore, imagem truncada).

**Limite honesto:** construtor e parser escritos juntos concordando entre si provam
consistência interna, **não** conformidade com o formato real. O que ainda precisa de um
arquivo de verdade está listado em [`docs/compat/formatos.md`](../compat/formatos.md).

## `media` — de onde vêm os bytes

A primeira versão dos parsers recebia `&[u8]` do arquivo inteiro, e isso só funcionava
para fixture: imagem de jogo tem de 2 a 7 GB. `Media` é fatia em memória **ou** arquivo
lido sob demanda, e `read_at` devolve `Cow` — `Borrowed` para fatia (zero cópia),
`Owned` para arquivo. Uma implementação do formato serve os dois casos, em vez de uma
versão "de teste" e outra "de produção" que divergem com o tempo.

Efeito medido: `isotool info` numa imagem de 2,87 GB passou a rodar em 0,4 s.

O `Mutex<File>` existe porque os métodos de leitura recebem `&self` e `Seek` exige
`&mut File`. Para leitura de setor isso é irrelevante; o caminho quente da Fase 4 vai
querer leitura posicionada de verdade, e isso é trabalho de `xbox-platform`.

## `fatx` — HD e memory units

FAT simplificado: superbloco de 4096 bytes, FAT a partir de `0x1000`, entradas de
diretório de 64 bytes com nome de 42 caracteres, numeração de cluster começando em 1 e
o cluster 1 sendo a raiz.

Dois pontos que exigiram decisão:

**A largura da entrada da FAT é circular.** Quantos clusters existem depende do tamanho
da FAT, que depende de quantos clusters existem. A documentação resolve estimando pelo
tamanho da partição e só então fixando 16 ou 32 bits — é o que fazemos, com o cálculo
repetido de propósito na fixture, para que divergência entre os dois apareça como teste
vermelho em vez de concordância por construção.

**Cadeia de clusters é lista ligada vinda do disco.** Pode ter ciclo. O caminhamento
mantém conjunto de visitados e limites de clusters, entradas e profundidade: partição
corrompida dá erro, nunca laço infinito. Coberto por fixture com ciclo e por 3 mil
mutações no teste de robustez.

Somente leitura. Escrita entra na Fase 9; até lá as partições graváveis são pasta do
host, via `vfs`.

## `eeprom` — configuração do console

256 bytes em três seções: hash e bloco cifrado, configuração de fábrica, configuração
do usuário. Lemos e escrevemos as duas últimas; o bloco cifrado é preservado byte a
byte e **não é decifrado** — precisaria de RC4, SHA-1 e HMAC, que são as funções `Xc*`
que o kernel HLE implementa na Fase 4.

`Eeprom::sintetica` gera uma EEPROM coerente com bloco em claro. Um console de verdade
recusaria; o nosso kernel não, e não existe caminho em que isso chegue a hardware. O
objetivo é que ninguém precise de dump de console para jogar.

**Esta é a parte menos verificada do crate.** Os deslocamentos vêm de documentação
pública e não passaram por dump real, ao contrário do XBE e do XDVDFS. O risco é
contido porque o único consumidor é o nosso próprio kernel: uma divergência aqui não
quebra jogo, só deixaria de detectar corrupção em dump de terceiro. Está registrado em
[`docs/compat/formatos.md`](../compat/formatos.md).

## `vfs` — as letras de drive

O jogo abre `D:\default.xbe` e `T:\save.bin`; esta camada transforma isso em leitura
numa imagem, numa partição ou numa pasta do host. É a interface que o
`NtCreateFile`/`NtReadFile` da Fase 4 consome.

**Partições graváveis são pasta do host**, por decisão de projeto. Escrever num FATX
emulado exigiria alocação de clusters, atualização de FAT e tolerância a falha no meio
da escrita — e o ganho seria nenhum: o jogo não sabe a diferença, e um save virado
arquivo é mais fácil de copiar e de recuperar.

**Caminhos de dispositivo** (`\Device\CdRom0\...`, `\??\D:\...`) são traduzidos para a
letra correspondente, porque é nessa forma que o kernel os recebe.

**Travessia de diretório é bloqueada em duas camadas.** Um título pedindo
`T:\..\..\..\Windows\System32\config` sairia da pasta montada e leria arquivo do
usuário. A resolução recusa qualquer componente `..`, recusa componente com `:` e,
depois de montar o caminho, confere que ele continua debaixo da raiz. A checagem dupla
é intencional: a primeira barra o ataque óbvio, a segunda cobre o que o sistema de
arquivos possa normalizar de forma inesperada. Há teste dedicado com cinco variantes de
ataque.
