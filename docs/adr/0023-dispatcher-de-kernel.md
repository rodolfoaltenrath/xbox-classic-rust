# ADR-0023 — Dispatcher dos serviços de kernel

Data: 2026-10-20. Estado: aceito com um serviço implementado.

## Contexto

Depois do ADR-0022 o Rust gerado sai na fronteira ABI com `Exit::KernelCall`, mas não
havia nada do outro lado: o projeto gerado abortava na primeira chamada, imprimindo
nome e ordinal. Faltava quem executasse o serviço, quem relatasse a falta de forma
acionável e quem retomasse a execução.

Duas restrições moldam a solução:

- **nenhum sucesso genérico.** Um ordinal sem serviço não pode devolver `EAX = 0` e
  seguir: o guia do alpha trata isso como falha de projeto, porque esconde o bloqueio e
  produz execução sem sentido;
- **sem ciclo de dependência.** `xbox-recomp` já depende de `xbox-kernel`. Um dispatcher
  que recebesse `Session` fecharia o ciclo.

## Decisão

Dispatcher em `xbox-kernel`, com assinatura `(&mut Bus, pc, regs) -> Result<Atendimento>`.
Quem aplica o resultado na CPU é o chamador, por `Session::complete_kernel_call`. Isso
evita o ciclo, mantém o dispatcher testável sem gerar e compilar código, e garante que
**uma falha não deixa estado pela metade**: a CPU só é alterada depois do serviço ter
sucesso.

Recusa carrega nome, ordinal, **endereço do chamador** e argumentos conforme a ABI
declarada. Cdecl mostra as primeiras palavras e avisa que a quantidade não está na
assinatura. Argumento ilegível aparece como `?`, porque a mensagem de diagnóstico não
pode falhar por causa do que ela está diagnosticando.

`IMPLEMENTADOS` é a fonte única consultada por `imports::servico_implementado` e pelo
relatório de ordinais. Um teste exige que a lista case exatamente com o que o dispatcher
atende, nos dois sentidos.

### O primeiro serviço

`RtlInitAnsiString` (289, stdcall/8), escolhido por ser **função pura sobre a memória
guest**: mede a string de origem e preenche `Length`, `MaximumLength` e `Buffer` no
destino. Nenhum estado de kernel, nada inventado, e o resultado é verificável lendo o
destino. Origem nula zera o destino; destino nulo é recusado em vez de escrever no
endereço 0; string sem NUL dentro do que `ANSI_STRING` representa é erro, não
truncamento silencioso.

É um serviço de verdade, não um stub — mas `1/69` não é progresso em direção a executar
o homebrew. Serve para provar o mecanismo ponta a ponta no Rust gerado.

## Consequências

- O projeto gerado passou a ter laço de despacho: executa, atende, retoma. Orçamento
  total e interrupção pelo host continuam sendo trabalho da entrega 5; o limite de
  fatias existe aqui só para que serviço faltante não vire laço infinito.
- Duas verificações no Rust **gerado** cobrem o caminho completo: serviço atendido com
  efeito na memória e execução retomando até o `RET`; e serviço faltante relatado com a
  CPU intacta. O contador de testes gerados foi de 69 para 71.
- `xbox-kernel` entrou nas dependências do projeto gerado.
- O relatório de ordinais passou a mostrar `1/69 serviços implementados`, derivado do
  estado real.
- O bloqueio do alvo **não mudou**: segue `LOCK CMPXCHG` em `0x209C3`, em tempo de
  tradução. Dispatcher não destrava ISA.

## Como reverter

O dispatcher é consultado em um ponto (`Session::atender_kernel`) e o laço vive no
`main.rs` gerado. Remover os dois devolve o comportamento anterior de abortar na
primeira chamada, sem afetar a fronteira ABI do ADR-0021.
