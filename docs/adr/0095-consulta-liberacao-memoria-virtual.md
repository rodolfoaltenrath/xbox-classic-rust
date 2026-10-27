# ADR-0095: consulta e liberação integral de regiões privadas conhecidas

Data: 2026-10-27. Estado: aceito. PLANO 12.2.

## Contexto e fontes

O hello importa NtFreeVirtualMemory (199, stdcall/12) e NtQueryVirtualMemory
(217, stdcall/8). As assinaturas Xbox e MEMORY_BASIC_INFORMATION de sete campos
de 32 bits vêm do nxdk 14d5ee97e73347c973f1f57b68b79ec08c9e77f2,
[xboxkrnl.h](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/xboxkrnl/xboxkrnl.h).
lib/winapi/memory.c é consumidor público: VirtualFree exige tamanho zero com
MEM_RELEASE e VirtualQuery usa diretamente os dois argumentos do Xbox.

Os contratos públicos de
[ZwFreeVirtualMemory](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-zwfreevirtualmemory)
e [ZwQueryVirtualMemory](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-zwqueryvirtualmemory)
fundamentam o recorte NT: base exata/RegionSize zero para release, tamanho
liberado em saída, consulta a partir da página do endereço até o fim dos atributos
homogêneos. Não se transpõe a ABI Windows, nem se afirma equivalência de todas
as variantes ao Xbox. filemanip.c contém release com tamanho não zero: esse
consumidor permanece explicitamente fora deste recorte, sem presumir o resultado.

## Decisão

199 libera uma região integral VMEM, com MEM_RELEASE e tamanho de entrada zero,
somente em Passive. Captura/preflight das entradas e saídas precedem a mutação.
Rejeita sobreposição física entre as saídas, saídas dentro da região liberada,
pilha ativa (inclusive frame ABI atravessando a borda) e região da filha,
que contém stack/TLS/TIB/KTHREAD. Imagem e pilha inicial não são recursos VMEM
liberáveis. Enquanto existir a filha, sua região permanece protegida, inclusive
depois do término: teardown de threads não foi implementado por este ADR.

A posse e ausência de aliases físicos externos são validadas pelo VMEM existente.
MMU/bitmap são planejados em cópias; liberação remove todos os mapeamentos,
devolve frames e retira o registro. Bytes físicos permanecem até reuso; o
alocador atual os zera. Saídas permanecem mapeadas e recebem base/tamanho real.
Demais flags, tamanhos e bases recusam com variante explícita.

217 só consulta regiões privadas committed registradas: base da página consultada,
base/proteção da alocação, bytes restantes até o fim, MEM_COMMIT/PAGE_READWRITE/
MEM_PRIVATE. Serializa sete ULONGs, exatamente 28 bytes. Não infere metadados
de heap, imagem, MMIO, região livre ou reservada pela existência de mapeamento.
Esses caminhos recusam. Buffer inválido devolve ACCESS_VIOLATION sem store.
O preflight usa consultar/permissões/backing RAM, sem alterar A/D nem executar MMIO.

VMEM v1 e KRNL v15 permanecem: estado e identidade dos recursos não mudaram.
O contrato novo atua também sobre os recursos restaurados. Limites em P24.

## Validação e consequências

Seis testes de kernel verificam layout e cauda, consulta interior, liberação,
reuso, vizinhos, falhas/aliases/MMIO, stacks/TLS/TIB, snapshot e posse corrompida.
Dois testes do Rust emitido verificam stdcall/8 e /12, todos os registradores,
flags e relógio, snapshot integral antes do release e recusa com retomada.
Hello passa de 44/69 para 46/69 variantes disponíveis; não é cobertura dinâmica.

Não há suporte a MEM_DECOMMIT/reserva separada, working set ou VADs gerais.
Reverter os dois braços/entradas do dispatcher e operações/testes novos desfaz
a entrega sem migrar snapshots. Não alterar o tamanho nem a interpretação VMEM v1.
