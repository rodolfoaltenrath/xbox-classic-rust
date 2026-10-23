//! Laço de execução do `main` gerado: fatias de orçamento, serviços de kernel e a
//! interrupção pelo host.
//!
//! Vive aqui, e não no template do `main`, para ser testável sem compilar um projeto
//! gerado: o código nativo entra como o fechamento `run`, e a janela como o fechamento
//! `apresentar`. Nenhum dos dois é conhecido por este módulo.
use super::{Exit, Servico, Session, State};
use crate::{Error, Result};
use xbox_mem::Bus;

/// Orçamento do laço: `fatias` fatias de `blocos_por_fatia` blocos cada.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limites {
    pub blocos_por_fatia: u64,
    pub fatias: u32,
}

impl Limites {
    /// Orçamento total em blocos, saturado: serve só para o relato de esgotamento.
    pub const fn total(self) -> u64 {
        self.blocos_por_fatia.saturating_mul(self.fatias as u64)
    }
}

/// Como o laço terminou sem erro.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Desfecho {
    /// O título retornou do ponto de entrada.
    Retornou,
    /// O host pediu para parar (janela fechada). Estado intacto e retomável.
    ParadoPeloHost,
}

impl Session {
    /// Executa até o título retornar, o host pedir para parar ou o orçamento acabar.
    ///
    /// `run` executa até `blocos` blocos recompilados. `apresentar` é chamado quando há
    /// uma chance natural de mostrar o quadro e devolve `true` se o host quer parar.
    /// Isso acontece em duas ocasiões: ao fim de uma fatia (rede de segurança para
    /// títulos que desenham sem nunca dormir) e depois de um serviço de kernel que
    /// **avançou o relógio guest**, isto é, uma espera. Detectar pelo `Clock` — e não
    /// pelo serviço — mantém a política independente de quais esperas existam.
    pub fn conduzir<R, A>(
        &mut self,
        limites: Limites,
        mut run: R,
        mut apresentar: A,
    ) -> Result<Desfecho>
    where
        R: FnMut(&mut State, &mut Bus, u64) -> Result<Exit>,
        A: FnMut(&Session) -> Result<bool>,
    {
        for _ in 0..limites.fatias {
            self.prepare_schedule()?;
            self.deliver_interrupts(&mut run, limites.blocos_por_fatia)?;
            let saida =
                self.run_slice(&mut run, self.scheduled_budget(limites.blocos_por_fatia))?;
            if matches!(
                saida,
                Exit::Budget | Exit::KernelCall { .. } | Exit::Returned
            ) {
                self.bus.service_gpu(4096)?;
            }
            self.publicar_tick_count()?;
            match saida {
                Exit::Returned => {
                    if self.complete_thread_return()? {
                        continue;
                    }
                    return Ok(Desfecho::Retornou);
                }
                Exit::Budget => {
                    if apresentar(self)? {
                        return Ok(Desfecho::ParadoPeloHost);
                    }
                }
                Exit::Breakpoint { pc } => {
                    return Err(Error::GuestBreakpoint {
                        pc,
                        next_eip: self.state.eip,
                    });
                }
                Exit::Halted {
                    pc,
                    interrupts_enabled,
                } => {
                    return Err(Error::GuestHalted {
                        pc,
                        next_eip: self.state.eip,
                        interrupts_enabled,
                    });
                }
                // O erro do dispatcher identifica nome, ordinal, chamador e argumentos.
                // A CPU fica intacta: o estado pendente continua retomável.
                Exit::KernelCall { .. } => {
                    let antes = self.state.clock.cycles();
                    // Uma espera que só termina com tempo e interrupções deixa o relógio
                    // correr até o próximo vblank; o ISR e o DPC rodam no topo da próxima
                    // volta, e a mesma chamada é repetida.
                    if self.atender_kernel_ou_bloquear()? == Servico::Bloqueado {
                        self.schedule_idle()?;
                    }
                    if self.state.clock.cycles() != antes && apresentar(self)? {
                        return Ok(Desfecho::ParadoPeloHost);
                    }
                }
            }
        }
        // O Clock guest distingue "dormiu várias vezes" de "preso num laço sem espera":
        // só esperas avançam o relógio além do custo sintético de uma instrução por passo.
        Err(Error::BudgetExhausted {
            blocks: limites.total(),
            served: self.kernel.atendidas(),
            refused: self.kernel.recusadas(),
            retired: self.state.retired,
            cycles: self.state.clock.cycles(),
            ms: self.state.clock.tick_count_ms(),
            eip: self.state.eip,
        })
    }
}

impl Session {
    /// Roda uma fatia e, se ela falhar por acesso à memória, diz **qual instrução** falhou.
    ///
    /// O erro cru do Bus só tem o endereço acessado; sem o EIP, "falta em 0x40" não diz
    /// quem dereferenciou. O Rust gerado grava `eip` no começo de cada instrução, então
    /// ele aponta para a que falhou.
    pub(super) fn run_slice<R>(&mut self, run: &mut R, blocks: u64) -> Result<Exit>
    where
        R: FnMut(&mut State, &mut Bus, u64) -> Result<Exit>,
    {
        self.state.poll = self.execution_poll()?;
        let result = run(&mut self.state, &mut self.bus, blocks);
        self.state.poll = None;
        result.map_err(|error| match error {
            Error::Memory(fault) => Error::MemoryAt {
                pc: self.state.eip,
                fault,
            },
            other => other,
        })
    }
}
