# ADR-0007: Licença do projeto

- **Status:** aceito — Apache-2.0, conforme o repositório criado pelo mantenedor
- **Data:** 2026-10-19
- **Fase:** 0

## Contexto

Os crates começaram com `publish = false` e sem campo `license`, o que era válido
para desenvolvimento mas não definia termos de uso e redistribuição. A escolha interage com o
ADR-0001 (reimplementação limpa) e com o ROADMAP §14.

## Alternativas consideradas

1. **GPL-3.0-or-later** — comum em emuladores; garante que forks permaneçam abertos;
   restringe a integração de código sob outras licenças.
2. **MIT OR Apache-2.0** — padrão do ecossistema Rust; máxima compatibilidade com as
   dependências; permite forks fechados.
3. **Sem licença (todos os direitos reservados)** — estado atual; impede contribuição
   externa e redistribuição.
4. **Apache-2.0** — licença selecionada pelo mantenedor ao criar o repositório GitHub.

## Decisão

**Apache-2.0.** Em 2026-10-19, o mantenedor criou
`https://github.com/rodolfoaltenrath/xbox-classic-rust` com um arquivo `LICENSE`
Apache-2.0 e autorizou o envio do projeto. O arquivo original e seu commit são
preservados; os metadados Cargo adotam a mesma licença.

## Consequências

A publicação do código no GitHub está autorizada. Os pacotes continuam com
`publish = false`: publicar no crates.io não faz parte dessa decisão. As dependências
mantêm seus próprios termos, registrados em `docs/dev/third-party.md`.

## Como reverter

Trocar de licença depois exige consentimento de todos os contribuidores, então é mais
barato decidir enquanto o projeto tem um autor só.
