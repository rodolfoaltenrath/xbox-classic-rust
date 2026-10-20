# ADR-0034 — Consulta integral da EEPROM da sessão

Data: 2026-10-20. Estado: aceito para leitura XC_MAX_ALL no perfil HLE atual.

## Necessidade observada

Após SHAInit, o hello real chegou a `ExQueryNonVolatileSetting` (24), retorno
`0x19176`, argumentos `0xFFFF, 0x7000FD78, 0x7000FD80, 0x100, 0x7000FD7C`.
O caminho público `init_rand_s` consulta 256 bytes e os passa a SHAUpdate.
Implementar a leitura de um estado definido de console, sem preencher uma saída
arbitrária para avançar e sem consultar BIOS.

## Origem e estado da imagem

Reutilizar `xbox_formats::eeprom::Eeprom::sintetica`, que já existia no projeto.
`EepromState` mantém **256 bytes autoritativos** e a origem explícita:

- `SyntheticV1`: gerador existente, serial `OBSIDIAN0001`, MAC local,
  configuração e checksums do perfil sintético. O bloco de segurança está em
  claro; não é uma EEPROM válida para hardware físico.
- `Supplied`: bytes fornecidos explicitamente por API pela aplicação; preservar
  cada byte, sem parse/recalcular checksums/decifrar ou ler um arquivo do host.

`Dispatcher::new` usa **NTSC-M/português**, perfil AOT inicial fixo.
Isso não conecta automaticamente `SystemConfig`, CLI, VFS ou persistência.
`Dispatcher::with_eeprom(EepromState)` permite escolher uma imagem sintética ou
fornecida antes de executar. Nenhum dump ou BIOS é necessário para o padrão.

Os bytes são imutáveis nesta etapa; a chamada retorna cópia em RAM guest.
Alterar essa cópia não modifica a EEPROM. `ExSaveNonVolatileSetting` continua
pendente; não existe escrita em disco nem requisito implícito de arquivo local.
Os limites de conformidade dos offsets/checksums do gerador continuam registrados
no guia de formatos. Consultar bytes crus não prova cifragem, identidade de um
console físico, entropia ou autenticação.

## Contrato atendido e políticas locais

A assinatura pública nxdk é stdcall/20: índice, Type, Value, ValueLength e
ResultLength. Atender somente `XC_MAX_ALL = 0xFFFF`, a consulta observada.
Outros índices devolvem erro de **variante não implementada**, com contexto de
chamada, sem escrever saídas. Não representar falta de implementação como
sucesso nem anunciar suporte a todos os índices pelo fato de haver catálogo.

Com capacidade de pelo menos 256 bytes, publicar a cópia, `Type = REG_BINARY`
(3) e `ResultLength = 256`, retornando `STATUS_SUCCESS`. Capacidade maior não
aumenta o tamanho copiado nem a faixa validada. As saídas do perfil exigem
ponteiros não nulos para RAM guest gravável; proteção, página ausente, wrap ou
MMIO devolvem `STATUS_ACCESS_VIOLATION` sem qualquer saída parcial.

Para buffer curto, adotar a política HLE de escrever **somente ResultLength**
com 256 e retornar `STATUS_BUFFER_TOO_SMALL` (`0xC0000023`). Type/Value não são
acessados nesse caminho e podem ser nulos. ResultLength é obrigatório no perfil;
se inválido, retornar access violation sem efeitos. Essa convenção de sondagem
e a precedência de validação são políticas locais, não testes do kernel físico.
As declarações públicas disponíveis não especificam todos esses casos limite.

Antes da primeira escrita de sucesso, validar as três faixas inteiras. O bus
fica sob posse exclusiva durante a operação; nenhum mapeamento muda entre
preflight e stores. Para aliases de saídas, a ordem local explícita é **Value,
Type, ResultLength**. Não prometer ordem byte a byte igual à do kernel físico.

NTSTATUS de falha é chamada atendida e retorna ao guest normalmente. Variante
não modelada mantém a CPU pendente, conforme o dispatcher. A fonte única de
serviços passa a **6/69**, sem representar percentual de compatibilidade.

## Snapshot e continuidade

`KRNL` passa a **v2**, acrescentando após `KMUT` uma seção `EEPR` v1 com tag da
origem e 256 bytes crus. O carregamento valida versão, origem e truncamento,
sem alocação de tamanho controlado pelo snapshot. Os bytes restaurados são
autoritativos; não regenerar uma EEPROM com os padrões/configuração do destino.

`KRNL` v1 não tinha imagem de EEPROM: migrar explicitamente para o perfil
sintético padrão NTSC-M/português, preservando contadores e mutexes. Não inventar
uma imagem antiga a partir da RAM guest. `AOTS` continua v1, `KMUT` v1 e CPU
`AOTC` v6. Sessões com erro de carga devem ser descartadas.

## Validação

Testes de serviço cobrem a origem sintética e bytes fornecidos, consulta/cópia,
guardas, capacidade maior, sondagem, índices pendentes, todas as saídas inválidas,
MMIO, falta/proteção tardia sem stores parciais, aliases, snapshot determinístico,
migração v1 e rejeição de origem/versão/truncamento.

Três casos adicionais compilam o Rust emitido: consulta seguida de SHA-1 sobre
bytes fornecidos `00..FF` (digest independente calculado com .NET), restauração
da sessão em uma chamada pendente com origem/bytes diferentes do padrão do
destino, NTSTATUS de erro sem cópia parcial e índice não modelado com CPU intacta.
O conjunto gerado passa a **121 casos em x86_64** (105 nos demais hosts).

Aceite final: **301 testes workspace/doctests**, **121 casos gerados em debug e
release** (release com `-D warnings`), Clippy global com `-D warnings`, fmt, diff,
links locais e check-memory do hello passaram. O relatório deriva **6/69** da
fonte real; não é contagem de todas as chamadas executadas.

## Resultado integrado e próximo passo

O hello real foi novamente traduzido (8769 instruções/blocos com `--entry
0x19110`), compilado e executado em debug. Passou pela consulta da EEPROM e
parou em **NtWaitForSingleObject (233)**, retorno reportado `0x16B99`, argumentos
`0x00000004, 0x00000000, 0x00000000`. O fonte público `mtx_lock.c` usa essa
chamada com o handle do mutex, `FALSE` e timeout nulo.

Próximo trabalho: aquisição/espera real para o mutex existente, incluindo
propriedade, recursão e eventual bloqueio guest. Não assumir que todos os casos
são imediatos, não bloquear o host e não devolver sucesso genérico. KMUT v1 só
aceita estados iniciais 0/1; ampliar/versionar ao adicionar estados de espera/
recursão. Threads, execução integrada completa e framebuffer permanecem pendentes.

Logs ignorados: `target/eeprom-focused.log`, `target/eeprom-workspace.log`,
`target/eeprom-release.log`, `target/eeprom-clippy.log`, `target/eeprom-ordinal.log`,
`target/eeprom-memory.log`, `target/eeprom-hello-translate.log` e
`target/eeprom-hello-run.log`. Projeto real: `target/alpha-hello-eeprom-20261020`.
Nenhuma BIOS, dump de console ou mídia comercial foi consultada.

## Referências

- [Declarações e XC_MAX_ALL do nxdk fixado, CC0](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/xboxkrnl/xboxkrnl.h).
- [NTSTATUS públicos](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/xboxkrnl/ntstatus.h).
- [Caminho init_rand_s](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/xboxrt/libc_extensions/stdlib_ext_.c).
- [Gerador existente](../../crates/xbox-formats/src/eeprom.rs) e [limites de conformidade](../compat/formatos.md).
- [Estado e serviço](../../crates/xbox-kernel/src/dispatch/nvram.rs).
- [Testes do serviço](../../crates/xbox-kernel/tests/nvram.rs) e [testes nativos](../../tools/xberecomp/tests/verification_nvram.inc).
