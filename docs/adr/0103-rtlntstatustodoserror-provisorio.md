# ADR-0103: RtlNtStatusToDosError com tabela provisória

Data: 2026-11-05. Estado: aceito. PLANO 12.6 (ordinal 301, parcial). Autorização do
mantenedor para valores provisórios declarados.

## Contexto e evidência

O nxdk fixado importa `RtlNtStatusToDosError` (`ULONG f(NTSTATUS)`) para preencher
`GetLastError`, mas não define a tabela. O Cxbx-Reloaded fixado a trata como serviço do NT.
Os pares NTSTATUS/Win32 usados aqui são os da documentação pública do NT (`ntstatus.h` e
`winerror.h`), não de fonte do console.

## Decisão

Tabela com os 18 status que este kernel emite (sucesso, timeout, pendente, falha genérica,
tamanho, violação de acesso, handle inválido, parâmetro inválido, fim de arquivo, sem memória,
acesso negado, buffer pequeno, nome ausente, colisão, compartilhamento, mutante não possuído,
limite do semáforo, recursos insuficientes). Qualquer outro status devolve
`ERROR_MR_MID_NOT_FOUND` (317), o resultado documentado do NT para status sem mapeamento.
Função pura e sem estado: nada entra em snapshot.

## P31

A tabela é provisória e parcial: o console mapeia centenas de status. Um programa que dependa
de outro código verá 317. Condição de saída: tabela completa do kernel do console ou
ampliação conforme novos status passam a ser emitidos.

## Escopo

12.6 continua em andamento: `RtlRaiseException` (302, despacho SEH) e
`MmPersistContiguousMemory` (178, persistência no reinício) seguem sem implementação.

## Reversão

O ordinal sai de `IMPLEMENTADOS` e do `match`; o módulo `status_map` é removível.
