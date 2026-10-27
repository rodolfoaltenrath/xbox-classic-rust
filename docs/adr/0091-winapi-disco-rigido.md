# ADR-0091 — winapi_filefind e winapi_drivelist param no disco rígido

Data: 2026-10-26. Estado: aceito. PLANO 10.2, provisórios P4/P22.

## Contexto

Os samples `winapi_filefind` e `winapi_drivelist` do nxdk fixado começam com
`nxMountDrive('C', "\\Device\\Harddisk0\\Partition2\\")` (o segundo monta
também E: em `Partition1`). Depois listam `C:\*.*` com
`FindFirstFile`/`FindNextFile` ou consultam `GetLogicalDrives` e
`GetLogicalDriveStringsA`. Os dois compilam com o toolchain local
(`target/homebrew-winapi_*/`) e entram no preparo/sonda.

## O que acontece

As duas sondas param no mesmo ponto, com recusa explícita e o serviço exato:

```
variante de kernel não implementada: IoCreateSymbolicLink (ordinal 67),
chamado de 0x000141A8; perfil só cria D: para diretório de CdRom0
```

(`target/sonda/winapi_filefind-20261026-234029-z3jj9gyH.log`; o drivelist
recusa a mesma variante em 0x00015928, em
`target/sonda/winapi_drivelist-20261026-234409-eMlcPQJG.log`.) Antes disso,
46 chamadas são atendidas.

## Decisão

Manter a recusa. O modelo não tem disco rígido: não há imagem FATX das
partições nem evidência do conteúdo que o console teria em C: e E:. Inventar
uma partição vazia violaria a regra 1 do projeto, e devolver
`STATUS_OBJECT_NAME_NOT_FOUND` faria o guest concluir que o console não tem
disco, o que é falso. Pelo mesmo motivo, `NtOpenFile`,
`NtQueryDirectoryFile` e `NtOpenDirectoryObject`/`NtQueryDirectoryObject`
(enumeração de arquivos e de `\??`) continuam pendentes: só fazem sentido
sobre uma mídia carregada.

O que P22 permite hoje (o `default.xbe` imutável em D:) não é alcançado por
esses samples, que nunca leem D:.

## Condição para avançar

Ligar a VFS existente (`xbox-formats`: FATX somente leitura, pastas do host)
ao kernel com uma imagem de disco fornecida pelo usuário e declarada na sessão.
Só então IoCreateSymbolicLink para `Harddisk0\PartitionN`, a abertura de
diretórios e a enumeração entram, com snapshot da mídia como no ADR-0084. P4 e
P22 registram esses dois consumidores.
