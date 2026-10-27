# ADR-0092 — Enumeração do diretório da mídia declarada

Data: 2026-10-27. Estado: aceito. PLANO 11.1, provisórios P4/P22.

## Contexto e fontes

O outro computador entregou X11, o pipeline do mesh e SMBus/SMC (ADRs
0086–0090). Os samples WinAPI continuam bloqueados ao montar C:/E: sem
imagem de HD (ADR-0091). Essa recusa permanece. Há, porém, uma mídia já
declarada: o XBE original publicado pelo perfil ImageFileV1 do ADR-0083.
Seu diretório virtual pode enumerar esse arquivo sem fabricar uma partição.
Não se representa o conteúdo de uma ISO nem a geometria de um CD físico.

Fontes primárias, nxdk fixado em 14d5ee97e73347c973f1f57b68b79ec08c9e77f2:

- [findfile.c](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/winapi/findfile.c): consumidor FindFirst/FindNext/FindClose.
- [xboxkrnl.h](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/xboxkrnl/xboxkrnl.h): NtOpenFile/6 argumentos, NtQueryDirectoryFile/10; struct com FileName ANSI no offset 64.
- [catálogo de exports](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/xboxkrnl/xboxkrnl.exe.def): ordinais 202/207, stdcall 24/40.

O contrato público de
[NtQueryDirectoryFile](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-ntquerydirectoryfile)
orienta a captura do filtro na primeira chamada, sua preservação nas chamadas
seguintes, restart e Information como contagem dos bytes retornados. A ABI
Windows atual não é copiada: tem Unicode e ReturnSingleEntry adicionais.

## Recorte

NtOpenFile aceita somente a raiz CdRom0, DIRECTORY_FILE|SYNCHRONOUS_IO_NONALERT,
LIST_DIRECTORY|SYNCHRONIZE, em Passive. Nome absoluto ou D: resolvido pelo link
guest existente; sem link, há STATUS_OBJECT_NAME_NOT_FOUND. Sem mídia ou com
perfil legado, recusa explícita. HD e outros dispositivos continuam sem modelo.

NtQueryDirectoryFile atende classe1, sem evento/APC. Um registro ANSI de 75 bytes
contém default.xbe, tamanho da mídia original, NextEntryOffset/FileIndex zero,
atributo readonly e timestamps desconhecidos zero, como os metadados do ADR-0083.
Somente o registro é escrito; a cauda do buffer permanece intacta. Não há entradas
artificiais . e .. neste diretório de manifesto. Isso é um perfil virtual declarado.

Glob ASCII limitado a letras, números, . _ - * ?: comparação case-insensitive;
não simular tokens DOS adicionais. Filtro nulo/vazio equivale a *. A primeira
consulta captura o filtro; as seguintes ignoram o ponteiro, mesmo com restart.
Cursor por handle: sucesso uma vez, STATUS_NO_MORE_FILES no fim; a primeira
consulta sem correspondência retorna STATUS_NO_SUCH_FILE. Restart permite
devolver novamente a mesma entrada, preservando o filtro inicial.

Não suportamos buffers parciais: buffer menor que 75 bytes ou maior que 1 MiB é
variante recusada, não um resultado truncado com semântica NT inventada.
Preflight verifica RAM, permissões e aliases físicos IOSB/buffer antes de qualquer
efeito; entradas são capturadas antes dos stores. Falhas não consomem handles,
filtro ou cursor. Compartilhamento e limite 256 abrangem os objetos de arquivo e
diretório; handles usam o contador monotônico já existente. NtClose fecha ambos.
NtReadFile/informações de arquivo sobre um diretório são recusados explicitamente.

## Persistência e compatibilidade

CDFS v2 mantém o prefixo de mídia/arquivos de v1 e acrescenta DIRE v1: presença
do perfil, registros ordenados com handle/share, cursor esgotado e filtro inicial.
Limites são verificados antes de alocar; versões, booleanos, filtros, contagens,
compartilhamento impossível e colisões com outros objetos são recusados.
KRNL permanece v15 porque a ordem das subseções é a mesma; o formato que mudou
tem sua própria versão. SAVE/AOTS não mudam.

CDFS v1 migra com mídia/arquivos preservados e diretórios desativados; regravar
não ativa o perfil. Kernel anterior à v14 continua sem mídia. Nova publicação
explícita do executável ativa o perfil para sessões novas, não para restaurações.
Fixtures nativas retiram CDFS/DIRE/SMBC pelo layout, preservando as outras seções.
O caso de versão futura corrompida passa de 2 para 3 porque 2 agora é válido; a
asserção de recusa e os demais casos permanecem. GINS/SMBC mantêm seus testes.

Oito testes do kernel cobrem ABI/dados, filtros, cursor/restart, namespace,
preflight, aliases, limites, colisões, corrupção e migração sem promoção. Dois
casos no Rust emitido exercitam stdcall, efeitos, recusa e replay integral desde
diretório aberto. Gate/contagens finais estão na evidência de PLANO 11.1.

## Próximo consumidor

Fonte própria file-enum usa D:, sem chamar os construtores de montagem de HD
dos samples bloqueados. Ela valida FindFirst/FindNext/FindClose e compara o
tamanho enumerado com o arquivo efetivamente aberto. Isso não desbloqueia os
samples C:/E: do ADR-0091 nem encerra P4/P22. Integração visual é a tarefa 11.2.
