# ADR-0094: alocação contígua simples e consulta dos recursos possuídos

Data: 2026-10-27. Estado: aceito. PLANO 12.1.

## Contexto

O hello importa 165/180 mas seu caminho cold boot não os chama. O nxdk fixado
em 14d5ee97e73347c973f1f57b68b79ec08c9e77f2 contém consumidores públicos:
lib/hal/xbox.c aloca LaunchDataPage com MmAllocateContiguousMemory;
lib/hal/video.c consulta o framebuffer herdado antes de liberá-lo. A ABI vem de
[xboxkrnl.h](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/xboxkrnl/xboxkrnl.h).
São um argumento de 32 bits e stdcall/4 em ambos os casos, diferentemente da
assinatura de alocação do Windows desktop.

## Decisão

165 delega ao mesmo planejador de 166 com faixa física irrestrita, alinhamento
de página e proteção RW do perfil AOT. Não duplica arenas ou listas. O perfil
conserva política local de zerar, Passive, limite e geometria do ADR-0048;
falhas de tamanho/capacidade devolvem NULL. Não promete equivalência de cache,
ordem de escolha de frames ou permissões executáveis com hardware.

180 aceita a base exata de recurso CMEM existente e devolve seu tamanho em
páginas. Antes, valida toda a relação entre recurso, MMU e posse física, sem
ler conteúdo, MMIO ou alterar A/D. Base interior, desconhecida, liberada ou
fora do perfil recusa explicitamente, sem inventar comportamento do console.
Este é um recorte de recursos contíguos, não consulta de heap/pool/VA arbitrário.

Ambas entram no dispatcher e contador derivado. CMEM v1 e KRNL v15 não mudam:
nenhum campo persistente foi acrescentado. Alocações restauradas podem ser
consultadas e liberadas pelo mesmo mecanismo. P23 registra os limites.

## Validação

simple_contiguous.rs exercita arredondamento, NULL sem recursos, leitura
cruzando páginas, consulta de recurso Ex/WRITECOMBINE, bases inválidas,
integridade MMU/posse, IRQL e restauração. Dois casos do Rust emitido chamam
165/180/171, conferem limpeza da pilha e registradores/flags, snapshot integral
e recusa com CPU intacta seguida de retomada.

Testes antigos de serviço ausente passam a usar MmQueryStatistics (181), com
a mesma ABI stdcall/4, preservando diagnóstico e integridade. O teste genérico
de ABI continua fazendo conclusão manual, independente da implementação.

## Consequências e reversão

Hello passa de 42/69 para 44/69 importações com alguma variante disponível;
isso não significa que novas chamadas sejam executadas pelo sample original.
As outras 25 são o restante do marco. Para reverter, remover os dois braços e
entradas do dispatcher e seus testes novos; nenhum snapshot requer migração.
