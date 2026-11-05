# ADR-0102: posição, atributos completos e escrita negada na mídia (valores provisórios)

Data: 2026-11-05. Estado: aceito. PLANO 12.5a (ordinais 210, 226 e 236). Autorização do
mantenedor em 2026-11-05 para adotar valores provisórios declarados.

## Contexto e evidência

A análise de 12.5 (PLANO) mostrou que o contrato do nxdk fixado cobre só parte do que os
consumidores precisam: `SetFilePointer` usa `NtSetInformationFile` com
`FilePositionInformation` (classe 14, `LARGE_INTEGER`), `GetFileAttributesEx` repassa ao guest
os carimbos de `FILE_NETWORK_OPEN_INFORMATION`, e `WriteFile` chama `NtWriteFile`. Não há
fonte no projeto para carimbos de data em mídia óptica nem para o status de escrita em mídia
somente leitura.

## Decisão

**NtSetInformationFile (226).** Só a classe 14. Exige `Length == 8` (senão
`STATUS_INFO_LENGTH_MISMATCH`, 0xC0000004), posição não negativa (senão
`STATUS_INVALID_PARAMETER`, 0xC000000D, IOSB escrito) e handle de arquivo; aceita posição
além do fim (a leitura seguinte devolve fim de arquivo). IOSB: status 0 e `Information` 0
(provisório). Outras classes e diretórios seguem variantes recusadas. A posição já fazia parte
do snapshot (CDFS), então nada muda de versão.

**NtWriteFile (236).** A mídia é a imagem original, imutável. Toda escrita em handle válido
devolve `STATUS_ACCESS_DENIED` (0xC0000022), grava o IOSB com `Information = 0` e não muda
nada. Handle inválido: `STATUS_INVALID_HANDLE`; evento/APC e diretório: variante recusada.

**NtQueryFullAttributesFile (210).** Resolve o nome como a abertura (mesmo `file_name`, mesmos
casos de nome ausente) e preenche `FILE_NETWORK_OPEN_INFORMATION` (56 bytes) com os **mesmos
valores** que a classe 34 de `NtQueryInformationFile` já devolvia: `AllocationSize` e
`EndOfFile` iguais ao tamanho, atributo 1 (somente leitura).

## Valores provisórios (P30)

1. Carimbos de criação, acesso, escrita e alteração são **zero** (sem data no disco de
   mídia modelado).
2. Escrever em mídia somente leitura é `STATUS_ACCESS_DENIED` (não medido no console).
3. `IO_STATUS_BLOCK.Information` é 0 em `NtSetInformationFile` e `NtWriteFile` negado.

## Fora do escopo

`NtQueryVolumeInformationFile` (218) segue **não implementado**: o ADR-0091 recusa partições
fictícias e valores de volume (tamanho de setor, serial, rótulo) não têm fonte. Marcado como
BLOQUEADO no PLANO; a 12.5 continua em andamento.

## Validação

Quatro testes de kernel (`tests/file_provisionals.rs`): posição vale para a leitura seguinte e
não afeta outro handle, posição além do fim, recusas sem alterar o estado, escrita negada sem
efeito no snapshot, atributos por nome idênticos à classe 34 e nome ausente sem escrita. Um
caso no Rust emitido (`kernel_file_provisionals`) com NtCreateFile, 210, 226 e 236.

## Reversão

Os três ordinais saem de `IMPLEMENTADOS` e do `match`; nenhum formato de snapshot mudou.
