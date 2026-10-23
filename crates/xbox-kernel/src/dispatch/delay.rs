//! `KeDelayExecutionThread`: espera por intervalo relativo, sem bloquear o host (ADR-0051).
//!
//! O serviço **não avança o relógio**. O ADR-0025 fixou que existe um único Clock, que
//! pertence à sessão, e que o HLE não avança ciclos por si. Aqui o serviço valida a
//! chamada e *registra o pedido*; quem consome o pedido e avança o Clock, com a
//! checagem de overflow de `TimeExhausted`, é a sessão — o mesmo desenho de
//! `start_ready_thread`, que a sessão também consulta depois do atendimento.
//!
//! ## Por que avançar o relógio é a semântica correta
//!
//! O ADR-0025 já dizia que o escalonador futuro "avança até deadlines quando todas as
//! threads estiverem esperando". Com **uma** thread guest executável, esperar `T` e
//! avançar o Clock em `T` são a mesma coisa observável: nada mais corre nesse intervalo,
//! e o guest só enxerga o tempo pelos relógios que derivam do Clock (RDTSC, tempo de
//! sistema, contadores). Não há `sleep` do host em lugar nenhum: a execução é
//! determinística e instantânea em tempo de parede.
//!
//! Isto vale **enquanto não houver outra thread runnable**. Com a thread filha em
//! execução e o pai estacionado, o pai não corre durante o intervalo. Quando existir
//! escalonamento de verdade, avançar o relógio passa a ser decisão do escalonador — e
//! este serviço precisa passar a bloquear a thread em vez de pedir o avanço.
//!
//! ## Contrato
//!
//! Protótipo: `NTSTATUS KeDelayExecutionThread(KPROCESSOR_MODE, BOOLEAN, PLARGE_INTEGER)`.
//! Fonte pública fixada: `lib/xboxkrnl/xboxkrnl.h` do nxdk, revisão
//! `14d5ee97e73347c973f1f57b68b79ec08c9e77f2`. O `SleepEx` do mesmo SDK passa
//! `UserMode`, o `bAlertable` do chamador e `ms * -10000` (relativo, em 100 ns), e só
//! sai do laço quando o status não é `STATUS_ALERTED`.

use xbox_core::time::Clock;
use xbox_mem::{Acesso, Bus, GuestVirt};

use super::{
    Atendimento, Dispatcher, Erro, Resultado, STATUS_SUCCESS, descrever_argumentos, ram_range,
};
use crate::imports::Call;

/// `KernelMode`.
const KERNEL_MODE: u32 = 0;
/// `UserMode`.
const USER_MODE: u32 = 1;

/// Pedido de avanço de tempo, pendente até a sessão consumi-lo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PedidoDeTempo {
    /// Ciclos a avançar no Clock da sessão.
    pub ciclos: u64,
}

impl Dispatcher {
    /// Consome o pedido de avanço de tempo deixado pelo último atendimento, se houver.
    ///
    /// Só a sessão chama: ela é a dona do Clock e aplica o avanço com a checagem de
    /// overflow, depois de capturar a continuação do guest.
    pub fn take_time_request(&mut self) -> Option<PedidoDeTempo> {
        self.pending_time.take()
    }

    pub(super) fn ke_delay_execution_thread(
        &mut self,
        bus: &mut Bus,
        call: &Call,
        scheduled: bool,
    ) -> Resultado {
        let wait_mode = call.argument(bus, 0)?;
        // BOOLEAN ocupa uma palavra na ABI, mas só o byte baixo importa.
        let alertable = call.argument(bus, 1)? as u8 != 0;
        let interval_ptr = call.argument(bus, 2)?;

        let variante = |detalhe| {
            Err(Erro::VarianteNaoImplementada {
                nome: call.export.name,
                ordinal: call.export.ordinal,
                chamador: call.return_pc,
                detalhe,
                argumentos: descrever_argumentos(bus, call),
            })
        };
        let invalido = |detalhe: &str| {
            Err(Erro::ArgumentoInvalido {
                nome: call.export.name,
                ordinal: call.export.ordinal,
                detalhe: detalhe.to_owned(),
            })
        };

        if scheduled && self.irql != super::Irql::Passive {
            return variante("espera escalonada exige Passive neste perfil");
        }
        if !matches!(wait_mode, KERNEL_MODE | USER_MODE) {
            return invalido("WaitMode deve ser KernelMode (0) ou UserMode (1)");
        }
        if alertable {
            // Espera alertável pode terminar em STATUS_ALERTED ou STATUS_USER_APC, o que
            // exige entrega de APC. Devolver SUCCESS esconderia essa diferença.
            return variante("espera alertável exige APCs, ainda não modeladas");
        }
        if !ram_range(bus, interval_ptr, 8, Acesso::Leitura) {
            return invalido("Interval deve cobrir oito bytes em RAM guest legível");
        }

        let mut bytes = [0u8; 8];
        bus.ler_bytes(GuestVirt(interval_ptr), &mut bytes)?;
        let interval = i64::from_le_bytes(bytes);

        if interval >= 0 {
            // Positivo é tempo absoluto; zero seria um yield. Ambos exigem comparar com o
            // tempo de sistema ou com o escalonador, e nenhum dos dois é o que o `Sleep`
            // do nxdk produz. Recusar é mais honesto que escolher um significado.
            return variante("intervalo absoluto ou zero ainda não modelado");
        }

        // Negativo é relativo. `unsigned_abs` cobre `i64::MIN`, cuja negação transbordaria;
        // o resultado é então recusado pela conversão verificada, não truncado.
        let unidades = interval.unsigned_abs();
        let Some(ciclos) = Clock::hundred_ns_to_cycles(unidades) else {
            return invalido("Interval excede o que o Clock de ciclos consegue representar");
        };

        self.pending_time = Some(PedidoDeTempo { ciclos });
        // A espera decorreu por inteiro: sem alerta, o único desfecho é o intervalo.
        Ok(Atendimento {
            eax: STATUS_SUCCESS,
        })
    }
}
