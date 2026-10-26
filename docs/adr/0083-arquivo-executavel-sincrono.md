# ADR-0083 — Arquivo imutável do executável e I/O síncrona

Data: 2026-10-26. Estado: aceito. PLANO 7.1, provisório P22.

## Contrato e fontes

O kernel ainda não abria arquivos. O próximo consumidor é um homebrew de
teste próprio que usa CreateFileA/ReadFile/GetFileSizeEx/CloseHandle do nxdk
fixado em 14d5ee97e73347c973f1f57b68b79ec08c9e77f2. Não é uma implementação
de um driver de disco nem uma montagem de arquivos arbitrários do host.

Fontes primárias do ABI Xbox:
[xboxkrnl.h](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/xboxkrnl/xboxkrnl.h),
[fileio.c](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/winapi/fileio.c),
[handleapi.c](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/winapi/handleapi.c).
OBJECT_ATTRIBUTES tem 12 bytes e usa ANSI_STRING; IO_STATUS_BLOCK tem 8.
NtCreateFile recebe nove DWORDs; NtReadFile recebe oito, sem o Key do NT
desktop. NtQueryInformationFile recebe cinco. Não copiar o ABI Windows atual.

O contrato público de
[NtCreateFile](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-ntcreatefile)
define FILE_OPEN, compartilhamento e FILE_OPENED no IOSB. O de
[NtReadFile](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-ntreadfile)
descreve posição síncrona, offset explícito, sentinela -2 e leitura até EOF.
São referências de comportamento NT, não prova de todas as variantes Xbox.

## Recorte

Files mantém bytes originais imutáveis de um único default.xbe, até 32 MiB,
e até 256 handles com posição e share. Handles vêm do contador monotônico
já compartilhado por mutants, eventos, links e thread; não há handle host.
A API de montagem só aceita uma publicação por dispatcher; a sessão será
ligada à imagem na tarefa 7.2. Até lá, Dispatcher novo não tem mídia.

- NtCreateFile (190): FILE_OPEN e SYNCHRONOUS_IO_NONALERT, leitura com
  SYNCHRONIZE e atributos de leitura (GENERIC_READ ou READ_DATA|READ_ATTRIBUTES),
  nome NT absoluto ou D: via link guest existente. Retorna handle
  e IOSB/FILE_OPENED; ausência e conflito de compartilhamento têm NTSTATUS real.
- NtReadFile (219): 1 byte até 1 MiB, sem event/APC. Posição por handle, offset
  não negativo ou -2; cópia curta preserva a cauda, EOF publica zero bytes.
- NtQueryInformationFile (211): classes 5, 14 e 34, correspondentes a
  Standard, Position e NetworkOpen; tamanhos 24, 8 e 56 bytes.
- NtClose (187): remove somente o handle; leitura posterior é INVALID_HANDLE.

Todos exigem PASSIVE. Disposições de escrita, async, tamanho zero, diretórios,
outros dispositivos/classes/flags e waits em arquivo recusam explicitamente.
O nome é ASCII NT, sem NUL/slash/traversal. OBJ_CASE_INSENSITIVE controla a
comparação do arquivo. Não se resolve caminho no filesystem do host.

Preflight verifica RAM/permissões para todas as saídas antes da cópia, sem
consumir posição ou handle em falha. IOSB e saída/buffer sobrepostos, inclusive
por aliases físicos, recusam. Entradas são capturadas antes dos stores, assim
aliases com argumentos não mudam o atendimento no meio da operação.

Metadados do arquivo virtual são declarados: timestamps desconhecidos zero,
atributo readonly, um link, allocation size igual ao comprimento em bytes.
Não se inventa geometria de CD, timestamps físicos ou latência de disco. P22
conserva esses limites; P4 continua aberto para nomes/mídia/VFS gerais.

## Persistência e verificação

KRNL v14 acrescenta CDFS v1: presença, comprimento/bytes, contagem e registros
ordenados handle/posição/share. Payload limitado antes de alocar, sem ponteiros.
Validação recusa versões, contagens, posições negativas, colisões com outros
objetos e compartilhamento impossível. KRNL v1–v13 migra sem mídia e sem
handles: restaurar/regravar não ativa um perfil novo.

Onze testes novos exercitam bytes, handles independentes, EOF, seek-and-read,
informações, namespace/case, preflight/aliases, compartilhamento, limites,
replay, corrupção e migração. Fixtures de GINS passam a localizar sua seção,
preservando as asserções; migração nativa v12 remove CDFS antes de GINS.
Gate e contagens finais na evidência de PLANO 7.1. A montagem automática,
Rust gerado e execução do produtor nxdk são tarefas 7.2/7.3, ainda pendentes.
