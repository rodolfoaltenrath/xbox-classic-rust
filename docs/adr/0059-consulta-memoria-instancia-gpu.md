# ADR-0059 — Consulta de memória de instância GPU com reserva física

Data: 2026-10-22. Base de código: `1a6d5c1`.
Estado: aceito para a consulta retail 64 MiB; bootstrap provisório P13.

## Evidência e decisão

A revisão dos commits posteriores ao ADR-0043 confirmou os marcos já entregues:
hello executando main, apresentação Win32, laço por fatias, eventos/ISR/DPC,
ISA ampliada e perfis NVPLL/PFB. O gate inicial no Linux passou com 495 testes.
O segundo alvo, triangle/pbkit, ainda parava antes de atender o ordinal 168,
retorno 0x182AB, argumentos 0xFFFFFFFF/0x0005FE14, aos 24701909 ciclos/retired.
Esta entrega atende somente essa consulta e reserva RAM real antes de responder.

Fontes públicas fixadas, usadas para conferir contratos, sem copiar implementação:

- [nxdk, xboxkrnl.h](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/xboxkrnl/xboxkrnl.h):
  assinatura com NumberOfBytes e NumberOfPaddingBytes, retorno PVOID e NTAPI.
- [nxdk, pbkit.c](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/pbkit/pbkit.c):
  pb_init consulta com 0xFFFFFFFF, verifica NULL e comenta retorno 0x83FF0000
  com padding 0x10000. Subtrai 0x5000 para pb_GpuInstMem, mas esse campo não
  é usado novamente nessa revisão; RAMHT e instâncias são acessados via PRAMIN.
- [Cxbx, CxbxKrnl.h](https://github.com/Cxbx-Reloaded/Cxbx-Reloaded/blob/585c49a50af1255ab155099e06f24505f9c5a800/src/core/kernel/init/CxbxKrnl.h)
  e [VMManager.cpp](https://github.com/Cxbx-Reloaded/Cxbx-Reloaded/blob/585c49a50af1255ab155099e06f24505f9c5a800/src/core/kernel/memory-manager/VMManager.cpp):
  perfil retail com 16 páginas de instância a partir do PFN 0x03FE0,
  limite de RAM PFN 0x04000, e cálculo do padding de 64 KiB. A consulta não
  redimensiona; tamanhos diferentes solicitam ajuste de páginas. Há tratamento
  distinto para debug/Chihiro, que não foi implementado aqui.
- [xemu, nv2a.c](https://github.com/xemu-project/xemu/blob/03b988c9d76553b33ed712ba00822ffdf49f0e7d/hw/xbox/nv2a/nv2a.c):
  mantém RAMIN independente e deixa comentada a hipótese de alias com VRAM.
  Isso não comprova uma tradução física da janela PRAMIN. Não foi adotado
  um alias presumido PRAMIN → topo da RAM.

Essas referências sustentam um perfil explícito de compatibilidade, não uma
medição do console ou o estado de reset físico. Nenhuma BIOS proprietária foi usada.

## Contrato e provisório P13

MmClaimGpuInstanceMemory (168), stdcall/8, aceita NumberOfBytes=0xFFFFFFFF,
IRQL Passive e arena real de 64 MiB. A saída exige quatro bytes graváveis em
RAM guest; captura dos argumentos, validação da saída e continuação precedem
a mutação. Outros tamanhos, prioridades e perfis 128 MiB/devkit recusam com
VarianteNaoImplementada, incluindo chamador e argumentos.

No primeiro atendimento, o bootstrap P13 reserva o intervalo físico completo
[0x03FE0000, 0x04000000), 32 frames, através do mesmo alocador contíguo:

| Intervalo físico | Papel no perfil | VA CPU |
|---|---|---|
| 0x03FE0000..0x03FF0000 | instância, 64 KiB | 0x83FE0000..0x83FF0000 |
| 0x03FF0000..0x04000000 | padding reservado, 64 KiB | 0x83FF0000..0x84000000 |

A reserva é registrada em CMEM com alinhamento de 4 KiB e PAGE_READWRITE;
MMU, bitmap de frames e DMA usam a única RAM do Bus. O serviço devolve a
**fronteira** 0x83FF0000 em EAX e escreve padding=0x10000, não o começo da
reserva. A VA 0x83FEB000 obtida pela subtração do pbkit tem backing físico real.
A RAM é zerada somente ao provisionar; consultas repetidas preservam os bytes
e a quantidade de frames. Não há cobrança de ciclos nem alteração de flags.

Se qualquer frame estiver ocupado, faltar capacidade ou o registro CMEM
estiver cheio, retorna NULL sem deslocar a reserva, tomar memória existente ou
alterar a saída. Metadados de memória inconsistentes recusam explicitamente.
A extração do caminho comum de publicação mantém a alocação contígua existente.

**P13 é deliberado:** no console, a reserva já pertence à inicialização do sistema;
aqui surge na primeira consulta. O padding inteiro também fica protegido pelo
mesmo recurso CMEM, sem simular a PFN database. Alocações anteriores podem causar
colisão e NULL. A condição de saída é integrar a reserva ao boot/firmware, definir
sua posse e o backing de PRAMIN com evidência, além dos perfis de memória maiores.
Não transformar esse bootstrap em política permanente por omissão.

PRAMIN, PFIFO, PGRAPH, DMA de comandos e resize continuam sem implementação.
Não há VRAM duplicada, unsafe novo, worker, relógio extra ou mudança em Send/!Sync.

## Snapshot

KRNL v13 acrescenta GINS v1 depois de KINT: tag, u32 versão, u8 provisioned
canônico 0/1, nove bytes. O estado ativo exige o recurso CMEM exato; a restauração
integral também confere tamanho da arena, frames, traduções, permissões e aliases.
Marcadores/versões inválidos ou reserva ausente recusam sem publicar Dispatcher
parcial. RAM/MMU/bitmap continuam serializados pelo Bus, sem duplicar o conteúdo.

KRNL v1–v12 migram sem bootstrap ativo e sem herdar o estado do destino.
A próxima consulta pode provisionar o perfil se o intervalo estiver livre.
BUS0 v4, AOTC v8 e AOTS v3 permanecem iguais. No teste de Session v12, remover
GINS no próprio intervalo preserva TCXT, que vem depois do kernel; truncar a
Session inteira no começo de GINS destruiria esse sufixo.

## Verificação e reprodução

Cinco testes de kernel cobrem reserva/zero/permissões, DMA, repetição, saída
inválida ou cruzando página, colisão, variantes recusadas, roundtrip e corrupção
de snapshot e migração KRNL v12. Três casos no Rust emitido cobrem stdcall/8,
CPU/flags/Clock/pilha, replay integral, migração e saída sobre o endereço de retorno.

Gate no Linux/Rust 1.98.1: **500 testes workspace/doctests**, **215 casos gerados
em debug e release** (contador esperado 199 fora de x86_64), fmt e Clippy all-targets
com warnings negados. Release também usou RUSTFLAGS=-D warnings. Os sete testes
Python dos scripts passaram separadamente. A diferença entre os 499 documentados
no ADR-0058 e os 495 do baseline Linux são quatro testes Win32 condicionais;
esta entrega acrescenta cinco testes de kernel. A apresentação Windows não foi
revalidada nesta máquina. Hello continua 37/69 serviços; triangle passa a 41/75.

Triangle foi recompilado com o nxdk fixado, sem alterar bytes guest:

```text
triangle.exe SHA256 ff6a52bd23573a3d110369676ad25643c133f171a9e09cf50b042b3f91681543
triangle-raw.xbe SHA256 f6288954b7f3bc38a367a5833012fc6ba8929d0d358ee1a68f0800ce89aeaf93
```

As mesmas dez raízes da passagem de bastão produzem 17883 blocos. O comando
release com --fatias 4000 passa do ordinal 168 e toca PRAMIN sem modelo, inclusive
0xFD71386C a partir de EIP=0x183AA. A execução integral termina depois em escrita
não mapeada VA=0x80000000, EIP=0x18F19; framebuffer segue com zero pixels coloridos.
O desmonte confirma MOV [0x80000000],EAX em 0x18F19, WBINVD em 0x18F1E e
call pb_start em 0x18F20. Corresponde à escrita de (pb_Head & 0x0FFFFFFF)+1
em [pb_init, linha 2775](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/pbkit/pbkit.c#L2775).
O consumidor cria antes um contexto DMA com base 0x80000000 e em seguida espera
NV_USER DMA_GET no offset 0x44. A falta é o mapeamento CPU ausente dessa página
usada pelo bootstrap DMA; ela não prova uma relação física entre PRAMIN e a
reserva atendida pelo ordinal 168. Não foi criado um alias amplo para ocultá-la.
O próximo contrato precisa definir posse/mapeamento dessa página e armazenamento
PRAMIN, depois efeitos PFIFO/DMA_GET. Isso não é aceite de pb_init ou renderização.

Uma sonda local interrompe deliberadamente na primeira fatia que observa PRAMIN,
conferindo padding, 32 novos frames e mapeamentos físicos após a consulta.
Passou em EIP=0x183B5, retired/ciclos=24706005, 53 chamadas atendidas.
Ela complementa os testes versionados; não passa a execução completa artificialmente.

Logs locais ignorados: target/instance-before-workspace-tests.log,
instance-workspace-tests.log, instance-clippy.log, instance-native-release.log,
instance-script-tests.log, instance-hello-ordinals.log, instance-triangle-ordinals.log,
instance-triangle-before.log, instance-triangle-after.log e instance-triangle-probe.log,
todos sob target/. Projeto traduzido e sonda:
`target/homebrew2/instance-review-20261022/tests/instance_probe.rs`.

Para reproduzir, reconstruir o sample com scripts/build-nxdk-sample.sh e usar as
dez raízes/comandos do CONTINUIDADE_ALPHA.md, sempre com diretório novo de tradução.
