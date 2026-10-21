# ADR-0040 — InterlockedExchange em RAM guest

Data: 2026-10-20. Estado: aceito para o Bus síncrono/exclusivo atual.

## Contexto e fontes

Após a leitura PCI, o hello real parava em `InterlockedExchange` (54), retorno
`0x19DC7`, argumentos `0x29158, 1`. O catálogo define FASTCALL/8, não stdcall:
os dois argumentos chegam em ECX/EDX, sem palavras de argumento para limpar.

Fontes públicas, como referência técnica, sem copiar implementação:

- [nxdk xboxkrnl.h, revisão fixada (CC0)](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/xboxkrnl/xboxkrnl.h):
  `LONG FASTCALL InterlockedExchange(LONG volatile *Target, LONG Value)`.
- [Microsoft InterlockedExchange](https://learn.microsoft.com/en-us/windows/win32/api/winnt/nf-winnt-interlockedexchange):
  substitui LONG de 32 bits, devolve valor anterior e ordena operações de memória.
  A assinatura/ABI Xbox vêm do nxdk; não se usa o wrapper Windows como implementação.
- [ADR-0024](0024-cmpxchg-setcc.md) e [ADR-0028](0028-sfence-ordenacao-do-bus.md):
  atomicidade/ordenação sob acesso exclusivo ao Bus, sem observador concorrente.

## Decisão

Atender somente o ordinal 54 requerido pelo alvo. Comparação/troca, soma,
incremento/decremento, operações de lista e variantes de 64 bits continuam
recusados como serviços ausentes; não anunciar uma família inteira por implementar
apenas uma função. A instrução CMPXCHG já existente é um caminho distinto.

O dispatcher captura Target/Value por `Call::argument`, verifica os quatro bytes
como RAM legível **e** gravável, lê o valor anterior e publica o novo. EAX recebe
os 32 bits anteriores, inclusive padrões que representam LONG negativo. Não é
NTSTATUS nem convenção de retorno VOID. A sessão preserva flags e registradores
fora de EAX/ESP/EIP, conforme seu contrato HLE existente.

| Situação | Comportamento do perfil |
|---|---|
| RAM RW válida | troca indivisível no modelo de execução guest atual |
| novo valor igual ao anterior | continua exigindo permissão de escrita |
| desalinhamento/página cruzada inteiramente em RAM RW | permitido pelo perfil funcional do Bus |
| nulo, wrap, página ausente, RO/RX, W sem R ou MMIO | erro de argumento antes da leitura/alteração do alvo |
| alvo sobre a pilha de retorno | usa continuação capturada antes do efeito |
| retorno guest inválido | Session recusa antes de executar o serviço |

Desalinhamento segue o perfil x86/Bus atual, não uma promessa da API para outras
arquiteturas ou máquinas com múltiplos processadores. A validação integral evita
stores parciais mesmo quando a segunda página não permite a operação. MMIO é
recusado antes de qualquer leitura ou escrita com efeitos de dispositivo.

Não há yield entre load/store, e `&mut Bus` mantém RAM/arena exclusivas. Portanto
nenhum observador guest pode ver uma troca pela metade. O serviço chama o contrato
de ordenação do Bus antes/depois; no perfil síncrono atual, leituras e escritas
de RAM já acontecem sequencialmente, sem operações pendentes. Isso não autoriza
acesso concorrente à arena nem cria uma fence física completa para MMIO/DMA host.
Filas, DMA concorrente ou scheduler com novos observadores exigirão rever esse
protocolo, como já exigem CMPXCHG/SFENCE. Não usar AtomicU32 sobre ponteiro guest
ou acrescentar unsafe para simular atomicidade entre threads host.

## Snapshot e ABI

Nenhum estado host novo ou operação parcialmente concluída: os formatos continuam
**AOTS v1 / AOTC v7 / KRNL v3 / KMUT v2 / EEPR v1 / PCIC v1**. RAM guarda o LONG,
e CPU/pilha guardam a chamada pendente. A continuação já é capturada/validada por
Session antes do serviço, permitindo escrever até sobre o endereço de retorno.
Uma chamada FASTCALL/8 remove somente o retorno de 4 bytes; argumentos em ECX/EDX
não geram limpeza de 8 bytes na pilha. Tail-call de kernel pode retornar ao host.

## Validação e execução real

- Cinco testes de kernel cobrem 60 combinações de bits/desalinhamento, guardas,
  ECX/EDX em vez da pilha, permissões, MMIO sem acesso, páginas e aliases/RAM/DMA.
- Cinco testes compilam x86 → Rust e verificam fastcall, flags/registradores,
  retorno anterior observado pelo guest, tail-call, snapshot com chamada pendente,
  erro/retry, alias do retorno e validação do retorno antes de efeitos.
- **336 testes workspace/doctests e 140 casos nativos debug/release** (124 fora
  de x86_64), Rust 1.98.1, Clippy global, fmt e diff aprovados.
- O hello mantém quatro raízes/9027 blocos e passa pelo ordinal 54. Um teste
  local adicional sobre o XBE real verificou EAX anterior e a escrita em RAM
  após a troca; o LONG em 0x29158 ficou 1 e `tsc_freq` continuou 733333333 Hz.
- O próximo bloqueio é **KeQuerySystemTime (128)**, retorno `0x20D8B`, argumento
  `0x7000FDD8`. Registro: **14/69 serviços**, com variantes limitadas.

Próximo passo: contrato explícito de tempo de sistema guest. O header nxdk
documenta saída LARGE_INTEGER de 8 bytes em unidades de 100 ns desde 1601/GMT.
Definir época guest e como o dispatcher recebe o Clock da sessão; não usar
relógio de parede host, contador `retired` ou valor zero para passar pelo caller.
Preservar determinismo, validação integral da saída e snapshot da referência
temporal. Threads, execução completa e framebuffer continuam pendentes.

Reprodução em diretório novo/vazio:

```sh
cargo run --offline -p xberecomp -- translate target/homebrew/hello.xbe --entry 0x19110 --entry 0x145E0 --entry 0x205E0 --entry 0x20889 --out target/alpha-hello-system-time
cargo run --offline --manifest-path target/alpha-hello-system-time/Cargo.toml
```

Logs: `target/interlocked-{kernel-tests,workspace-tests,doctests,clippy}.log`,
`target/interlocked-native-release-tests.log`, `target/interlocked-hello-{run,ordinals,effect-test}.log`.
O teste adicional está no artefato ignorado
`target/alpha-hello-msr-ratio-20261020/tests/interlocked_effect.rs`, sobre o hello
de SHA-256 `baaa88110014743864d0f38606e18eb423b0bf0da5bb4083a3e55b89a0a2af7f`.

## Como reverter

Remover o ordinal 54 de IMPLEMENTADOS/dispatcher e o módulo `interlocked`.
Nenhuma versão ou migração de snapshot precisa ser revertida.
