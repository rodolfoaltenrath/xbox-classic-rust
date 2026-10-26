# ADR-0084 — Mídia original na sessão AOT

Data: 2026-10-26. Estado: aceito. PLANO 7.2.

Session::load publica uma cópia do XBE validado no arquivo virtual default.xbe
do ADR-0083. A cópia não vem das páginas carregadas: relocação, escrita guest
e alterações de headers na RAM não mudam o arquivo. O núcleo não guarda
File/descriptor/caminho host. Limite inicial: 32 MiB, declarado em P22.

Snapshot continua AOTS v4/SAVE v1, com KRNL v14/CDFS v1 incorporado. Restaurar
substitui a mídia pelo conteúdo serializado, sem chamar mount novamente.
KRNL v1–v13 permanece sem mídia. encode/decode de SAVE conferem que a mídia,
se presente, coincide com o XBE exato do contêiner: CRC válido não autoriza
trocar os bytes do arquivo virtual por outro executável. Snapshot bruto
mantém o contrato anterior de não autenticar/bindar por si só a imagem externa.

Três casos novos compilam e executam Rust emitido para NtCreateFile, duas
leituras, consulta NetworkOpen e NtClose. Conferem stdcall/EAX/ESP/flags,
bytes originais apesar de headers RAM modificados, replay desde handle aberto,
recusa com CPU intacta e migração v13 sem mídia. Um teste de SAVE forja CRC
correto com mídia divergente e exige recusa específica na leitura e escrita.

As comparações integrais de migrações antigas continuam ativas: o resultado
esperado agora declara CDFS ausente quando o kernel de origem era anterior
à v14, mantendo cada outro byte da sessão. Não remover mídia de snapshots
que só mudam scheduler/AVST/BUS: essas mudanças não implicam kernel legado.

Erros de fixture encontrados: .rdata original tinha 64 bytes raw e virtuais;
escrever argumentos em 0x12100 exigiu ampliar arquivo, raw_size e virtual_size.
O loader corretamente recusou índices fora do arquivo e raw maior que virtual.
Corrigidos os três componentes do layout; nenhuma validação foi relaxada.
O refinamento de namespace também recusa dispositivos fora de CdRom0 em vez
de mascarar sua ausência de implementação como arquivo inexistente.

Gate/contagens finais na evidência de PLANO 7.2. O produtor nxdk próprio,
ferramentas públicas e apresentação são a entrega 7.3.
