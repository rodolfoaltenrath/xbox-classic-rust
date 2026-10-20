# Changelog

Formato baseado em [Keep a Changelog](https://keepachangelog.com/pt-BR/1.1.0/).
Versionamento: SemVer 0.x — a API interna muda livremente até a 1.0.

Cada entrada deve referenciar a fase do [ROADMAP.md](ROADMAP.md) e, quando houver,
o ADR correspondente em [docs/adr/](docs/adr/).

## [Não publicado]

### Adicionado

- Alpha: build local do nxdk hello com revisão/submódulos fixados, manifesto e
  hashes; normalização dos timestamps e padding de metadata do cxbe. Novo
  `ordinal-report` inventaria TLS, ABI e pendências: 69 funções e 3 dados no alvo.
- Perfil inicial TLS nxdk, base FS explícita, LEA sem segmento e snapshot AOTC v3
  com migração v1/v2. Imports de funções recebem trampolins guest RX e saem por
  `Exit::KernelCall`; stdcall/cdecl/fastcall e retorno validados em Rust gerado.
  Serviços, dados exportados e threads continuam pendentes; o hello completo
  ainda não executa (ADR-0021).

- Marco 1/6 do alpha: loader compartilhado em `xbox-kernel`, com políticas de
  inspeção/AOT; `obsidian check-memory` valida XBE/pasta/ISO pelo bus sem executar
  código. T14 NFSU2 local confirma 14 seções, 152 ordinais e 4634697 bytes.
  Contratos T1–T10 passaram no Miri; 20000 cenários contra vetor independente,
  benchmark seguro, auditoria de unsafe e job Miri no CI (ADR-0020).
- Corrigidos os 19 avisos de Clippy de memória; preservado formato de snapshot.
  Arena recusa tamanho acima de isize::MAX. Posse exclusiva Send/não Sync documentada;
  proposta de aceitar data races do ADR-0011 substituída. Alvo nxdk hello fixado
  para as próximas entregas; ainda não há homebrew executando.

- Fase 3-AOT: CALL/JMP near de 32 bits por registrador/memória, incluindo SIB e
  aliases de ESP. Descoberta iterativa por constantes locais e ponteiros imutáveis;
  API `analyze_with_entries` e CLI `--entry` para callbacks/tabelas mutáveis.
  Destinos conhecidos são blocos explícitos, sem decodificação em execução.
  CALL/JMP/RET validam alvo e permissão antes de comprometer CPU/pilha; erros
  `UnknownTarget` incluem PC e destino. Orçamento, recursão e snapshots preservados
  (ADR-0019).

- Fase 3-AOT: JCXZ/JECXZ e LOOP/LOOPE/LOOPNE com CX/ECX; CLD/STD e
  MOVS/STOS/LODS/CMPS/SCAS de 8/16/32 bits, com endereços de 16/32 bits.
  REP/REPE/REPNE usam um elemento por passagem de bloco, respeitando orçamento
  e snapshot parcial. Comparações restauram flags originais em faltas; `AOTC` v2
  preserva a continuação e migra v1. 3456 comparações com strings na CPU física,
  mais testes de faults, wrap, DF, sobreposição e snapshots (ADR-0018).
- Fase 2: o bus valida a faixa inteira antes de escrever; uma página inválida,
  read-only ou região física inválida não deixa metade de um store na RAM.

**Fase 3-AOT — MUL/IMUL, DIV/IDIV e diagnóstico de #DE (ADR-0017)**
- Produtos e divisões de 8/16/32 bits com registradores implícitos; IMUL de dois e
  três operandos em 16/32 bits, com memória e imediatos com sinal.
- CBW/CWDE/CWD/CDQ. Flags indefinidas normalizadas para zero e partes superiores
  preservadas. Divisor zero e overflow do quociente retornam `#DE` recuperável com
  PC, sem comprometer CPU/FLAGS; entrega via IDT/kernel continua pendente.
- 483916 comparações físicas adicionais e 1000580 diagnósticos classificados de
  divisão. A integração executa 45 testes nativos, incluindo memória read-only,
  aliases, erros e snapshot entre produto e divisão.

**Fase 3-AOT — rotações e shifts duplos (ADR-0016)**
- ROL/ROR/RCL/RCR de 8/16/32 bits, com anel de carry de até 33 bits, contagens
  mascaradas, voltas completas e preservação de SF/ZF/AF/PF.
- SHLD/SHRD de 16/32 bits com fonte registrada e contagem imediata ou CL; contagens
  efetivas maiores que a largura retornam erro recuperável com PC antes de acesso
  à memória, pois o resultado é indefinido na ISA.
- Mais 791764 comparações físicas: 70488 rotações gerais, 524288 rotações de byte
  e 196988 shifts duplos definidos. A integração agora executa 35 testes nativos,
  incluindo aliases, memória, falhas e snapshot entre RCL/RCR/SHLD.

**Fase 3-AOT — SHL/SHR/SAR e flags indefinidas (ADR-0015)**
- Shifts de 8/16/32 bits com contagem um implícita, imediato ou CL, com contagem
  mascarada a cinco bits, sinal de SAR e preservação das flags em contagem zero.
- Flags indefinidas normalizadas para zero, com máscara de comparação independente
  no oráculo físico; memória mantém leitura/escrita mesmo com contagem efetiva zero.
- 52470 comparações físicas de shifts e 393216 casos exaustivos de byte. Testes
  adicionais de AH/CL/CX/ECX, memória, falhas e snapshot antes de ADC; a integração
  agora executa 24 testes nativos em x86_64.

**Fase 3-AOT — ADC/SBB e carry entre blocos (ADR-0014)**
- ADC/SBB de 8/16/32 bits na análise, IR e Rust gerado, com registradores parciais,
  memória e imediatos; CF de entrada participa do resultado e das flags.
- Oráculo físico ampliado: 9438 casos gerais de ALU e 262144 comparações exaustivas
  de ADC/SBB de byte; as 3872 verificações independentes de Jcc permanecem.
- Testes nativos de AH, imediatos com sinal, leitura/escrita de memória, falha de
  escrita sem comprometer FLAGS e pausa/snapshot entre palavras de uma soma/subtração.

**Repositório e publicação**
- README com estado atual, build, demonstração AOT e documentação.
- Metadados Apache-2.0 conforme a licença já escolhida no repositório do mantenedor;
  ADR-0007 atualizado. `publish = false` permanece para os pacotes Cargo.
- Exclusão explícita de `*.bin`, além de mídias, dados locais e builds já ignorados.

**Fase 3-AOT — ISA inteira ampliada e referência independente (ADR-0014)**
- Operandos de 8/16/32 bits, subregistradores incluindo AH, memória tipada,
  AND/OR, INC/DEC/NEG/NOT, MOVZX/MOVSX, LEA e as 16 condições Jcc curtas/near.
- PUSH/POP de 16/32 bits e LEAVE de 32 bits, com os casos especiais de ESP.
- Oráculo opcional em `xbox-cpu`: instruções fixas na CPU física, isolado dos
  módulos nativos. 7986 comparações ALU/flags e 3872 casos de Jcc no Rust compilado.
- Treze testes nativos na integração x86_64, mantendo as verificações de snapshot,
  memória e erros da primeira prova. Formato de snapshot preservado.
- CI de testes inclui `xbox-cpu/isa-oracle`. Validação local: 173 testes com
  warnings como erros; 13 testes do Rust gerado passaram em debug e release.

**Fase 3-AOT — primeira recompilação nativa em Rust (ADR-0013)**
- `xbox-recomp`: decoder Rust `iced-x86`, descoberta conservadora de fluxo, IR e
  emissão de blocos Rust para um subconjunto inteiro de x86-32. Instruções sem
  suporte e fluxo ambíguo são recusados com endereço.
- Runtime experimental com memória pelo `Bus`, CALL/RET na pilha do guest, orçamento
  por blocos e snapshot explícito de CPU. Não implementa TLS/kernel/GPU/áudio.
- `xberecomp demo` e `translate`: geram projeto Cargo local; `--run` compila e
  executa. Diretório não vazio é recusado.
- Fixture aberta com soma/loop/call/escrita; executável Rust resultante retorna
  EAX=62 após 37 instruções. Teste de integração compila e executa o Rust gerado,
  incluindo snapshot com call pendente, flags, endereçamento e erros de memória.
- Documentação de arquitetura e uso em `docs/arch/recompilacao.md` e
  `docs/dev/recomp.md`. Jogos comerciais ainda não executam.

**Documentação**
- `ROADMAP.md`: arquitetura completa, 12 fases, decisões fundamentais, protocolo de
  documentação.
- ADRs 0001–0008 formalizando as decisões de arquitetura (Fase 0).
- `docs/arch/core.md`, `docs/arch/formats.md`, `docs/dev/build.md`,
  `docs/dev/third-party.md`, `docs/README.md`.
- `docs/compat/formatos.md`: os 11 pontos dos formatos que ainda precisam de
  verificação contra arquivo real — a honestidade de que fixture gerada por código
  prova consistência, não conformidade.

**Fase 0 — fundação**
- Workspace Cargo com 16 pacotes: 11 crates de subsistema, o app `obsidian` e 4
  ferramentas de linha de comando. Cada crate esqueleto declara em `ROADMAP_PHASE` a
  fase que o implementa.
- Perfis de build (`release` com LTO fat e 1 codegen unit, `release-dbg` para
  perfilagem) e lints de workspace com `unsafe_code = "deny"`.
- `xbox-core`:
  - `error`: erros tipados, incluindo `NotImplemented { what, phase }`, que faz um
    caminho não escrito falhar citando a fase responsável em vez de entrar em pânico;
  - `snapshot`: formato binário com seções versionadas e trait `Snapshot` (ADR-0004),
    com testes de determinismo e de rejeição de dado truncado/corrompido;
  - `time`: relógio virtual único em ciclos do guest (733⅓ MHz) com derivação para
    contador de performance, tick de 1 ms, áudio e vblank; agendador de eventos por
    prazo, determinístico e serializável;
  - `config`: configuração TOML com defaults, `deny_unknown_fields` e validação;
  - `log`: `tracing` com filtro por subsistema via `OBSIDIAN_LOG`.
- `obsidian`: CLI com `--version`, `--help`, `ui`, `run`, `info` e `doctor`.
- CI (GitHub Actions): `fmt`, `clippy -D warnings`, testes, doctests, build release e
  compilação dos benchmarks, em Windows e Linux.
- Benchmarks com criterion em `crates/xbox-core/bench` (pacote fora do workspace).

**Fase 1 — formatos**
- `xbox-formats::bytes`: leitura limitada de inteiros e strings. Nenhum acesso direto a
  `&[u8]` no crate, para que arquivo truncado seja `Err` com posição exata, nunca
  pânico.
- `xbox-formats::xbe`: parser completo de executáveis — header, certificado de tamanho
  variável, seções (com validação antecipada de faixa), versões de biblioteca, TLS e
  logo RLE. Entry point e thunk table são desofuscados **por plausibilidade**: as três
  chaves conhecidas são testadas e vence a que aterra dentro da imagem; se nenhuma
  aterrar, o erro lista os candidatos.
- `xbox-formats::xdvdfs`: leitor do filesystem do DVD — descritor de volume, árvore
  binária de diretórios, leitura por caminho sem distinguir caixa, `walk` com ordem
  estável. Detecta a base da partição (imagem cortada vs. dump completo) e tem
  terminação garantida contra ciclos na árvore.
- `xbox-formats::fixtures`: construtores de XBE e de imagem XISO em código, incluindo
  variantes defeituosas (seção fora do arquivo, ciclo na árvore) para os testes
  pedirem exatamente o caso patológico que querem.
- `xbox-formats::identificar`: detecta o tipo de mídia pelos bytes, não pela extensão.
- `tests/robustez.rs`: ~24 mil entradas mutadas de forma determinística mais truncamento
  em todos os pontos, exigindo ausência de pânico e de laço infinito. Semente fixa, para
  que qualquer falha seja reproduzível.
- `fuzz/`: alvos `cargo-fuzz` para XBE e XDVDFS (nightly, fora do CI de todo push).
- `xbetool`: relatório completo de um `.xbe`, com extração de seção e desenho do logo em
  ASCII — que é como se verifica o decodificador de RLE a olho. Aceita `.xbe`, pasta com
  `default.xbe` ou imagem XISO (extrai o executável da imagem).
- `isotool`: `info`, `ls`, `tree` e `extract` sobre imagens XISO, com `--base` para
  forçar a base da partição.
- Exemplo `gerar_fixtures`: escreve as fixtures em arquivos para teste manual das
  ferramentas.
- `xbox-formats::media`: origem de bytes como fatia em memória **ou arquivo lido sob
  demanda**, devolvendo `Cow` para manter zero-copy onde dá. Motivo: imagem de jogo tem
  de 2 a 7 GB, e carregar o arquivo inteiro para ler um setor é inviável — tanto na
  ferramenta quanto, depois, no `NtReadFile` do kernel HLE.
- `Xdvdfs::read_range`: leitura parcial de arquivo, que é a forma como o jogo lê mídia
  durante a gameplay.
- `Xbe::va_to_offset`: traduz endereço virtual em deslocamento no arquivo, olhando a
  região de headers **e** as seções.
- `Xbe::kernel_imports` e `xbetool --thunks`: lista os ordinais do kernel que o título
  importa. É o inventário exato do que a Fase 4 precisa implementar para um jogo rodar
  (o disco verificado importa 152).
- `Header::extended`: guarda as palavras do header além de `0x178`, que existem em XBE
  de SDK novo (`size_of_image_header = 0x184` no disco verificado).
- `docs/compat/formatos.md`: resultado da verificação contra um disco de varejo real —
  7 dos 11 itens de conformidade fechados, 2 itens novos descobertos e fechados, e o
  limite deliberado de não examinar o código de biblioteca da Microsoft dentro do XBE.
- `xbox-formats::fatx`: leitor do filesystem do HD e dos memory units. Superbloco, FAT
  de 16 ou 32 bits, cadeia de clusters com terminação garantida contra ciclo, leitura
  parcial, e a tabela de partições padrão do HD do Xbox.
- `xbox-formats::eeprom`: os 256 bytes de configuração do console — serial, MAC, padrão
  de vídeo, idioma, flags de vídeo e áudio, região de DVD. `Eeprom::sintetica` gera uma
  coerente, para que ninguém precise de dump de console para jogar. O bloco cifrado é
  preservado byte a byte e não é decifrado: precisa de RC4/SHA-1/HMAC, que chegam com
  os `Xc*` da Fase 4.
- `xbox-formats::vfs`: as letras de drive que o jogo vê (`D:`, `T:`, `U:`, `Z:`, `C:`,
  `E:`, `X:`, `Y:`) sobre imagem XDVDFS, partição FATX ou pasta do host, com tradução
  dos caminhos de dispositivo (`\Device\CdRom0\...`, `\??\D:\...`) e **bloqueio de
  travessia de diretório em duas camadas**.
- `obsidian info` e `obsidian run` passaram a funcionar sobre mídia real: abrem a
  imagem, montam os drives do console, leem e parseiam o executável e contam os
  ordinais do kernel necessários. `run` ainda falha ao executar — citando as Fases 3 a
  5 — mas só depois de provar que todo o resto está no lugar.
- Alvos de fuzz para FATX e EEPROM; testes de robustez cobrindo mutação de partição
  FATX, EEPROM de conteúdo arbitrário e caminhos de VFS aleatórios.

### Alterado
- `ROADMAP.md` §4.6.1: esclarece que Vulkan e SPIR-V são a API de render desde a
  **Fase 5**, nos dois SOs — a Fase 10 troca apenas a *apresentação*
  (`winit` → `VK_KHR_display` no KMS), não a API nem os shaders. Decidido fixar o
  backend Vulkan do wgpu também no Windows, com DX12 apenas como reserva.
- `ROADMAP.md`: Fase 0 concluída; Fase 1 parcial (XBE e XDVDFS prontos). A infra de
  fuzz saiu da Fase 0 para a Fase 1, onde existem parsers para fuzzar.
- `clap` e `tracing-subscriber` passam a ser usados sem as features de cor
  (`color`/`ansi`) e os benchmarks saíram do workspace — contorno de toolchain
  documentado no **ADR-0008**, com prazo de validade antes da Fase 5.

### Corrigido
- `Clock::ms_to_cycles`/`us_to_cycles` arredondam para **cima**. Com arredondamento
  para baixo, uma espera de 16 ms vencia em 15,999 ms de tempo emulado — o guest veria
  um timer acordar antes do pedido. Tempo decorrido continua arredondando para baixo.
- `xbetool`: o rótulo de tamanho dizia "tamanho do arquivo" mesmo quando o `.xbe` tinha
  sido extraído de uma imagem, mostrando o tamanho do executável ao lado do caminho da
  imagem.
- **TLS perdido em jogos reais.** `Xbe::parse` só resolvia o diretório de TLS se ele
  estivesse na região de headers, e devolvia `None` caso contrário. Num XBE retail de
  verdade o TLS fica **dentro de `.rdata`** — ou seja, o caso que importa era
  justamente o que falhava, e sem TLS o loader da Fase 4 não inicializa as threads do
  título. Agora a tradução de endereço virtual considera headers e seções.
  Encontrado ao rodar o parser contra um disco de varejo.

### Adicionado (revisão da Fase 2)
- `docs/arch/memoria.md`: revisão arquitetural de memória, MMU, MMIO e fastmem **antes**
  da implementação, com medição no hardware real e 13 seções cobrindo taxonomia de
  endereços, mapa físico vs. janelas virtuais, API única para CPU/GPU/kernel, plano de
  testes, preparação para Vulkan e limitações conhecidas.
- ADR-0009: cinco espaços de endereçamento com newtypes e nenhuma conversão implícita.
- ADR-0010: **revisa o ADR-0005** — caminho seguro na Fase 2, fastmem na Fase 3b,
  medido contra o JIT.
- ADR-0011: arena como objeto de memória compartilhável, e a corrida CPU↔GPU como
  desvio consciente e documentado do modelo de memória do Rust.
- ADR-0012: modelo de sandbox das pastas montadas; o que está fechado e o que fica para
  a Fase 7.
- `tests/sandbox.rs`: cinco testes do sandbox da VFS, incluindo fuga por atalho de
  diretório, que é exercitável neste host mesmo sem privilégio de link simbólico.

### Corrigido (segurança)
- **Fuga da pasta montada por link simbólico.** A VFS recusava `..` e conferia o prefixo
  do caminho, mas um link dentro da pasta apontando para fora passava pelas duas
  barreiras — nenhum componente é `..` e o caminho está sob a raiz. A resolução agora
  canonicaliza caminho e raiz e exige que o resultado continue debaixo da raiz.
  Verificado com *junction* de diretório, que não exige privilégio no Windows.
  A corrida entre canonicalizar e abrir **continua aberta** e está agendada para a
  Fase 7 (ADR-0012).

### Alterado (revisão da Fase 2)
- Fase 2 do ROADMAP reescrita: fastmem fora de escopo, newtypes de endereço, arena
  compartilhável, Miri no caminho completo e um teste de integração (T14) que prova a
  leitura dos 152 ordinais do jogo real **através do bus**.
- ADR-0005 marcado como revisado pelo ADR-0010.
- ADR-0008 atualizado: declarar funções do Win32 à mão linka no toolchain atual sem
  `windows-sys`, então `xbox-mem` e `xbox-platform` não dependem dele. O prazo passa a
  valer só para a Fase 5, por causa do `wgpu`.

### Alterado (continuação)
- `Xdvdfs` passou a receber um `Media` em vez de `&[u8]`, e `read` devolve `Cow`.
  `isotool` e `xbetool` leem sob demanda: `isotool info` numa imagem de 2,87 GB passou
  a rodar em **0,4 s** sem carregar a imagem na memória.

### Performance
- Base de medição criada (criterion: agendador, conversões de relógio, snapshot).
  Ainda não há número a comparar.

### Removido
-
