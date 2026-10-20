# Revisão arquitetural — memória, MMU, MMIO e fastmem

> **Status:** aceite local da Fase 2 concluído em 2026-10-20 (ADR-0020).
> **Data:** 2026-10-19
> **Escopo:** Fase 2 do [ROADMAP](../../ROADMAP.md).
> **Gatilho:** revisão externa pedindo validação independente antes de escrever código.

> **Implementação posterior (2026-10-20):** o bus seguro está em uso pelo AOT;
> o T14 real, os contratos sob Miri e o benchmark estão em `docs/dev/memoria.md`. `Bus::escrever_bytes` valida
> todas as páginas/regiões antes de executar a escrita, para evitar alteração parcial
> de RAM quando a segunda página falha. Leituras e telemetria mantêm os limites
> anteriores. A motivação e os testes estão no [ADR-0018](../adr/0018-strings-rep-retomaveis.md).

Este documento é o portão da Fase 2. Ele revisa o que o ROADMAP planejava, **corrige
duas decisões com base em medição** e fixa a API que CPU, GPU e kernel vão usar.

**Ordenação AOT posterior (ADR-0028):** SFENCE chama `Bus::ordenar_escritas`.
RAM é aplicada sincronicamente e a arena permanece exclusiva; visões virtual,
física e DMA observam os mesmos bytes, sem store buffer pendente. A barreira
atual não precisa executar código adicional. Escrita diferida, write-combining
ou DMA concorrente exigem ampliar esse protocolo antes de serem habilitados;
uma fence do host não substitui a publicação guest nem a conclusão da GPU.
Detalhes: [ADR-0028](../adr/0028-sfence-ordenacao-do-bus.md).

Medições feitas no host do mantenedor (x86-64, 8 núcleos lógicos, Windows 11,
`rustc 1.98.1`, perfil `release` com LTO fat). A sonda está em
`scratchpad/mem-probe` e é reproduzível.

---

## Sumário das conclusões

| # | Conclusão | Base |
|---|---|---|
| C1 | Cinco espaços de endereçamento distintos, com newtypes e **nenhuma** conversão implícita | análise |
| C2 | O mapa do ROADMAP §3.2 **mistura endereço físico com janela virtual**; corrigido aqui | análise |
| C3 | **Fastmem rende 3,4× no regime de cache, 1,04× quando o acesso vai à RAM** — não é o ganho que o ADR-0005 supunha | medido |
| C4 | **MMIO por falha de página é inviável no Windows:** 11,5 µs por falha, ~86 falhas por milissegundo | medido |
| C5 | Logo: **Fase 2 entrega só o caminho seguro**; fastmem vira trabalho da Fase 3b, medido contra o JIT | decorre de C3 e C4 |
| C6 | A arena precisa nascer como objeto de memória **compartilhável**, ou a Fase 5 não consegue importá-la no Vulkan | análise |
| C7 | Despacho por ponteiro de função custa +2%: abstração não é o gargalo, não vale contorcer a API por isso | medido |
| C8 | Win32 declarado à mão linka no toolchain atual — a dívida do ADR-0008 é menor do que parecia | medido |

---

## 1. Os cinco espaços de endereçamento

Este é o requisito que mais previne retrabalho, porque confundir dois deles produz bugs
que só aparecem três fases depois, dentro da GPU, como "textura errada".

| Tipo | O que é | Quem usa | Exemplo real (NFSU2) |
|---|---|---|---|
| `GuestVirt(u32)` | endereço virtual do guest, o que o código x86 do título manipula | CPU, loader, kernel | entry point `0x0021B1CE` |
| `GuestPhys(u32)` | endereço físico do Xbox: RAM abaixo de `0x04000000`, dispositivos acima de `0xFD000000` | kernel, GPU (DMA), bus | o que `MmGetPhysicalAddress` devolve |
| `ArenaOff(u32)` | deslocamento dentro da arena de 64 MiB | só `xbox-mem` | — |
| `HostPtr` (`*mut u8`) | endereço do processo host | **só** dentro de `xbox-mem` | `0x1d0e66c0000` na sonda |
| `FileOff(usize)` | deslocamento dentro de um arquivo | `xbox-formats`, loader | `Xbe::va_to_offset` |

### Por que `GuestVirt` e `GuestPhys` não podem ser o mesmo tipo

Não é purismo; é o caminho que o D3D8 do jogo usa, e o NFSU2 tem a seção `D3D` dentro
do executável:

1. o jogo chama `MmAllocateContiguousMemory` e recebe um **virtual** na janela
   `0x80000000`;
2. chama `MmGetPhysicalAddress` nesse ponteiro e recebe um **físico**;
3. escreve esse físico dentro de um objeto DMA do NV2A;
4. a GPU lê **por endereço físico**, sem passar pela MMU.

Se os dois forem `u32`, o passo 3 aceita silenciosamente um virtual e a GPU lê o lugar
errado. Com newtypes, isso não compila.

### Regras de conversão — são estas e só estas

```text
GuestVirt ──Mmu::traduzir(va, acesso)──► Result<GuestPhys, Falta>
GuestPhys ──Bus::classificar(pa)───────► Regiao::{Ram(ArenaOff), Dispositivo{..}, Buraco}
ArenaOff  ──Arena::ponteiro(off)───────► HostPtr        (só dentro de xbox-mem, unsafe)
GuestVirt ──Xbe::va_to_offset(va)──────► Option<FileOff> (já implementado, Fase 1)
```

Nenhum `as`, nenhum `From`, nenhuma aritmética de janela escondida dentro do bus. A
janela contígua `0x80000000` **não** é um `+ 0x80000000` no código do bus: é uma
entrada de tabela de páginas que nós montamos no boot, e a tradução passa pelo mesmo
caminho de qualquer outro endereço. Um caso especial aqui seria uma mentira que a
Fase 4 pagaria.

---

## 2. O mapa de memória, com físico e virtual separados

### Correção ao ROADMAP §3.2

A tabela do ROADMAP lista, numa coluna só, `0x00000000` (físico), `0x80000000`
("mapeamento contíguo") e `0xF0000000` ("memória tiled"). Os dois últimos **não são
endereços físicos** — são janelas virtuais que o kernel cria na tabela de páginas.
Misturar as duas coisas num mapa é exatamente o erro que o item 1 quer evitar.

### Espaço físico — o que `GuestPhys` endereça

| Faixa | Conteúdo | Tratamento na Fase 2 |
|---|---|---|
| `0x00000000`–`0x03FFFFFF` | RAM, 64 MiB | arena |
| `0x04000000`–`0x07FFFFFF` | RAM extra de dev kit | arena, só com feature `ram128` |
| `0xFD000000`–`0xFDFFFFFF` | NV2A (PMC, PFIFO, PGRAPH, PCRTC, PRAMDAC, PTIMER, PRAMIN) | dispositivo stub com telemetria |
| `0xFE800000`–`0xFE87FFFF` | APU | dispositivo stub |
| `0xFEC00000`–`0xFEC00FFF` | AC97 | dispositivo stub |
| `0xFED00000`–`0xFED0FFFF` | USB OHCI 0 e 1 | dispositivo stub |
| `0xFEF00000`–`0xFEF003FF` | NIC nForce | dispositivo stub |
| `0xFF000000`–`0xFFFFFFFF` | flash ROM, espelhada | leitura devolve 0xFF; não bootamos BIOS (ADR-0001) |
| resto | buraco | acesso gera falha + telemetria |

### Janelas virtuais — o que o kernel monta na tabela de páginas

| Faixa virtual | Aponta para | Natureza |
|---|---|---|
| `0x00010000`+ | páginas físicas arbitrárias | imagem do título e heap |
| `0x80000000`–`0x83FFFFFF` | físico `0x00000000`+ | janela contígua, 1:1, cacheada |
| `0xF0000000`–`0xF3FFFFFF` | físico `0x00000000`+ | janela **tiled** |
| `0xFD000000`+ | MMIO | identidade |

> ⚠️ **Todas as faixas acima vêm de documentação pública e não foram validadas contra
> hardware.** Não há como validá-las com a ISO do usuário: endereço de MMIO só se
> confirma com código rodando. A validação vem na Fase 3/4, pelo mecanismo do item 6:
> todo acesso a região desconhecida é contado e logado com o PC do chamador.

### A janela tiled não é um espelho

`0xF0000000` aparece em documentação como "memória tiled", e a tentação é implementá-la
como terceiro alias da arena. **Não é.** Tiling é uma *transformação de endereço* feita
pela GPU para localidade bidimensional: o byte em `0xF0001000` não é o mesmo byte que
`0x00001000`. Tratar como alias simples corromperia textura de forma difícil de
diagnosticar.

Decisão: na Fase 2 a janela tiled é **região reconhecida mas não implementada** — acesso
a ela gera falha com mensagem explícita e entra na telemetria. A transformação correta
é trabalho da Fase 5, junto com o resto do NV2A, que é quem define o formato de tiling.

---

## 3. Uma abstração de memória, três visões

```rust
// xbox-mem
pub struct Arena { /* 64 MiB em objeto compartilhável; ver §6 */ }
pub struct Mmu   { /* tabelas de página do guest, TLB de tradução */ }
pub struct Bus   { arena: Arena, mmu: Mmu, dispositivos: TabelaDeRegioes }

impl Bus {
    // ── visão da CPU: passa pela MMU ──
    pub fn ler_u8  (&self, va: GuestVirt) -> Result<u8,  Falta>;
    pub fn ler_u16 (&self, va: GuestVirt) -> Result<u16, Falta>;
    pub fn ler_u32 (&self, va: GuestVirt) -> Result<u32, Falta>;
    pub fn escrever_u32(&self, va: GuestVirt, v: u32) -> Result<(), Falta>;

    // ── visão do kernel HLE: já conhece o físico ──
    pub fn traduzir(&self, va: GuestVirt, acesso: Acesso) -> Result<GuestPhys, Falta>;
    pub fn ler_fisico (&self, pa: GuestPhys, dest: &mut [u8]) -> Result<(), Falta>;
    pub fn mapear(&self, va: GuestVirt, pa: GuestPhys, paginas: u32, perm: Perm) -> Result<(), Falta>;

    // ── visão da GPU: DMA por endereço físico, fatia contígua ──
    pub fn dma(&self, pa: GuestPhys, len: u32) -> Result<&[u8], Falta>;

    // ── diagnóstico ──
    pub fn regioes_desconhecidas(&self) -> Vec<(GuestPhys, u64)>;
}
```

Três visões, **uma** implementação. Pontos que isso resolve de antemão:

- a CPU nunca toca a arena direto, então não existe um segundo caminho de acesso para
  divergir do primeiro;
- o kernel não reimplementa tradução: ele chama `traduzir`, igual à CPU;
- a GPU recebe `GuestPhys` e não tem como passar um virtual por engano;
- `Falta` carrega o que a CPU precisa para levantar `#PF` com `CR2` e código de erro
  corretos — a exceção não é inventada no meio do caminho.

`Acesso` é `{ Leitura, Escrita, Execucao }`, porque permissão de página depende disso e
o JIT precisa saber se uma página é executável para decidir invalidação.

---

## 4. Fastmem: o que a medição mostrou

O ADR-0005 afirmava que o acesso seguro custa "~15 instruções" contra "~3" do fastmem, e
concluía que fastmem deveria ser o caminho padrão. **Medi, e a afirmação não se
sustenta como escrita.**

### Custo por acesso de 32 bits, por regime de localidade

50 milhões de acessos por medição, endereços pseudoaleatórios dentro de uma janela:

| Working set | direto | + limites | + MMU e limites | custo do caminho seguro |
|---|---|---|---|---|
| 32 KiB (L1) | 0,60 ns | 2,07 ns | 2,34 ns | **+292%** |
| 256 KiB (L2) | 1,29 ns | 2,51 ns | 3,14 ns | +144% |
| 8 MiB (L3) | 12,47 ns | 13,15 ns | 17,98 ns | +44% |
| 64 MiB (RAM) | 21,26 ns | 22,16 ns | 22,11 ns | **+4%** |
| sequencial | 0,68 ns | 2,11 ns | 2,50 ns | +268% |

Leitura correta destes números:

- **a checagem não é grátis**: no regime de cache ela triplica o custo do acesso;
- **mas ela desaparece quando o acesso falha o cache**: a 21 ns de latência de RAM, as
  duas ou três instruções extras ficam escondidas;
- a primeira versão da sonda usou acesso aleatório em 64 MiB e deu empate, o que
  **quase me levou à conclusão errada** de que fastmem é inútil. Era só benchmark ruim:
  working set de 64 MiB mede latência de memória, não custo de checagem.

O que decide, então, é qual fração dos acessos do guest fica em cache. Pilha, variáveis
locais e estruturas quentes de um jogo ficam; streaming de textura e varredura de
geometria não. Sem o JIT existindo, **não tenho esse número** — e é por isso que a
decisão muda (§5).

Nota metodológica: as variantes seguras pagam uma chamada de função (`#[inline(never)]`),
o que é realista para slowmem chamado de dentro do JIT, mas superestima o custo para um
interpretador, que inlinaria. Parte dos ~1,4 ns de diferença é chamada, não checagem.

### Custo de uma falha de página tratada

| Operação | Custo |
|---|---|
| escrita normal em página gravável | 0,3 ns |
| `VirtualProtect` (uma chamada) | 793 ns |
| ciclo completo: proteger → falhar → VEH → liberar → repetir instrução | 13.148 ns |
| falha + manipulador + retomada, descontados os dois `VirtualProtect` | **11.562 ns** |

Isso é **38 mil vezes** o custo de uma escrita normal. A 60 fps, um milissegundo de
orçamento paga **86 falhas por frame**.

Consequência direta: **MMIO por falha de página é inviável no Windows.** Um título que
toque registrador do NV2A algumas centenas de vezes por frame gastaria milissegundos
só em despacho de exceção. O VEH do Windows é notoriamente caro; no Linux, `SIGSEGV`
costuma custar de 1 a 3 µs, o que ainda não é barato o suficiente para o caminho comum.

Onde falha de página **continua** sendo a ferramenta certa: eventos raros e de primeira
ocorrência — marcar página de código como suja para detectar código automodificável
(acontece uma vez por página até a próxima invalidação), e página-guarda como rede de
segurança. Não para MMIO, e com cuidado para invalidação de textura na Fase 5.

---

## 5. Decisão revisada: a Fase 2 entrega o caminho seguro

Em vez de implementar fastmem agora, como o ROADMAP previa:

1. **Fase 2 entrega apenas o caminho seguro** — arena, MMU, bus, classificação de
   região, dispositivos stub, snapshot. Código seguro, com um núcleo mínimo de `unsafe`
   confinado à arena.
2. **Fastmem vira item da Fase 3b**, junto do JIT, porque é lá que (a) o número que
   falta — fração de acessos que ficam em cache — passa a ser medível, e (b) o ganho
   real aparece: o JIT emitir um `mov` em vez de montar argumentos e chamar um helper.
3. **A API já nasce pronta para os dois**: o `Bus` é o mesmo; o que muda é a
   implementação por baixo e o código que o JIT emite. Nenhuma assinatura pública muda
   quando o fastmem entrar.
4. **Os dois caminhos ficam obrigados a passar a mesma suíte**, com um modo diferencial
   que roda o mesmo traço de acessos nos dois e compara byte a byte — igual ao que o
   ADR-0003 faz entre interpretador e JIT.

Isto não é abandonar o fastmem: é deixar de construir, sem medição, a parte mais
perigosa do subsistema — a que mistura `unsafe`, manipulador de sinal e recuperação de
falha dentro de código gerado. O ADR-0005 passa a ter status "revisado pelo ADR-0010".

Ganho colateral: a Fase 2 fica quase toda em Rust seguro, o que torna o requisito 5
(auditoria de `unsafe`) muito mais fácil de cumprir de verdade.

---

## 6. Isolamento de `unsafe` e o argumento de segurança

### Onde o `unsafe` vive

Só em `xbox-mem`, e dentro dele só em dois lugares:

**Correção da previsão:** a implementação tem 16 blocos em `arena.rs`, cobrindo
alocação, fatias, FFI e destruição, e um impl Send. O inventário vigente está em
[memoria-unsafe.md](memoria-unsafe.md). A tabela abaixo descreve a ideia original,
não uma contagem dos blocos entregues. Não há API pública de ponteiro de RAM.

| Local | O que faz | Invariante |
|---|---|---|
| `Arena::novo` | cria o objeto de memória compartilhável e mapeia | o mapeamento tem exatamente `RAM_SIZE` bytes e é RW |
| `Arena::ponteiro(ArenaOff)` | devolve `HostPtr` para um deslocamento | `off < self.len`, verificado **antes**, em código seguro |

Todo o resto — MMU, classificação de região, despacho de dispositivo, snapshot — é
código seguro. A API pública de `xbox-mem` não expõe ponteiro nem `unsafe`.

### O teorema de segurança do fastmem, para quando ele chegar

Vale registrar agora porque é o que torna o fastmem auditável:

> A reserva tem 4 GiB. O endereço do guest é `u32`. Logo
> `base + endereço` ∈ `[base, base + 2³²)` ⊆ reserva, **para todo** endereço possível.
> Não existe endereço de guest que escape da reserva.

A sonda confirmou as duas premissas no host real: reserva de 4 GiB em 5,4 µs, e
`base + 0xFFFFFFFF` dentro dela. O que o guest pode fazer de pior é acertar uma página
não commitada dentro da própria reserva — que é falha tratada, não corrupção do host.

### Manipulador de falha: o desenho que evita decodificar instrução

Quando o fastmem chegar, o manipulador **não** vai decodificar a instrução que falhou.
Em vez disso, o JIT mantém uma tabela de recuperação `PC do host → (registrador, tamanho,
tipo de acesso)` para cada acesso que emite. O manipulador consulta a tabela, nunca
aloca, nunca trava mutex e nunca chama função não reentrante. Decodificar x86 dentro de
um manipulador de sinal seria código complexo rodando no contexto mais hostil possível.

### Plano de auditoria

- `clippy::undocumented_unsafe_blocks` ligado em `xbox-mem`: bloco sem `// SAFETY:` não
  compila;
- **Miri** no caminho seguro completo (arena, MMU, bus, snapshot) no CI noturno. Miri
  não roda fastmem nem manipulador de sinal, e é exatamente por isso que a Fase 2 ser
  toda segura tem valor: ela fica 100% verificável por Miri;
- contagem de blocos `unsafe` no README do crate, revisada a cada PR que mexa neles;
- um documento `docs/arch/memoria-unsafe.md` listando cada bloco, seu invariante e o
  teste que o exercita.

### A ameaça concreta

Um XBE malformado ou malicioso não pode corromper memória do host. As barreiras, em
ordem: (1) o loader valida faixas de seção antes de mapear — já implementado e testado
na Fase 1; (2) todo endereço do guest é `u32` e toda tradução devolve `Result`; (3) o
acesso final é indexação verificada em `&[u8]` no caminho seguro, ou está coberto pelo
teorema acima no fastmem; (4) nenhuma dessas rotinas confia em tamanho declarado pelo
arquivo sem conferir contra o tamanho real.

---

## 7. Plano de testes

Não considero a fase concluída por testes passarem, mas sem estes ela nem começa a ser
avaliada.

| # | Família | O que exercita |
|---|---|---|
| T1 | Limites | acesso em `len-1`, `len`, `len+1`; leitura de 4 bytes começando em `len-1` |
| T2 | Overflow de endereço | `0xFFFFFFFF` + 4; `GuestPhys` máximo; `u64` de `base + off` sem wrap |
| T3 | Alinhamento | `u16`/`u32`/`u64` desalinhados, inclusive atravessando página e região |
| T4 | Permissões | escrita em página somente-leitura; execução em página não executável; cada um levanta `Falta` com o código certo |
| T5 | Janelas virtuais | escrita em `0x00001000` visível em `0x80001000` e vice-versa, com as mesmas páginas físicas |
| T6 | Travessia de página | acesso de 4 bytes com 1 byte em uma página e 3 na seguinte, sendo a segunda ausente |
| T7 | Regiões MMIO | leitura e escrita em cada faixa de dispositivo chega no stub certo, com deslocamento certo |
| T8 | Buracos | acesso a região não mapeada gera falha e **incrementa a telemetria** |
| T9 | Tiled | acesso a `0xF0000000` falha com mensagem explícita de não implementado |
| T10 | Snapshot | arena + MMU ida e volta; salvar duas vezes dá bytes idênticos (ADR-0004) |
| T11 | Diferencial | mesmo traço de acessos em caminho seguro e, quando existir, fastmem; comparação byte a byte |
| T12 | Robustez | 20 mil acessos de endereço e tamanho pseudoaleatórios, semente fixa, sem pânico |
| T13 | Miri | T1–T10 sob Miri, sem UB relatado |
| T14 | **Integração com o loader** | ver abaixo |

### T14, o teste que amarra a Fase 2 à Fase 1

É a demonstração de integração pedida pela revisão, e é critério de aceite:

1. abrir a ISO real com `Xdvdfs`, extrair `default.xbe`, parsear com `Xbe` — tudo isso
   já funciona hoje;
2. criar `Bus` com 64 MiB;
3. para cada uma das 14 seções, mapear páginas e copiar `section_data` para o endereço
   virtual da seção;
4. montar a tabela de páginas da janela `0x80000000`;
5. **verificar** que `bus.ler_u32(GuestVirt(xbe.entry_point))` devolve exatamente os
   mesmos 4 bytes que o arquivo tem em `xbe.va_to_offset(entry_point)`;
6. **verificar** que a thunk table em `0x0033F800` lida pelo bus produz os mesmos 152
   ordinais que `xbe.kernel_imports()` leu direto do arquivo;
7. **verificar** que o mesmo endereço pela janela contígua dá o mesmo byte;
8. **verificar** que ler uma página fora de qualquer seção gera `Falta`, não zero.

O passo 6 é o mais valioso: ele fecha o ciclo arquivo → loader → memória → tradução de
endereço com dado de um jogo comercial real, e qualquer confusão entre virtual e físico
aparece como ordinal errado.

Como a ISO não pode entrar no repositório, T14 roda em duas versões: uma com fixture
gerada (sempre, no CI) e uma com a mídia real (manual, atrás de variável de ambiente
`OBSIDIAN_MIDIA_REAL=<caminho>`).

---

## 8. Preparação para Vulkan, sem implementar GPU

Uma decisão precisa ser tomada **agora** porque muda a criação da arena:

**A arena tem de ser um objeto de memória compartilhável, não um `mmap` anônimo.**
`memfd_create` no Linux, objeto de seção no Windows. Motivo: na Fase 5 a GPU precisa ler
vértices e texturas que estão na RAM do guest. Com objeto compartilhável, essa memória
pode ser importada no Vulkan (`VK_EXT_external_memory_dma_buf` no Linux) ou mapeada em
memória host-visible sem segunda cópia. Com `mmap` anônimo, só resta copiar tudo a cada
frame.

O que fica preparado agora, sem uma linha de GPU:

- `Bus::dma(GuestPhys, len) -> Result<&[u8], Falta>`: a interface pela qual a GPU lerá,
  já tipada em físico;
- arena criada como objeto compartilhável, com o descritor guardado para a Fase 5;
- a arena tem posse exclusiva e é Send, não Sync e não clonável; acesso concorrente
  pela GPU precisa de nova decisão de sincronização/posse na Fase 5 (ADR-0020);
- nada de formato de textura, pipeline ou shader. Isso é Fase 5.

### A corrida que precisa ser decidida, não escondida

**Decisão atual (ADR-0020):** Arena é Send, não Sync e não clonável. Transferência
de posse/sincronização ou cópia deverá ser definida na Fase 5. A proposta histórica
abaixo de aceitar data races foi substituída; não é uma permissão de introduzir UB.

A thread de CPU escreve na arena enquanto a thread de GPU lê. Isso é, pelo modelo de
memória do Rust, corrida de dados — UB formal, mesmo que em x86-64 acessos alinhados
não rasguem na prática. As opções:

| Opção | Custo |
|---|---|
| acesso atômico byte a byte | mata a performance |
| duplicar a arena (buffer duplo) | +64 MiB e sincronização por frame |
| aceitar a corrida, com acesso por ponteiro cru e invariantes documentados | o que todo emulador com thread de GPU faz |

A sincronização **real** não é do modelo de memória: é o protocolo do pushbuffer, com
semáforos e notifiers do NV2A, que o jogo usa para saber que a GPU terminou. Proposta:
aceitar a corrida, documentar o invariante ("a GPU só lê faixa cuja posse o jogo cedeu
via pushbuffer") e registrar como desvio conhecido no ADR-0011. Fingir que o problema
não existe seria pior do que escolher conscientemente.

---

## 9. Limitações conhecidas

1. **O mapa de MMIO não está validado.** Vem de documentação pública; endereço de
   dispositivo só se confirma com código rodando, na Fase 3/4. Mitigação: telemetria de
   acesso desconhecido com PC do chamador.
2. **Fastmem não entregue na Fase 2**, por decisão baseada em medição (§5). Enquanto
   isso, o custo do caminho seguro é de +4% (acesso que vai à RAM) a +292% (acesso em
   L1) sobre o acesso direto.
3. **A fração de acessos que fica em cache é desconhecida**, e é ela que determina o
   ganho real do fastmem. Só mensurável com o JIT existindo.
4. **Falha de página custa 11,5 µs no Windows.** Isso restringe o uso de proteção de
   página a eventos raros, e afeta o desenho da invalidação de textura na Fase 5.
5. **Janela tiled não implementada**; acesso falha explicitamente.
6. **128 MiB (dev kit) não validado** — a feature existe, nenhum título real foi testado.
7. **Concorrência CPU↔GPU pendente:** a arena atual tem posse exclusiva; a proposta
   de aceitar data races foi substituída pelo ADR-0020. Não há acesso concorrente hoje.
8. **Miri não cobre fastmem** nem manipulador de sinal. Quando o fastmem entrar, essa
   parte sai da cobertura de Miri e passa a depender de teste diferencial.
9. **Medições são de um host só** (Windows, x86-64 moderno). O alvo do projeto é um
   notebook fraco com Linux; os números lá serão diferentes, provavelmente com custo
   relativo de checagem **menor** (CPU mais lenta esconde mais) e custo de falha
   **menor** (`SIGSEGV` é mais barato que VEH).

---

## 10. Independência da interface

A revisão externa sugeriu manter o emulador independente da futura interface. Concordo
com o objetivo e ele **já está satisfeito**, mas por um mecanismo diferente do proposto:

- a dependência é de via única: `xbox-ui` depende do núcleo; **nenhum** crate do núcleo
  depende de `xbox-ui`. Isso é verificável em `cargo tree` hoje;
- o emulador é dirigível sem interface: `obsidian run <mídia>` já monta a VFS e carrega
  o executável num processo sem janela. Desenvolver e testar no Fedora não exige a distro
  customizada, que é o que a sugestão queria garantir;
- **mas não vou separar em processos.** O requisito de UX do ROADMAP §1.1 é que o botão
  Home suspenda o jogo e volte ao menu instantaneamente, sem recriar contexto gráfico.
  Com Shell e emulador em processos distintos seriam dois devices Vulkan, IPC no caminho
  do frame e handoff de superfície — custo alto para resolver um problema que a direção
  de dependência já resolve.

A separação certa aqui é **fronteira de biblioteca, não de processo**.

---

## 11. Estratégia de execução do código x86 — resposta direta

A revisão pergunta se será interpretação, JIT ou execução nativa assistida. A resposta
do ADR-0003 é **JIT como caminho de produção**, e a medição de falha de página reforça:

- **interpretação** é infraestrutura permanente de teste, não caminho de produção: a
  ~50–150 ciclos de host por instrução do guest, não há 60 fps no hardware-alvo;
- **JIT** é o caminho suportado. Cranelift primeiro, por ser mais rápido de acertar;
  backend próprio só se o perfil justificar;
- **virtualização (KVM/WHPX)** é experimental, e há um argumento quantitativo contra
  ser o caminho principal: com kernel HLE, **cada chamada ao kernel é uma saída de VM**.
  O NFSU2 importa 152 ordinais; um frame faz de centenas a milhares dessas chamadas. A
  um custo de saída comparável ao que medi para exceção, isso consome milissegundos por
  frame — enquanto o JIT chama o kernel HLE como função nativa, em nanossegundos.
  A virtualização ganha no código puro e perde na fronteira; para Xbox, que é um
  console cujo título conversa muito com o kernel, a fronteira é frequente.

Há um ganho de correção real na virtualização que vale registrar: x87 com 80 bits e
controle de precisão sai **exato e de graça**, e emular isso fielmente é um dos pontos
mais chatos de um JIT. Por isso o caminho continua previsto — como acelerador opcional
para títulos limitados por CPU, não como base.

---

## 12. A ressalva de segurança sobre a VFS estava certa

A revisão aponta que recusar `..` e conferir o caminho final não garante sandbox: link
simbólico e corrida entre verificar e abrir permitem escapar. **Está certo**, e a
implementação da Fase 1 tem essa limitação.

O que mudou agora: a resolução passa a canonicalizar o caminho e a raiz e exigir que o
resultado continue sob a raiz — isso fecha a fuga por link simbólico, que é o ataque
realizável sem corrida.

O que **não** está fechado: a janela entre canonicalizar e abrir. A correção certa é
abertura relativa a descritor de diretório com `openat2(RESOLVE_BENEATH)` no Linux, e
verificação do caminho real do handle depois de abrir no Windows. Isso é dependente de
sistema operacional, então mora em `xbox-platform`, e está registrado no ADR-0012 e
como item da Fase 7.

Contexto de risco, para dimensionar: o atacante aqui é um XBE que o próprio usuário
escolheu rodar, com acesso a `T:`/`U:`/`Z:`. Não é um servidor exposto. Isso justifica
tratar a corrida como dívida agendada em vez de bloqueio — mas não justificava deixar a
fuga por link simbólico aberta, que era barata de fechar.

---

## 13. O que a Fase 2 entrega

Checklist de aceite, substituindo o do ROADMAP:

- [ ] newtypes `GuestVirt`, `GuestPhys`, `ArenaOff` sem conversão implícita
- [ ] arena de 64 MiB em objeto compartilhável (`memfd`/seção), com handle `Send + Sync`
- [ ] MMU: páginas de 4 KiB e 4 MiB, permissões, bits de sujo e acessado, cache de
      tradução, `Falta` com `CR2` e código de erro
- [ ] janelas virtuais, incluindo a contígua `0x80000000` por tabela de páginas
- [ ] classificação de região física e tabela de dispositivos com stubs e telemetria
- [ ] caminho de acesso seguro completo, 100% Rust seguro fora dos dois blocos da arena
- [ ] `Snapshot` de arena e MMU, determinístico
- [ ] T1–T13 verdes, inclusive sob Miri
- [ ] **T14 verde com a ISO real**, incluindo os 152 ordinais lidos pelo bus
- [ ] `docs/arch/memoria-unsafe.md` com cada bloco `unsafe` e seu invariante
- [ ] benchmark do caminho seguro no `bench/`, para servir de linha de base ao fastmem

Fora de escopo, por decisão: fastmem (Fase 3b), janela tiled (Fase 5), escrita em
dispositivo que não seja stub (Fases 5 e 6).
