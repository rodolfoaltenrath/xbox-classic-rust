# ADR-0033 — SHA-1 em contexto guest opaco e copiável

Data: 2026-10-20. Estado: aceito para o perfil HLE atual.

## Caminho observado e escopo

Depois de criar mutex por NtCreateMutant, o hello real parou em **XcSHAInit (335)**,
retorno reportado `0x19140`, argumento `0x28EF0`. O fonte fixado do nxdk contém
`mtx_init` seguido de SHAInit em `init_rand_s`; depois consulta EEPROM e alimenta
o hash. Não substituir esse caminho por sucesso genérico ou uma sequência de zeros.

Implementar a família **XcSHAInit (335), XcSHAUpdate (336) e XcSHAFinal (337)**
como uma unidade: a inicialização fica verificável pelo digest, e os três serviços
compartilham o mesmo estado. Update e Final têm validação em fixtures; isso não
prova que o hello real já os executou.

O contrato público nxdk fixa buffer opaco de **116 bytes**, digest de 20 bytes
e assinaturas VOID/stdcall de 4/12/8 bytes. Não publica um layout dos campos
internos. A implementação Rust segue as equações SHA-1 e os vetores do RFC 3174;
não incorpora implementação de kernel, BIOS ou código de outro emulador.

## Formato e limites do perfil

Todo o estado autoritativo fica nos 116 bytes da **RAM guest**. Não existe mapa
host indexado pelo endereço do contexto. Copiar o buffer cria um contexto
independente, e o snapshot normal da RAM preserva um hash parcial.

| Offset | Tamanho | Conteúdo do perfil HLE v1 |
|---|---|---|
| 0 | 4 | marcador ASCII `XSH1` |
| 4 | 4 | versão 1, little-endian |
| 8 | 20 | cinco palavras de estado SHA-1, little-endian |
| 28 | 8 | total de bytes consumidos, little-endian |
| 36 | 64 | bloco parcial; bytes após `total % 64` ficam zerados |
| 100 | 16 | reserva zerada |

**Esse é um formato interno do HLE**, não evidência do layout de um Xbox físico.
O perfil atende código que trata o contexto como opaco e o manipula por esses
serviços. Não importar contextos de um kernel físico nem prometer compatibilidade
com código que inspeciona campos privados. Marcador, versão, reserva, contador
e cauda do buffer são validados; contexto não inicializado é erro identificável.

Init escreve exatamente 116 bytes, com constantes iniciais reais do SHA-1,
contador zero e buffer vazio. Update processa entrada em blocos temporários
limitados, sem alocar o tamanho informado pelo guest. O contador aceita até
`u64::MAX / 8` bytes; um avanço além disso é erro antes de qualquer efeito.
O digest é emitido em ordem big-endian, conforme SHA-1.

Para Final, adotar a política local de finalizar **uma cópia**, preservando o
contexto de origem quando os buffers não se sobrepõem. A declaração nxdk marca
o contexto como `IN`, e seu `rand_s` continua com Update depois de Final; isso
fundamenta a escolha do perfil, mas **não prova preservação byte a byte pelo
kernel físico**. Novas evidências de compatibilidade podem exigir rever essa
política e versionar o formato. Final repetido e Update posterior são testados.

## Memória e falhas

Contexto, entrada não vazia e digest exigem RAM guest com as permissões corretas;
MMIO é recusado antes de acessar dispositivos. Validar faixas por página, evitando
um laço de tradução por byte em buffers grandes. Ponteiro nulo para buffer não
vazio, wrap de endereço, falta de página ou proteção são erros do runtime.
Esses serviços são VOID: não inventar NTSTATUS; EAX zero segue a convenção local
já existente para funções VOID.

Update lê/valida o contexto e toda a entrada antes de publicar a nova imagem
do contexto. A compressão opera em cópia local, e o único store final usa o
preflight integral do Bus. Entrada pode sobrepor o contexto: todos os bytes
originais são consumidos antes do store. Entrada de tamanho zero não acessa
o ponteiro de entrada, inclusive se ele for nulo, mas ainda valida o contexto.

Final calcula a partir da cópia e escreve somente 20 bytes. Se a saída sobrepõe
o contexto, esses 20 bytes o modificam como qualquer store guest; não há registro
host que esconda essa mudança. Falta tardia em Init/Update/Final não deixa estado
ou digest parcialmente escrito. A CPU permanece na chamada pendente em erro.

Não há novos ponteiros host, unsafe, dependências ou estado de kernel a serializar.
`AOTS`/`KRNL`/`KMUT` continuam v1; `AOTC` continua v6. O marcador dentro do contexto
versiona seu formato. O serviço ainda é síncrono e o orçamento por blocos AOT
não interrompe uma chamada HLE longa; controle de execução segue pendente.

## Validação

Os testes de serviço usam os vetores públicos `abc`, mensagem de 56 bytes e um
milhão de `a` do RFC 3174, além do hash vazio. Outros 15 tamanhos, incluindo
55/56/57, 63/64/65 e 4095/4096/4097, têm resultados calculados separadamente por
`System.Security.Cryptography.SHA1.ComputeHash` do .NET. Cada tamanho é exercitado
com sete divisões de entrada diferentes, sem usar a implementação como oráculo.

Também são cobertos guardas, reinicialização, cópia independente de contexto,
buffer vazio, contexto inválido, overflow, proteção RO, MMIO, desalinhamento,
cruzamento de páginas, sobreposição, faltas tardias e snapshot parcial.
Três testes adicionais compilam o Rust emitido e verificam ABI, digest de `abc`,
replay da sessão com Update pendente, CPU intacta em falta e contexto copiado.
O conjunto gerado passa a **118 casos em x86_64** (102 nos demais hosts).

O registro passa a **5/69 serviços**, com variantes limitadas. Essa contagem
representa serviços registrados, não percentual de compatibilidade ou execução
real de todas as chamadas.

Aceite local: **288 testes de workspace/doctests**, **118 casos do Rust gerado
em debug e release** (release com `-D warnings`), Clippy global com `-D warnings`,
fmt, diff, links locais e loader do hello. Não houve mudança de mídia, entrada
ou bytes do XBE para contornar o bloqueio.

## Resultado integrado e próximo passo

O hello real foi traduzido novamente com `--entry 0x19110`: **8769 instruções/
blocos**, compilado e executado em debug. Passou por SHAInit e parou em
**ExQueryNonVolatileSetting (24)**, retorno reportado `0x19176`, argumentos:

```text
ValueIndex   = 0x0000FFFF
Type         = 0x7000FD78
Value        = 0x7000FD80
ValueLength  = 0x00000100
ResultLength = 0x7000FD7C
```

A consulta de EEPROM é o próximo serviço a modelar, com contrato, origem e
estado explícitos. Não preencher um buffer fictício e anunciar sucesso, nem
usar BIOS como requisito implícito. Update/Final não foram chamados pelo hello
até essa parada; a prova desses serviços vem das fixtures compiladas.
Threads, espera, execução integrada completa e framebuffer continuam pendentes.

Logs ignorados: `target/sha-focused.log`, `target/sha-workspace.log`,
`target/sha-release.log`, `target/sha-clippy.log`, `target/sha-ordinal.log`,
`target/sha-memory.log`, `target/sha-hello-translate.log`, `target/sha-hello-run.log`.
Projeto real: `target/alpha-hello-sha-20261020`. Nenhuma BIOS foi consultada.

## Referências públicas

- [Declarações nxdk fixadas, CC0](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/xboxkrnl/xboxkrnl.h).
- [init_rand_s/rand_s do nxdk](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/xboxrt/libc_extensions/stdlib_ext_.c).
- [RFC 3174, equações §6.1 e vetores §7.3](https://www.rfc-editor.org/rfc/rfc3174.html).
- [Implementação](../../crates/xbox-kernel/src/dispatch/sha.rs).
- [Testes do serviço](../../crates/xbox-kernel/tests/sha.rs) e [testes gerados](../../tools/xberecomp/tests/verification_sha.inc).
