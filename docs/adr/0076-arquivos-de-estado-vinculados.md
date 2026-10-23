# ADR-0076 — Arquivos de estado vinculados à imagem

Data: 2026-10-23. Estado: aceito para PLANO 5.1. Base `a2fc5e9`.

## Decisão

O alpha precisa retomar sessões fora dos testes. `checkpoint` envolve o snapshot
integral da Session em um contêiner OBSIDSNP/SAVE v1: bytes exatos do XBE original,
snapshot interno com comprimento e CRC32 IEEE. O teto total é 256 MiB; leitura
limitada também impede crescimento concorrente do arquivo. Versão zero/futura,
imagem divergente, truncamento, CRC incorreto e dados excedentes recusam.
CRC detecta corrupção acidental; não autentica arquivos maliciosos. Não aceitar
save states de fontes desconhecidas. Não há compressão, criptografia ou ROM/BIOS.

`decode` carrega a imagem e restaura numa Session nova; a troca só ocorre depois
de todos os subsistemas validarem. O contrato mutável legado de Snapshot::load
não se torna transacional por acidente: o candidato é descartado em erro. SAVE
é uma nova camada; AOTS v4, AOTC v10, BUS0 v9 e suas migrações não mudam.
SAVE v1 é a primeira versão, sem predecessor a migrar. Snapshot cru não é SAVE.
A identidade é do XBE, não de toda revisão do recompilador: formatos internos
continuam controlando compatibilidade; não prometer replay entre modelos distintos.

Publicação escreve temporário exclusivo no diretório de destino, sincroniza o
arquivo e cria hard link para o nome final. Destino existente não é substituído,
mesmo numa corrida. O temporário pertence só à chamada e é removido em seguida;
crash pode deixar `.part`, nunca selecionado automaticamente. Filesystem sem
hard links recusa, sem fallback que apague/substitua arquivos. Não há promessa
de durabilidade do diretório sob perda de energia.
API e limites do primitivo: [Rust std::fs::hard_link](https://doc.rust-lang.org/std/fs/fn.hard_link.html).

`--carregar caminho` precede execução; `--salvar caminho` exporta ao encerramento,
inclusive orçamento/falha guest. Não sobrescreve o caminho carregado. Falha ao
salvar é relatada e faz uma execução bem-sucedida falhar; numa falha guest a
causa original permanece. BGRA continua captura de imagem, não estado.

## Verificação

Testes cobrem roundtrip integral CPU/RAM/kernel, integridade, limites, versões,
truncamentos, imagem divergente, trailing bytes internos/externos e publicação
sem sobrescrita. O Rust efetivamente gerado retoma um CALL pendente com replay
integral; seu main salva loop por orçamento, carrega em outro processo e avança.
HLT/INT3 preservam o diagnóstico mesmo com destino de estado indisponível.
Números finais do gate constam na evidência imutável da tarefa 5.1 em PLANO.md.
