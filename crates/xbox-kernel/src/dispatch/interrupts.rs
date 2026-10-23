//! Interrupções e DPCs: o contrato do kernel com o guest (ADR-0055).
//!
//! `KINTERRUPT` e `KDPC` vivem na memória **do guest**, nos layouts do nxdk
//! (`xboxkrnl.h`): o título as aloca, o kernel as preenche e lê de volta na entrega. O
//! estado do host é só o que o guest não guarda: quais interrupções estão conectadas e a
//! fila de DPCs, ambas listas de endereços.
//!
//! Este módulo **não executa código guest**: ele diz à sessão o que chamar
//! ([`GuestCall`]). Quem executa é `Session::conduzir`, que tem o código nativo.
//!
//! ## Valores declarados, não verificados
//!
//! - `HalGetInterruptVector`: vetor = `0x30 + nível` e `KIRQL = 27 − nível`. É a
//!   convenção do HAL x86 do NT; nenhum alvo testado depende do número, que só passa pelo
//!   guest de volta para `KeInitializeInterrupt`. Registrado como P10.
//! - O ISR roda com o IRQL **inalterado**: o perfil só representa PASSIVE/APC/DISPATCH
//!   (ADR-0042), e um DIRQL não existe nele.

use std::collections::VecDeque;

use xbox_core::error::SnapshotError;
use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_mem::{Acesso, Bus, GuestVirt};

use super::{Atendimento, Erro, Irql, Resultado, ram_range};
use crate::imports::Call;

/// Vetor do primeiro IRQ (declarado, ver o módulo).
pub(super) const VECTOR_BASE: u32 = 0x30;
/// Maior nível de barramento aceito (16 IRQs).
pub(super) const MAX_BUS_LEVEL: u32 = 15;
/// Nível de barramento da GPU, o `GPU_IRQ` do pbkit.
pub const GPU_IRQ: u32 = 3;
/// Interrupções conectadas ao mesmo tempo.
pub(super) const MAX_CONNECTED: usize = 32;
/// DPCs na fila ao mesmo tempo.
pub(super) const MAX_QUEUED_DPCS: usize = 256;
/// Notificações de desligamento registradas ao mesmo tempo.
pub(super) const MAX_SHUTDOWN: usize = 16;
/// `KDPC.Type` de um DPC inicializado.
const DPC_OBJECT: u16 = 19;

// Layout de KINTERRUPT.
const KI_SERVICE_ROUTINE: u32 = 0;
const KI_SERVICE_CONTEXT: u32 = 4;
const KI_BUS_LEVEL: u32 = 8;
const KI_CONNECTED: u32 = 16;
/// Bytes de `KINTERRUPT` que o kernel escreve: até `ServiceCount`, sem `DispatchCode`.
const KI_HEADER: u32 = 24;

// Layout de KDPC.
const KD_INSERTED: u32 = 2;
const KD_ROUTINE: u32 = 12;
const KD_CONTEXT: u32 = 16;
const KD_ARG1: u32 = 20;
const KD_ARG2: u32 = 24;
/// Tamanho de `KDPC`.
const KD_SIZE: u32 = 28;

/// Uma chamada que a sessão deve fazer ao código guest, como `stdcall`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuestCall {
    /// Endereço da rotina.
    pub entry: u32,
    /// Argumentos em ordem; só os `count` primeiros valem.
    pub arguments: [u32; 4],
    /// Quantos argumentos a rotina `stdcall` limpa da pilha.
    pub count: usize,
}

#[derive(Debug, Default)]
pub(super) struct Interrupts {
    /// Endereços de `KINTERRUPT` conectados, em ordem de conexão.
    connected: Vec<u32>,
    /// Endereços de `KDPC` enfileirados, em ordem de chegada.
    dpcs: VecDeque<u32>,
    /// Registros de notificação de desligamento (`HAL_SHUTDOWN_REGISTRATION`), em ordem.
    shutdown: Vec<u32>,
}

impl Interrupts {
    /// A interrupção da GPU está conectada? Sem ela, esperar o vblank não termina.
    pub(super) fn gpu_connected(&self, bus: &Bus) -> bool {
        self.connected
            .iter()
            .any(|&interrupt| bus_level(bus, interrupt) == Some(GPU_IRQ))
    }

    /// A rotina de serviço da primeira interrupção conectada à GPU, se a linha está ligada.
    pub(super) fn pending(&self, bus: &Bus) -> Option<GuestCall> {
        if !bus.gpu_irq_line_asserted() {
            return None;
        }
        let interrupt = *self
            .connected
            .iter()
            .find(|&&interrupt| bus_level(bus, interrupt) == Some(GPU_IRQ))?;
        let routine = bus
            .ler_u32(GuestVirt(interrupt + KI_SERVICE_ROUTINE))
            .ok()?;
        let context = bus
            .ler_u32(GuestVirt(interrupt + KI_SERVICE_CONTEXT))
            .ok()?;
        Some(GuestCall {
            entry: routine,
            arguments: [interrupt, context, 0, 0],
            count: 2,
        })
    }

    /// Retira o próximo DPC e o marca como não enfileirado **antes** de devolvê-lo, como o
    /// kernel faz: assim a própria rotina pode se enfileirar de novo.
    pub(super) fn next_dpc(&mut self, bus: &mut Bus) -> Result<Option<GuestCall>, Erro> {
        let Some(dpc) = self.dpcs.pop_front() else {
            return Ok(None);
        };
        let routine = bus.ler_u32(GuestVirt(dpc + KD_ROUTINE))?;
        let context = bus.ler_u32(GuestVirt(dpc + KD_CONTEXT))?;
        let arg1 = bus.ler_u32(GuestVirt(dpc + KD_ARG1))?;
        let arg2 = bus.ler_u32(GuestVirt(dpc + KD_ARG2))?;
        bus.escrever_u8(GuestVirt(dpc + KD_INSERTED), 0)?;
        Ok(Some(GuestCall {
            entry: routine,
            arguments: [dpc, context, arg1, arg2],
            count: 4,
        }))
    }

    pub(super) fn has_queued_dpcs(&self) -> bool {
        !self.dpcs.is_empty()
    }

    /// Confere o estado restaurado contra a memória guest: cada endereço guardado precisa
    /// ser um objeto coerente, senão o snapshot é de outra sessão.
    pub(super) fn validate(&self, bus: &Bus) -> Result<(), SnapshotError> {
        let bad = |what| SnapshotError::BadValue {
            section: "KINT",
            what,
        };
        for &interrupt in &self.connected {
            if bus.ler_u8(GuestVirt(interrupt + KI_CONNECTED)).ok() != Some(1)
                || bus_level(bus, interrupt).is_none()
            {
                return Err(bad("interrupção conectada diverge da memória guest"));
            }
        }
        for &dpc in &self.dpcs {
            if bus.ler_u8(GuestVirt(dpc + KD_INSERTED)).ok() != Some(1) {
                return Err(bad("DPC enfileirado diverge da memória guest"));
            }
        }
        Ok(())
    }
}

/// `BusInterruptLevel` de uma `KINTERRUPT`, se legível e dentro do intervalo.
fn bus_level(bus: &Bus, interrupt: u32) -> Option<u32> {
    bus.ler_u32(GuestVirt(interrupt + KI_BUS_LEVEL))
        .ok()
        .filter(|&level| level <= MAX_BUS_LEVEL)
}

impl Snapshot for Interrupts {
    fn save(&self, out: &mut SnapshotWriter) {
        out.section("KINT", 2);
        out.u32(self.connected.len() as u32);
        for &interrupt in &self.connected {
            out.u32(interrupt);
        }
        out.u32(self.dpcs.len() as u32);
        for &dpc in &self.dpcs {
            out.u32(dpc);
        }
        out.u32(self.shutdown.len() as u32);
        for &registration in &self.shutdown {
            out.u32(registration);
        }
    }

    fn load(&mut self, inp: &mut SnapshotReader<'_>) -> Result<(), SnapshotError> {
        let bad = |what| SnapshotError::BadValue {
            section: "KINT",
            what,
        };
        let version = inp.section("KINT", 2)?;
        if !matches!(version, 1 | 2) {
            return Err(bad("versão desconhecida"));
        }
        let connected = inp.u32()? as usize;
        if connected > MAX_CONNECTED {
            return Err(bad("interrupções conectadas além do limite"));
        }
        let mut list = Vec::with_capacity(connected);
        for _ in 0..connected {
            let interrupt = inp.u32()?;
            if list.contains(&interrupt) {
                return Err(bad("interrupção conectada duplicada"));
            }
            list.push(interrupt);
        }
        let queued = inp.u32()? as usize;
        if queued > MAX_QUEUED_DPCS {
            return Err(bad("fila de DPCs além do limite"));
        }
        let mut dpcs = VecDeque::with_capacity(queued);
        for _ in 0..queued {
            let dpc = inp.u32()?;
            if dpcs.contains(&dpc) {
                return Err(bad("DPC duplicado na fila"));
            }
            dpcs.push_back(dpc);
        }
        // v1 não tinha notificações de desligamento: migra para nenhuma.
        let mut shutdown = Vec::new();
        if version >= 2 {
            let count = inp.u32()? as usize;
            if count > MAX_SHUTDOWN {
                return Err(bad("notificações de desligamento além do limite"));
            }
            for _ in 0..count {
                let registration = inp.u32()?;
                if shutdown.contains(&registration) {
                    return Err(bad("notificação de desligamento duplicada"));
                }
                shutdown.push(registration);
            }
        }
        self.connected = list;
        self.dpcs = dpcs;
        self.shutdown = shutdown;
        Ok(())
    }
}

fn invalid(call: &Call, detail: &str) -> Erro {
    Erro::ArgumentoInvalido {
        nome: call.export.name,
        ordinal: call.export.ordinal,
        detalhe: detail.to_owned(),
    }
}

/// O objeto guest precisa estar em RAM gravável; MMIO descartaria o que o kernel escreve.
fn writable(bus: &Bus, call: &Call, address: u32, size: u32, what: &str) -> Result<(), Erro> {
    if ram_range(bus, address, size, Acesso::Escrita) {
        Ok(())
    } else {
        Err(invalid(call, &format!("{what} fora da RAM gravável")))
    }
}

impl super::Dispatcher {
    /// `ULONG HalGetInterruptVector(ULONG BusInterruptLevel, PKIRQL Irql)`
    pub(super) fn hal_get_interrupt_vector(bus: &mut Bus, call: &Call) -> Resultado {
        let level = call.argument(bus, 0)?;
        let irql = call.argument(bus, 1)?;
        if level > MAX_BUS_LEVEL {
            return Err(invalid(call, "nível de barramento fora de 0..=15"));
        }
        writable(bus, call, irql, 1, "KIRQL de saída")?;
        // 27 (PROFILE_LEVEL) − nível: declarado, ver o módulo.
        #[allow(clippy::cast_possible_truncation)]
        bus.escrever_u8(GuestVirt(irql), (27 - level) as u8)?;
        Ok(Atendimento {
            eax: VECTOR_BASE + level,
        })
    }

    /// `VOID KeInitializeInterrupt(PKINTERRUPT, ServiceRoutine, ServiceContext, Vector,
    /// Irql, InterruptMode, ShareVector)`
    pub(super) fn ke_initialize_interrupt(&mut self, bus: &mut Bus, call: &Call) -> Resultado {
        let interrupt = call.argument(bus, 0)?;
        let routine = call.argument(bus, 1)?;
        let context = call.argument(bus, 2)?;
        let vector = call.argument(bus, 3)?;
        let irql = call.argument(bus, 4)?;
        let mode = call.argument(bus, 5)?;
        let share = call.argument(bus, 6)? as u8;
        let level = vector
            .checked_sub(VECTOR_BASE)
            .filter(|&level| level <= MAX_BUS_LEVEL)
            .ok_or_else(|| invalid(call, "vetor fora de 0x30..=0x3F"))?;
        if mode > 1 || irql > 0xFF {
            return Err(invalid(call, "modo ou IRQL fora do intervalo"));
        }
        if self.interrupts.connected.contains(&interrupt) {
            return Err(invalid(call, "reinicializar uma interrupção conectada"));
        }
        writable(bus, call, interrupt, KI_HEADER, "KINTERRUPT")?;
        let mut header = [0u8; KI_HEADER as usize];
        header[0..4].copy_from_slice(&routine.to_le_bytes());
        header[4..8].copy_from_slice(&context.to_le_bytes());
        header[8..12].copy_from_slice(&level.to_le_bytes());
        header[12..16].copy_from_slice(&irql.to_le_bytes());
        header[16] = 0; // Connected
        header[17] = share;
        header[18] = mode as u8;
        // ServiceCount fica em 0; DispatchCode é do kernel real e não é preenchido.
        bus.escrever_bytes(GuestVirt(interrupt), &header)?;
        Ok(Atendimento { eax: 0 })
    }

    /// `BOOLEAN KeConnectInterrupt(PKINTERRUPT)`
    pub(super) fn ke_connect_interrupt(&mut self, bus: &mut Bus, call: &Call) -> Resultado {
        let interrupt = call.argument(bus, 0)?;
        writable(bus, call, interrupt, KI_HEADER, "KINTERRUPT")?;
        if bus_level(bus, interrupt).is_none() {
            return Err(invalid(call, "KINTERRUPT não inicializada"));
        }
        if bus.ler_u8(GuestVirt(interrupt + KI_CONNECTED))? != 0 {
            return Ok(Atendimento { eax: 0 }); // já conectada
        }
        let routine = bus.ler_u32(GuestVirt(interrupt + KI_SERVICE_ROUTINE))?;
        // Rotina que não é código executável seria um salto para dados na entrega.
        bus.traduzir(GuestVirt(routine), Acesso::Execucao)
            .map_err(|_| invalid(call, "rotina de serviço fora de memória executável"))?;
        if self.interrupts.connected.len() >= MAX_CONNECTED {
            return Err(invalid(call, "interrupções conectadas além do limite"));
        }
        bus.escrever_u8(GuestVirt(interrupt + KI_CONNECTED), 1)?;
        self.interrupts.connected.push(interrupt);
        Ok(Atendimento { eax: 1 })
    }

    /// `BOOLEAN KeDisconnectInterrupt(PKINTERRUPT)`
    pub(super) fn ke_disconnect_interrupt(&mut self, bus: &mut Bus, call: &Call) -> Resultado {
        let interrupt = call.argument(bus, 0)?;
        let Some(position) = self
            .interrupts
            .connected
            .iter()
            .position(|&connected| connected == interrupt)
        else {
            return Ok(Atendimento { eax: 0 });
        };
        bus.escrever_u8(GuestVirt(interrupt + KI_CONNECTED), 0)?;
        self.interrupts.connected.remove(position);
        Ok(Atendimento { eax: 1 })
    }

    /// `VOID KeInitializeDpc(PKDPC, DeferredRoutine, DeferredContext)`
    pub(super) fn ke_initialize_dpc(&mut self, bus: &mut Bus, call: &Call) -> Resultado {
        let dpc = call.argument(bus, 0)?;
        let routine = call.argument(bus, 1)?;
        let context = call.argument(bus, 2)?;
        writable(bus, call, dpc, KD_SIZE, "KDPC")?;
        if self.interrupts.dpcs.contains(&dpc) {
            return Err(invalid(call, "reinicializar um DPC enfileirado"));
        }
        let mut bytes = [0u8; KD_SIZE as usize];
        bytes[0..2].copy_from_slice(&DPC_OBJECT.to_le_bytes());
        bytes[12..16].copy_from_slice(&routine.to_le_bytes());
        bytes[16..20].copy_from_slice(&context.to_le_bytes());
        bus.escrever_bytes(GuestVirt(dpc), &bytes)?;
        Ok(Atendimento { eax: 0 })
    }

    /// `BOOLEAN KeInsertQueueDpc(PKDPC, SystemArgument1, SystemArgument2)`
    pub(super) fn ke_insert_queue_dpc(&mut self, bus: &mut Bus, call: &Call) -> Resultado {
        let dpc = call.argument(bus, 0)?;
        let arg1 = call.argument(bus, 1)?;
        let arg2 = call.argument(bus, 2)?;
        writable(bus, call, dpc, KD_SIZE, "KDPC")?;
        if bus.ler_u16(GuestVirt(dpc))? != DPC_OBJECT {
            return Err(invalid(call, "KDPC não inicializada"));
        }
        if bus.ler_u8(GuestVirt(dpc + KD_INSERTED))? != 0 {
            return Ok(Atendimento { eax: 0 }); // já na fila
        }
        if self.interrupts.dpcs.len() >= MAX_QUEUED_DPCS {
            return Err(invalid(call, "fila de DPCs além do limite"));
        }
        // `Inserted` (+2) e os dois argumentos (+20) ficam em lugares separados.
        let mut arguments = [0u8; 8];
        arguments[0..4].copy_from_slice(&arg1.to_le_bytes());
        arguments[4..8].copy_from_slice(&arg2.to_le_bytes());
        bus.escrever_u8(GuestVirt(dpc + KD_INSERTED), 1)?;
        bus.escrever_bytes(GuestVirt(dpc + KD_ARG1), &arguments)?;
        self.interrupts.dpcs.push_back(dpc);
        Ok(Atendimento { eax: 1 })
    }

    /// `BOOLEAN KeRemoveQueueDpc(PKDPC)`
    pub(super) fn ke_remove_queue_dpc(&mut self, bus: &mut Bus, call: &Call) -> Resultado {
        let dpc = call.argument(bus, 0)?;
        let Some(position) = self
            .interrupts
            .dpcs
            .iter()
            .position(|&queued| queued == dpc)
        else {
            return Ok(Atendimento { eax: 0 });
        };
        bus.escrever_u8(GuestVirt(dpc + KD_INSERTED), 0)?;
        self.interrupts.dpcs.remove(position);
        Ok(Atendimento { eax: 1 })
    }

    /// Há uma interrupção da GPU conectada e o vblank chega até a linha? Se sim, esperar
    /// um evento que o ISR/DPC sinaliza pode terminar.
    pub fn interrupt_can_signal(&self, bus: &Bus) -> bool {
        self.interrupts.gpu_connected(bus) && bus.nv2a_irq().vblank_armed()
    }

    /// A rotina de serviço a chamar agora, se a linha de interrupção está ligada.
    pub fn pending_interrupt(&self, bus: &Bus) -> Option<GuestCall> {
        self.interrupts.pending(bus)
    }

    /// O próximo DPC, já desmarcado como enfileirado.
    pub fn next_dpc(&mut self, bus: &mut Bus) -> Result<Option<GuestCall>, Erro> {
        self.interrupts.next_dpc(bus)
    }

    /// Há DPCs esperando para rodar?
    pub fn has_queued_dpcs(&self) -> bool {
        self.interrupts.has_queued_dpcs()
    }

    /// Entra em `DISPATCH_LEVEL` para um DPC e devolve o IRQL anterior.
    pub fn enter_dpc_level(&mut self) -> Irql {
        std::mem::replace(&mut self.irql, Irql::Dispatch)
    }

    /// Restaura o IRQL guardado por [`Self::enter_dpc_level`].
    pub fn restore_irql(&mut self, irql: Irql) {
        self.irql = irql;
    }
}

impl super::Dispatcher {
    /// A espera atual não pode terminar agora: a sessão deixa o tempo correr, entrega as
    /// interrupções e repete a chamada. Só a sessão chama; o pedido é consumido.
    pub fn take_block_request(&mut self) -> bool {
        std::mem::take(&mut self.pending_block)
    }
}

impl super::Dispatcher {
    /// `VOID HalRegisterShutdownNotification(PHAL_SHUTDOWN_REGISTRATION, BOOLEAN Register)`
    ///
    /// Só registra e remove: **nenhum caminho de desligamento chama as rotinas** ainda
    /// (`HalReturnToFirmware` não é um serviço implementado). O `pbkit` desregistra no
    /// início de `pb_kill` sem ter registrado antes, o que precisa ser inofensivo.
    pub(super) fn hal_register_shutdown_notification(
        &mut self,
        bus: &mut Bus,
        call: &Call,
    ) -> Resultado {
        let registration = call.argument(bus, 0)?;
        let register = call.argument(bus, 1)? as u8 != 0;
        let known = self
            .interrupts
            .shutdown
            .iter()
            .position(|&entry| entry == registration);
        if !register {
            if let Some(position) = known {
                self.interrupts.shutdown.remove(position);
            }
            return Ok(Atendimento { eax: 0 });
        }
        if !ram_range(bus, registration, 16, Acesso::Leitura) {
            return Err(invalid(call, "HAL_SHUTDOWN_REGISTRATION fora da RAM"));
        }
        let routine = bus.ler_u32(GuestVirt(registration))?;
        bus.traduzir(GuestVirt(routine), Acesso::Execucao)
            .map_err(|_| invalid(call, "rotina de desligamento fora de memória executável"))?;
        if known.is_none() {
            if self.interrupts.shutdown.len() >= MAX_SHUTDOWN {
                return Err(invalid(call, "notificações de desligamento além do limite"));
            }
            self.interrupts.shutdown.push(registration);
        }
        Ok(Atendimento { eax: 0 })
    }
}

impl super::Dispatcher {
    /// Eventos, interrupções e DPCs: o grupo de serviços que sinalizam e esperam.
    ///
    /// Separado do `dispatch` principal só para ele não crescer sem limite; a escolha do
    /// serviço continua sendo pelo ordinal.
    pub(super) fn dispatch_signals(&mut self, bus: &mut Bus, call: &Call) -> Resultado {
        use super::{
            EventOperation, ORDINAL_HAL_GET_INTERRUPT_VECTOR,
            ORDINAL_HAL_REGISTER_SHUTDOWN_NOTIFICATION, ORDINAL_KE_CONNECT_INTERRUPT,
            ORDINAL_KE_DISCONNECT_INTERRUPT, ORDINAL_KE_INITIALIZE_DPC,
            ORDINAL_KE_INITIALIZE_INTERRUPT, ORDINAL_KE_INSERT_QUEUE_DPC,
            ORDINAL_KE_REMOVE_QUEUE_DPC, ORDINAL_NT_CLEAR_EVENT, ORDINAL_NT_CREATE_EVENT,
            ORDINAL_NT_PULSE_EVENT, ORDINAL_NT_SET_EVENT,
        };
        match call.export.ordinal {
            ORDINAL_NT_CREATE_EVENT => self.nt_create_event(bus, call),
            ORDINAL_NT_SET_EVENT => self.nt_event_operation(bus, call, EventOperation::Set),
            ORDINAL_NT_CLEAR_EVENT => self.nt_event_operation(bus, call, EventOperation::Clear),
            ORDINAL_NT_PULSE_EVENT => self.nt_event_operation(bus, call, EventOperation::Pulse),
            ORDINAL_HAL_GET_INTERRUPT_VECTOR => Self::hal_get_interrupt_vector(bus, call),
            ORDINAL_HAL_REGISTER_SHUTDOWN_NOTIFICATION => {
                self.hal_register_shutdown_notification(bus, call)
            }
            ORDINAL_KE_INITIALIZE_INTERRUPT => self.ke_initialize_interrupt(bus, call),
            ORDINAL_KE_CONNECT_INTERRUPT => self.ke_connect_interrupt(bus, call),
            ORDINAL_KE_DISCONNECT_INTERRUPT => self.ke_disconnect_interrupt(bus, call),
            ORDINAL_KE_INITIALIZE_DPC => self.ke_initialize_dpc(bus, call),
            ORDINAL_KE_INSERT_QUEUE_DPC => self.ke_insert_queue_dpc(bus, call),
            ORDINAL_KE_REMOVE_QUEUE_DPC => self.ke_remove_queue_dpc(bus, call),
            ordinal => unreachable!("ordinal {ordinal} não pertence a este grupo"),
        }
    }
}
