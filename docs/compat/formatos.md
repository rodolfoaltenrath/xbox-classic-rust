# Conformidade dos formatos

As fixtures do projeto são geradas por código (ROADMAP §14), o que prova que parser e
construtor concordam, **não** que o parser lê arquivos reais corretamente. Esta página
registra a verificação contra arquivo real.

## Verificação de 2026-10-19 — disco de varejo NTSC-U

Mídia do mantenedor, imagem cortada de 2.871.951.360 bytes, 185 arquivos, 16
diretórios. **Nenhum byte dela entrou no repositório** (`*.iso` está no `.gitignore`);
só metadados estruturais foram lidos, e nada de código de biblioteca da Microsoft foi
examinado — ver [Limite deliberado](#limite-deliberado) abaixo.

O que o executável principal declarou:

| Campo | Valor |
|---|---|
| título | `NFS Underground 2` |
| title ID | `EA-005A` (0x4541005A) |
| build | retail |
| tamanho do `.xbe` | 4.038.656 bytes, setor 28560 |
| entry point | 0x0021B1CE (cru 0xA8DDE665) |
| thunk table | 0x0033F800 (cru 0x5B5EB8B6) |
| seções | 14, incluindo `D3D`, `XGRPH`, `DSOUND`, `XONLINE`, `DOLBY`, `XPP` |
| bibliotecas | 8, todas 1.0.5849 |
| importações do kernel | **152 ordinais** |
| caminho de build | `D:\Speed\XBOX\obj\UG2Release\nfsUG2Release.exe` |

### XBE

| # | Item | Resultado |
|---|---|---|
| X1 | Chaves XOR retail | ✅ **fechado.** O entry point caiu dentro de `.text` e o thunk em **0x0033F800, que é exatamente o início de `.rdata`** — é lá que a thunk table mora. Chave certa. |
| X2 | Chaves XOR de debug e Chihiro | ⬜ aberto — este disco é retail; precisa de um XBE de dev kit |
| X3 | Header além de `0x178` | ✅ **fechado.** `size_of_image_header = 0x184`: existem 3 palavras extras. Agora são guardadas cruas em `Header::extended` em vez de ignoradas |
| X4 | Tamanhos reais de certificado | ✅ **fechado.** 0x1EC (492 bytes), bem maior que os 0xD0 da nossa fixture. A leitura condicional por `size` aguentou |
| X5 | Layout de bits do RLE do logo | ✅ **fechado.** `xbetool --logo` desenhou o logo do Xbox legível, não ruído. 690 bytes de RLE para 1700 pixels |
| X6 | TLS fora da região de headers | ✅ **fechado — e era um bug.** O diretório de TLS está em 0x00361990, dentro de `.rdata`. O parser devolvia `None` e perdia o endereço; agora resolve VA por headers **e** por seções |
| X7 | Nome de biblioteca com 8 caracteres exatos | ✅ **fechado.** `XONLINES` tem 8 caracteres e nenhum NUL; foi lido inteiro |

### XDVDFS

| # | Item | Resultado |
|---|---|---|
| D1 | Base `0x18300000` em dump completo | ⬜ aberto — esta imagem é cortada (base 0); precisa de um dump completo |
| D2 | Collation da árvore binária | ✅ **fechado na prática.** 201 entradas em 16 diretórios, todas alcançadas pelo caminhamento, incluindo diretórios aninhados (`pveh/COMP/`) |
| D3 | Preenchimento de fim de setor | ✅ **fechado.** O setor da raiz termina com 0xFF a partir do byte 0x80; nosso tratamento de `nome_len` 0xFF está certo |
| D4 | Nomes com bytes acima de 0x7F | ⬜ aberto — todos os 185 nomes deste disco são ASCII |
| D5 | **Ponteiro vazio como `0`** (item novo) | ✅ **fechado.** Imagens reais usam `0`, não `0xFFFF`, para "sem filho": todos os `left` da raiz são 0. Nosso tratamento de `0` como "sem filho" estava correto |
| D6 | **Escala** (item novo) | ✅ **fechado.** `isotool info` roda em **0,4 s** numa imagem de 2,87 GB porque agora lê sob demanda; antes, carregava o arquivo inteiro na RAM |

## Itens ainda abertos

| # | Falta | Como fechar |
|---|---|---|
| X2 | Chaves de debug/Chihiro | XBE compilado com nxdk (debug) ou dump de placa Chihiro |
| D1 | Base de dump completo | Imagem de disco não cortada |
| D4 | Nomes não-ASCII | Disco japonês ou europeu com acentos |
| X8 | Significado das 3 palavras extras do header | Documentação pública ou comparação entre vários XBEs |

## Como reproduzir esta verificação

```sh
# Estrutura e inventário da imagem, sem carregá-la na RAM
isotool <imagem> info
isotool <imagem> tree

# Executável principal: extrai da imagem e descreve
xbetool <imagem>
xbetool <imagem> --logo      # validação visual do decodificador de RLE
xbetool <imagem> --thunks    # ordinais do kernel que o título precisa
```

## Limite deliberado

As seções `D3D`, `XGRPH`, `DSOUND` e `XONLINE` do executável **são código da
Microsoft** linkado dentro do jogo. Lemos os *metadados* delas (nome, tamanho,
permissões) e a lista de ordinais importados, porque isso é estrutura de arquivo.

Não desmontamos, não analisamos e não derivamos comportamento desse código. O ADR-0001
afirma que o kernel HLE é reimplementação limpa, e essa afirmação depende de a
implementação vir de documentação pública e de comportamento observável — não de ler o
binário da Microsoft. Examinar aquelas seções traria risco jurídico sem trazer
informação que a documentação pública já não dê.

## FATX, EEPROM e VFS — ainda sem verificação real

Estes três foram escritos depois da verificação de 2026-10-19 e **não** passaram por
arquivo real: não temos imagem de HD de Xbox nem dump de EEPROM. Validados apenas
contra fixture gerada por código, o que prova consistência interna.

| # | O que verificar | Por que importa | Status |
|---|---|---|---|
| F1 | Superbloco de um HD real (cluster, cópias de FAT) | Nossa fórmula de largura da FAT é uma leitura da documentação | ⬜ aberto |
| F2 | Deslocamentos de `PARTICOES_DO_HD` | Montar a partição errada dá "arquivo não encontrado" sem explicação | ⬜ aberto |
| F3 | Formato de data/hora do FATX | Hoje guardamos o `u32` cru sem interpretar | ⬜ aberto |
| E1 | Deslocamentos da EEPROM (fábrica e usuário) | Idioma/vídeo lidos do lugar errado dariam configuração silenciosamente errada | ⬜ aberto |
| E2 | Fórmula dos dois checksums | Só detectaria corrupção em dump de terceiro; nossa EEPROM é autoconsistente | ⬜ aberto |
| V1 | Correspondência partição ↔ letra em `ALIASES_DE_DISPOSITIVO` | O kernel recebe caminhos `\Device\Harddisk0\PartitionN` | ⬜ aberto |

Nenhum deles bloqueia a Fase 4 para rodar jogo de disco: para isso bastam XBE e XDVDFS,
que **foram** verificados. FATX e EEPROM entram quando o título for salvar ou consultar
configuração do console.
