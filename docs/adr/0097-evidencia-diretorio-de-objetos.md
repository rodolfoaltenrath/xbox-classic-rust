# ADR-0097: bloquear enumeração de objetos sem contrato de Type verificado

Data: 2026-10-27. Estado: investigação bloqueada, sem modelo novo. PLANO 12.3b.

## Problema concreto

O marco é implementar as importações do hello com comportamento testado. A
enumeração de objetos (NtQueryDirectoryObject, ordinal 208) não tem a mesma
estrutura ou assinatura do Windows desktop. Copiar o modelo NT preencheria um
campo diferente. Preencher com zero porque o consumidor atual não lê o campo
repetiria a classe de erro já discutida para XeImageFileName (ADR-0022).

## O que foi verificado

- nxdk 14d5ee97e73347c973f1f57b68b79ec08c9e77f2,
  [xboxkrnl.h](https://github.com/XboxDev/nxdk/blob/14d5ee97e73347c973f1f57b68b79ec08c9e77f2/lib/xboxkrnl/xboxkrnl.h):
  OBJECT_DIRECTORY_INFORMATION é OBJECT_STRING Name + ULONG Type, 12 bytes;
  Type está no offset 8. NtOpenDirectoryObject tem dois argumentos e
  NtQueryDirectoryObject tem seis; não há ReturnSingleEntry do NT desktop.
- lib/winapi/filemanip.c/GetLogicalDrives abre ObDosDevicesDirectory (-3) e
  consulta nome/contexto com buffer de 16 bytes. Não lê Type. Esse consumidor
  não permite inferir se o inteiro é índice, tag, ponteiro ou outro contrato.
- Fonte pública Cxbx-Reloaded, revisão
  585c49a50af1255ab155099e06f24505f9c5a800,
  [EmuKrnlNt.cpp](https://github.com/Cxbx-Reloaded/Cxbx-Reloaded/blob/585c49a50af1255ab155099e06f24505f9c5a800/src/core/kernel/exports/EmuKrnlNt.cpp):
  ordinal 208 é marcado LOG_UNIMPLEMENTED e retorna sucesso sem preencher o
  buffer. Não é evidência de conteúdo. Não foi copiado código dessa fonte.
- Consultas públicas ao XboxDevWiki e buscas por semântica do campo não
  forneceram referência verificável nesta investigação. Documentação do NT
  desktop usa outra estrutura (duas strings), que não resolve a diferença.

## Decisão

Não promover 201/208 a implementados com dados supostos, nem ocultar o problema
retornando diretório vazio quando D: existe. Manter recusa explícita e registrar
BLOQUEADO no PLANO, conforme AGENTS.md. A subentrega 12.3a permanece válida;
12.3 integral não pode ser marcada concluída. 69, 201, 208, 246 e 250 continuam
pendentes. As tarefas 12.4–12.6 não foram implementadas nesta investigação.

Não interpretar BIOS/kernel proprietário para deduzir o contrato. O caminho
de obtenção de evidência é comportamento observável ou documentação pública.

## Preparação reproduzível para obter evidência

homebrew/object-directory-probe contém sonda própria MIT, com assert de tamanho
e offset da ABI. O builder existente usa SDK fixado, build isolado e hashes.
Registra status, Context antes/depois, ReturnLength, Name.Length/MaximumLength,
Type cru e offset do buffer de nome. Não desreferencia ponteiro fora da saída.
Mede buffers completos e tamanhos 0/11/12/14, com reinício, sem supor drives.

```bash
bash scripts/build-nxdk-sample.sh object-directory-probe homebrew-object-directory-probe
```

O XBE compilado deve ser executado em Xbox ou ambiente de referência confiável,
registrando kernel, origem da mídia, hash do XBE e saída por vídeo/log. Um
resultado do próprio Obsidian não valida seu comportamento esperado. Não há
resultado de hardware nesta sessão; build bem sucedido não encerra o bloqueio.

Build WSL concluído nesta sessão com SDK fixado e assert de layout:
XBE SHA-256 `29fc0c3afbae6f7f55d6daf5669adfc39fcb451c7944c48af8e4e7be5295d852`;
PE `94d5b3ac84cd615633dd057c2c39d1d7a0c8add4c3a57a37de7731a19d57d4e8`.
Manifesto e log em target/homebrew-object-directory-probe. Artefatos ignorados;
somente Makefile, main.c e README da sonda entram no Git.

## Condição de saída

Obter referência verificável que identifique a semântica/valores de Type e o
contrato de buffer/contexto/tamanho retornado. Captura pode orientar a medição,
mas um inteiro isolado sem identificação não prova o contrato. Depois, modelar
o namespace real declarado, com ABI emitida, limites, snapshot/migração e gate.
Não há mudança de runtime nem novo formato de snapshot neste ADR.
