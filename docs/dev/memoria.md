# Validar memória e carregamento

O loader em `xbox-kernel` prepara bytes e permissões, sem TLS, resolução de imports
ou execução. A sessão AOT reutiliza o loader sob política mais restritiva.

```powershell
cargo run -p obsidian -- check-memory Need_for_Speed_Underground_2.iso
cargo test -p xbox-mem --test contract
cargo test -p xbox-kernel --test loader
```

`check-memory` aceita também `.xbe` e pasta com `default.xbe`. Lê a ISO sob demanda
e limita o XBE a 128 MiB; compara toda a imagem mapeada com o arquivo/BSS e o alias
físico. Não extrai ou grava código comercial e não monta drives de saves.

O T14 com mídia real é intencionalmente ignorado no CI:

```powershell
$env:OBSIDIAN_MIDIA_REAL = 'C:\caminho\Need_for_Speed_Underground_2.iso'
cargo test -p xbox-kernel --test loader t14_midia_real -- --ignored --nocapture
```

Sem a variável, esse teste explícito falha; a fixture sempre roda normalmente.

## Miri

SFENCE usa a barreira do bus síncrono exclusivo, sem filas de stores; o contrato
`barreira_de_stores_preserva_visibilidade_sincrona_entre_visoes_e_aliases` verifica
RAM por alias, visão física e DMA. AOTC e snapshots do bus mantêm seus formatos.
Não cobre MMIO funcional ou DMA concorrente, ainda pendentes
([ADR-0028](../adr/0028-sfence-ordenacao-do-bus.md)).

```powershell
rustup toolchain install nightly-2026-10-20 --profile minimal --component miri,rust-src
cargo +nightly-2026-10-20 miri setup --target x86_64-unknown-linux-gnu
cargo +nightly-2026-10-20 miri test -p xbox-mem --test contract --target x86_64-unknown-linux-gnu
```

No host Windows local, o alias `+nightly` instalado em 2026-10-20 foi usado
(rustc 1.101.0-nightly, 07e526db9). O toolchain normal permanece Rust 1.98.1.
Miri executou seis testes cobrindo T1–T10, sem diagnóstico de UB. T11/T12 é
ignorado sob Miri e roda nativamente com 20000 cenários; T14 comercial também
é nativo. A arena de 64 KiB usa alocação fallback; não há cobertura FFI de SO.
Miri emite aviso informativo de que ignora opt-level do perfil.

A documentação oficial recomenda o alvo Linux para essa execução a partir de
Windows: [Miri](https://github.com/rust-lang/miri#cross-interpretation-running-for-different-targets).
O CI ganhou um job desse contrato, com nightly fixado; execução remota ainda não
foi observada nesta etapa. Auditoria: [memoria-unsafe.md](../arch/memoria-unsafe.md).

## Benchmark local

```powershell
cargo run --release -p xbox-mem --example safe_memory
```

Compara leituras u32 alinhadas com stride de 64 bytes, via arena e via MMU/bus,
em 64 KiB e 64 MiB, depois de materializar páginas. Usa checksums e black_box.
É uma linha de base deste host, não FPS ou prova de desempenho em um notebook.

Medição local única em release (sem limiar de aprovação):

| Conjunto de dados | Leituras | Arena, ns/acesso | Bus, ns/acesso | Razão |
|---|---:|---:|---:|---:|
| 64 KiB | 2097152 | 2,75 | 12,65 | 4,61× |
| 64 MiB | 4194304 | 4,67 | 13,21 | 2,83× |

Não é a mesma sonda dos números históricos do ADR-0010; não permite comparar
diretamente ganhos entre revisões ou estimar o custo de um frame de jogo.

## Evidência local de 2026-10-20

Windows x86_64-pc-windows-gnu, Rust 1.98.1. T14 do NFSU2: 14 seções, 1132 páginas,
4634697 bytes, 152 ordinais idênticos aos do arquivo. Entry 0x0021B1CE = 0x01180D8B.
O teste automatizado com `OBSIDIAN_MIDIA_REAL` também passou.

Passaram 200 testes de workspace, um teste real explícito adicional, 64 testes
nativos do Rust gerado em debug/release e dois doctests. Um teste de mídia real
fica ignorado na execução normal por depender do arquivo local. Clippy do
workspace inteiro, com all-targets/isa-oracle e warnings como erros, passou.
Formatação e diff também passaram. O AOT mantém EAX=62 e 37 instruções na prova.

Logs locais ignorados pelo Git: `target/memory-nfs-t14.log`,
`target/memory-t14-real-test.log`, `target/memory-miri-contract.log`,
`target/memory-safe-benchmark.log`, `target/memory-workspace-tests.log` e
`target/memory-workspace-clippy.log`. Mídias e BIOS permanecem fora do repositório.

## Integração com alocação virtual HLE — ADR-0043

NtAllocateVirtualMemory usa MMU e alocador de frames existentes para reserva+commit
privado, no perfil descrito em [ADR-0043](../adr/0043-alocacao-virtual-guest.md).
O planejamento copia apenas MMU/bitmap; a arena permanece única. Novos frames
são zerados antes de publicar páginas e saídas. A enumeração de mapeamentos é
somente leitura e não marca Accessed/Dirty. Session confere registros, posse,
permissões e aliases ao restaurar. FRM0 v1 valida bitmap/contagem/padding e
truncamento antes de alterar o alocador. MMU0 v1 recusa frames fora do espaço
de 32 bits e páginas grandes fisicamente desalinhadas antes de enumerá-las.
Nenhum unsafe ou API de arena mudou.
A suíte inclui frames fragmentados/sujos, OOM, aliases de saída e snapshot
inconsistente. O aceite local original da Fase 2 continua sendo o marco do ADR-0020.
