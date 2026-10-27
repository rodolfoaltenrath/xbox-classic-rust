//! Dispatcher dos serviços de kernel.
//!
//! O Rust gerado sai com [`crate::imports::Call`] na fronteira ABI; aqui o serviço é
//! executado, ou a falta dele é relatada de forma identificável. A regra que organiza
//! este módulo é a do guia do alpha: **nenhum sucesso genérico**. Ordinal sem serviço
//! devolve erro com nome, ordinal, endereço do chamador e argumentos — não `EAX = 0`.
//!
//! ## Por que fica em `xbox-kernel` e opera sobre `Bus`
//!
//! O dispatcher precisa ler argumentos e escrever efeitos na memória guest, mas **não**
//! pode depender de `Session`: `xbox-recomp` já depende deste crate, e o inverso
//! fecharia um ciclo. O chamador fornece `Bus`, PC, registradores e, para consultas temporais,
//! o Clock autoritativo da sessão por `atender_com_clock`. Quem aplica o
//! resultado na CPU é o chamador, por `Session::atender_kernel`. Isso também
//! mantém o dispatcher testável sem gerar e compilar código.
//!
//! ## Estado
//!
//! [`Dispatcher`] guarda contadores, referência UTC, IRQL da CPU, EEPROM, perfil PCI
//! e mutexes guest serializáveis. A sessão AOT é dona deste estado; salvar só
//! CPU/RAM não preserva os objetos de kernel ou a prioridade lógica da CPU.

use xbox_core::error::SnapshotError;
use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_mem::{Acesso, Bus, GuestVirt};

mod contiguous;
mod critical;
mod delay;
mod events;
mod files;
mod gpu_instance;
mod interlocked;
mod interrupts;
mod irql;
mod links;
mod mutants;
mod nvram;
mod pci;
mod sha;
mod smbus;
mod system_time;
mod threads;
mod video;
mod virtual_memory;
pub use contiguous::{
    CONTIGUOUS_WINDOW, ContiguousAllocation, ContiguousMemory, MAX_CONTIGUOUS_ALLOCATIONS,
    PAGE_WRITECOMBINE,
};
pub use critical::SECTION_SIZE as CRITICAL_SECTION_SIZE;
pub use delay::PedidoDeTempo;
use events::{EventOperation, Events};
pub use files::{MAX_FILE_HANDLES, MAX_MEDIA_BYTES};
pub use gpu_instance::GpuInstanceMemory;
pub use interrupts::{GPU_IRQ, GuestCall};
pub use irql::Irql;
pub use links::{MAX_LINK_HANDLES, STATUS_OBJECT_NAME_COLLISION, STATUS_OBJECT_NAME_NOT_FOUND};
use mutants::Mutants;
pub use mutants::{Aquisicao, INITIAL_THREAD, Liberacao, MAX_MUTANTS, MAX_RECURSION, Mutant};
pub use nvram::{EepromSource, EepromState, REG_BINARY, STATUS_BUFFER_TOO_SMALL, XC_MAX_ALL};
pub use pci::{PciConfig, PciProfile};
pub use sha::CONTEXT_SIZE as SHA_CONTEXT_SIZE;
pub use smbus::Smbus;
pub use system_time::SystemTimeReference;
pub use threads::{CHILD_THREAD, GuestThread, MAX_THREAD_STACK, ThreadPhase};
pub use video::{
    AV_PACK_STANDARD, COLOR_A8R8G8B8, DisplayMode, FramebufferView, HEIGHT as FRAMEBUFFER_HEIGHT,
    MODE_640_480_NTSC_M, VIDEO_BASE, VIDEO_ENC_FLICKERFILTER, VIDEO_ENC_GET_SETTINGS,
    VIDEO_ENC_SOFTEN_FILTER, VIDEO_ENC_VIDEOENABLE, VideoState, WIDTH as FRAMEBUFFER_WIDTH,
};
pub use virtual_memory::{
    ImageRange, MAX_VIRTUAL_ALLOCATIONS, MEM_COMMIT, MEM_RESERVE, PAGE_READWRITE,
    STATUS_INVALID_PARAMETER, STATUS_NO_MEMORY, VirtualAllocation, VirtualMemory,
};

use crate::imports::{Abi, Call, Kind};
use crate::loader::Result as LoaderResult;

/// Ordinal de `RtlInitAnsiString`.
pub const ORDINAL_RTL_INIT_ANSI_STRING: u32 = 289;
pub const ORDINAL_AV_SEND_TV_ENCODER_OPTION: u32 = 2;
pub const ORDINAL_AV_GET_SAVED_DATA_ADDRESS: u32 = 1;
pub const ORDINAL_AV_SET_DISPLAY_MODE: u32 = 3;
pub const ORDINAL_MM_ALLOCATE_CONTIGUOUS_MEMORY_EX: u32 = 166;
pub const ORDINAL_MM_ALLOCATE_CONTIGUOUS_MEMORY: u32 = 165;
pub const ORDINAL_MM_QUERY_ALLOCATION_SIZE: u32 = 180;
pub const ORDINAL_MM_CLAIM_GPU_INSTANCE_MEMORY: u32 = 168;
pub const ORDINAL_MM_FREE_CONTIGUOUS_MEMORY: u32 = 171;
pub const ORDINAL_HAL_WRITE_SMBUS_VALUE: u32 = 50;
pub const ORDINAL_IO_CREATE_SYMBOLIC_LINK: u32 = 67;
pub const ORDINAL_NT_OPEN_SYMBOLIC_LINK_OBJECT: u32 = 203;
pub const ORDINAL_NT_CLOSE: u32 = 187;
pub const ORDINAL_NT_CREATE_FILE: u32 = 190;
pub const ORDINAL_NT_OPEN_FILE: u32 = 202;
pub const ORDINAL_NT_QUERY_DIRECTORY_FILE: u32 = 207;
pub const ORDINAL_NT_QUERY_INFORMATION_FILE: u32 = 211;
pub const ORDINAL_NT_READ_FILE: u32 = 219;
pub const ORDINAL_RTL_ENTER_CRITICAL_SECTION: u32 = 277;
pub const ORDINAL_RTL_INITIALIZE_CRITICAL_SECTION: u32 = 291;
pub const ORDINAL_RTL_LEAVE_CRITICAL_SECTION: u32 = 294;
pub const ORDINAL_RTL_TRY_ENTER_CRITICAL_SECTION: u32 = 306;
/// Ordinal de `NtCreateMutant` (stdcall/12, sem `DesiredAccess` no Xbox).
pub const ORDINAL_NT_CREATE_MUTANT: u32 = 192;
pub const ORDINAL_NT_RELEASE_MUTANT: u32 = 221;
pub const ORDINAL_HAL_GET_INTERRUPT_VECTOR: u32 = 44;
pub const ORDINAL_HAL_REGISTER_SHUTDOWN_NOTIFICATION: u32 = 47;
pub const ORDINAL_KE_CONNECT_INTERRUPT: u32 = 98;
pub const ORDINAL_KE_DISCONNECT_INTERRUPT: u32 = 100;
pub const ORDINAL_KE_INITIALIZE_DPC: u32 = 107;
pub const ORDINAL_KE_INITIALIZE_INTERRUPT: u32 = 109;
pub const ORDINAL_KE_INSERT_QUEUE_DPC: u32 = 119;
pub const ORDINAL_KE_REMOVE_QUEUE_DPC: u32 = 137;
pub const ORDINAL_NT_CLEAR_EVENT: u32 = 186;
pub const ORDINAL_NT_CREATE_EVENT: u32 = 189;
pub const ORDINAL_NT_PULSE_EVENT: u32 = 205;
pub const ORDINAL_NT_SET_EVENT: u32 = 225;
pub const ORDINAL_NT_WAIT_FOR_SINGLE_OBJECT: u32 = 233;
pub const ORDINAL_EX_QUERY_NON_VOLATILE_SETTING: u32 = 24;
pub const ORDINAL_HAL_READ_WRITE_PCI_SPACE: u32 = 46;
pub const ORDINAL_INTERLOCKED_EXCHANGE: u32 = 54;
pub const ORDINAL_KE_GET_CURRENT_IRQL: u32 = 103;
pub const ORDINAL_KE_QUERY_SYSTEM_TIME: u32 = 128;
pub const ORDINAL_NT_ALLOCATE_VIRTUAL_MEMORY: u32 = 184;
pub const ORDINAL_KE_GET_CURRENT_THREAD: u32 = 104;
pub const ORDINAL_PS_CREATE_SYSTEM_THREAD_EX: u32 = 255;
pub const ORDINAL_PS_TERMINATE_SYSTEM_THREAD: u32 = 258;
pub const ORDINAL_KE_DELAY_EXECUTION_THREAD: u32 = 99;
pub const ORDINAL_XC_SHA_INIT: u32 = 335;
pub const ORDINAL_XC_SHA_UPDATE: u32 = 336;
pub const ORDINAL_XC_SHA_FINAL: u32 = 337;
pub const STATUS_SUCCESS: u32 = 0;
pub const STATUS_ACCESS_VIOLATION: u32 = 0xC000_0005;
pub const STATUS_INSUFFICIENT_RESOURCES: u32 = 0xC000_009A;
pub const STATUS_INVALID_HANDLE: u32 = 0xC000_0008;
/// Estouro da contagem de recursão de um mutant.
pub const STATUS_MUTANT_LIMIT_EXCEEDED: u32 = 0xC000_0191;
/// Liberação pedida por quem não é o proprietário do mutant.
pub const STATUS_MUTANT_NOT_OWNED: u32 = 0xC000_0046;
/// Espera satisfeita pelo objeto de índice zero.
///
/// Numericamente igual a [`STATUS_SUCCESS`], mas com significado próprio: é o valor
/// que o `mtx_lock` do nxdk compara para decidir se o lock foi adquirido.
pub const STATUS_WAIT_0: u32 = 0;

/// Ordinais cujo serviço está implementado.
///
/// Fonte única de verdade: [`crate::imports::servico_implementado`] e o relatório de
/// ordinais consultam esta lista, e um teste exige que ela case exatamente com o que
/// [`Dispatcher::atender`] sabe atender.
pub const IMPLEMENTADOS: &[u32] = &[
    ORDINAL_MM_ALLOCATE_CONTIGUOUS_MEMORY,
    ORDINAL_MM_QUERY_ALLOCATION_SIZE,
    ORDINAL_AV_SET_DISPLAY_MODE,
    ORDINAL_MM_ALLOCATE_CONTIGUOUS_MEMORY_EX,
    ORDINAL_MM_CLAIM_GPU_INSTANCE_MEMORY,
    ORDINAL_MM_FREE_CONTIGUOUS_MEMORY,
    ORDINAL_HAL_WRITE_SMBUS_VALUE,
    ORDINAL_AV_GET_SAVED_DATA_ADDRESS,
    ORDINAL_AV_SEND_TV_ENCODER_OPTION,
    ORDINAL_IO_CREATE_SYMBOLIC_LINK,
    ORDINAL_NT_OPEN_SYMBOLIC_LINK_OBJECT,
    ORDINAL_NT_CLOSE,
    ORDINAL_NT_CREATE_FILE,
    ORDINAL_NT_OPEN_FILE,
    ORDINAL_NT_QUERY_DIRECTORY_FILE,
    ORDINAL_NT_QUERY_INFORMATION_FILE,
    ORDINAL_NT_READ_FILE,
    ORDINAL_EX_QUERY_NON_VOLATILE_SETTING,
    ORDINAL_HAL_READ_WRITE_PCI_SPACE,
    ORDINAL_INTERLOCKED_EXCHANGE,
    ORDINAL_KE_GET_CURRENT_IRQL,
    ORDINAL_KE_GET_CURRENT_THREAD,
    ORDINAL_KE_QUERY_SYSTEM_TIME,
    ORDINAL_NT_ALLOCATE_VIRTUAL_MEMORY,
    ORDINAL_NT_CREATE_MUTANT,
    ORDINAL_NT_RELEASE_MUTANT,
    ORDINAL_HAL_GET_INTERRUPT_VECTOR,
    ORDINAL_HAL_REGISTER_SHUTDOWN_NOTIFICATION,
    ORDINAL_KE_CONNECT_INTERRUPT,
    ORDINAL_KE_DISCONNECT_INTERRUPT,
    ORDINAL_KE_INITIALIZE_DPC,
    ORDINAL_KE_INITIALIZE_INTERRUPT,
    ORDINAL_KE_INSERT_QUEUE_DPC,
    ORDINAL_KE_REMOVE_QUEUE_DPC,
    ORDINAL_NT_CLEAR_EVENT,
    ORDINAL_NT_CREATE_EVENT,
    ORDINAL_NT_PULSE_EVENT,
    ORDINAL_NT_SET_EVENT,
    ORDINAL_NT_WAIT_FOR_SINGLE_OBJECT,
    ORDINAL_PS_CREATE_SYSTEM_THREAD_EX,
    ORDINAL_PS_TERMINATE_SYSTEM_THREAD,
    ORDINAL_KE_DELAY_EXECUTION_THREAD,
    ORDINAL_RTL_ENTER_CRITICAL_SECTION,
    ORDINAL_RTL_INIT_ANSI_STRING,
    ORDINAL_RTL_INITIALIZE_CRITICAL_SECTION,
    ORDINAL_RTL_LEAVE_CRITICAL_SECTION,
    ORDINAL_RTL_TRY_ENTER_CRITICAL_SECTION,
    ORDINAL_XC_SHA_INIT,
    ORDINAL_XC_SHA_UPDATE,
    ORDINAL_XC_SHA_FINAL,
];

/// `Length`/`MaximumLength` de `ANSI_STRING` são `u16`: uma string sem NUL dentro desse
/// limite é erro, não truncamento silencioso.
const MAX_ANSI: u32 = u16::MAX as u32;

/// O serviço está implementado?
pub fn implementado(ordinal: u32) -> bool {
    IMPLEMENTADOS.contains(&ordinal)
}

/// Resultado de um serviço atendido.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Atendimento {
    /// Valor a colocar em `EAX`.
    ///
    /// Serviço `VOID` devolve zero por convenção nossa; o chamador não deve ler `EAX`
    /// nesse caso, e a ABI do catálogo é quem diz se há retorno.
    pub eax: u32,
}

/// Falha ao atender uma chamada de kernel.
#[derive(Debug, thiserror::Error)]
pub enum Erro {
    #[error("estado de vídeo guest inconsistente: {0}")]
    VideoState(&'static str),
    #[error("estado de memória contígua guest inconsistente: {0}")]
    MemoriaContigua(&'static str),
    #[error("estado de thread guest inconsistente: {0}")]
    ThreadState(&'static str),
    #[error("estado de memória virtual guest inconsistente: {0}")]
    MemoriaVirtual(&'static str),
    /// Falta de Clock explícito ou referência fora do intervalo de `LARGE_INTEGER`.
    #[error("KeQuerySystemTime (ordinal 128): tempo guest indisponível: {detalhe}")]
    ContextoTemporal { detalhe: &'static str },
    /// A fronteira ABI ou um acesso à memória guest falhou.
    #[error(transparent)]
    Fronteira(#[from] crate::loader::Error),

    /// Variante válida da API ainda não modelada: não simular falha do Xbox.
    #[error(
        "variante de kernel não implementada: {nome} (ordinal {ordinal}), chamado de 0x{chamador:08X}; {detalhe}; argumentos: {argumentos}"
    )]
    VarianteNaoImplementada {
        nome: &'static str,
        ordinal: u32,
        chamador: u32,
        detalhe: &'static str,
        argumentos: String,
    },

    /// O ordinal é conhecido e o serviço não existe.
    ///
    /// A mensagem carrega o que torna o relato acionável: quem chamou e com quê.
    #[error(
        "serviço de kernel não implementado: {nome} (ordinal {ordinal}), \
         chamado de 0x{chamador:08X}; argumentos: {argumentos}"
    )]
    NaoImplementado {
        /// Nome no catálogo.
        nome: &'static str,
        /// Ordinal importado.
        ordinal: u32,
        /// Endereço de retorno, isto é, de onde a chamada partiu.
        chamador: u32,
        /// Argumentos conforme a ABI declarada.
        argumentos: String,
    },

    /// O serviço existe, mas o argumento recebido está fora do que ele aceita.
    #[error("{nome} (ordinal {ordinal}) recebeu argumento inválido: {detalhe}")]
    ArgumentoInvalido {
        /// Nome no catálogo.
        nome: &'static str,
        /// Ordinal importado.
        ordinal: u32,
        /// O que estava errado.
        detalhe: String,
    },
}

/// Estado dos serviços de kernel.
#[derive(Debug, Default)]
pub struct Dispatcher {
    atendidas: u64,
    recusadas: u64,
    mutants: Mutants,
    events: Events,
    interrupts: interrupts::Interrupts,
    eeprom: EepromState,
    pci: PciConfig,
    system_time: SystemTimeReference,
    // Prioridade da única CPU guest, não da thread host nem de EFLAGS.IF.
    irql: Irql,
    virtual_memory: VirtualMemory,
    threads: threads::Threads,
    links: links::Links,
    video: VideoState,
    contiguous: ContiguousMemory,
    gpu_instance: GpuInstanceMemory,
    files: files::Files,
    // LED do SMC escrito por HalWriteSMBusValue (ADR-0090).
    smbus: smbus::Smbus,
    // Pedido de avanço de tempo deixado pelo último serviço; só a sessão consome.
    // Transiente: não entra em snapshot, porque a sessão o aplica antes de qualquer
    // ponto de salvamento (ver `Session::atender_kernel`).
    pending_time: Option<delay::PedidoDeTempo>,
    // A espera atual não termina sem tempo e interrupções; só a sessão consome. Transiente
    // como `pending_time`: a sessão o resolve antes de qualquer ponto de salvamento.
    pending_block: bool,
}

impl Dispatcher {
    /// Dispatcher sem estado acumulado.
    pub fn new() -> Self {
        Self::default()
    }

    /// Preserva todo o intervalo declarado da imagem, inclusive buracos sem páginas.
    pub fn for_image(base: u32, size: u32) -> xbox_core::Result<Self> {
        Ok(Self {
            virtual_memory: VirtualMemory::for_image(base, size)?,
            ..Self::default()
        })
    }

    pub const fn virtual_memory(&self) -> &VirtualMemory {
        &self.virtual_memory
    }
    pub const fn contiguous_memory(&self) -> &ContiguousMemory {
        &self.contiguous
    }

    /// Destino guest de D:, sem montagem ou acesso a caminhos do host.
    pub fn drive_d_target(&self) -> Option<&str> {
        self.links.target()
    }

    pub const fn gpu_instance_memory(&self) -> &GpuInstanceMemory {
        &self.gpu_instance
    }
    pub const fn video(&self) -> &VideoState {
        &self.video
    }
    pub fn framebuffer<'a>(&self, bus: &'a Bus) -> Result<Option<FramebufferView<'a>>, Erro> {
        self.video.framebuffer(bus, &self.contiguous, &self.eeprom)
    }

    /// Snapshots anteriores ao alocador dependem do XBE usado para abrir a sessão.
    /// Confere seus headers na RAM restaurada antes de recuperar esse contexto.
    pub fn restore_memory_context(
        &mut self,
        original: Option<ImageRange>,
        bus: &Bus,
    ) -> Result<(), SnapshotError> {
        self.virtual_memory.restore_context(original, bus)?;
        self.threads
            .validate(&self.mutants, &self.virtual_memory, Some(bus))?;
        self.contiguous
            .validate_bus(bus)
            .map_err(|what| SnapshotError::BadValue {
                section: "CMEM",
                what,
            })?;
        self.gpu_instance.validate(&self.contiguous, Some(bus))?;
        self.interrupts.validate(bus)?;
        self.video
            .validate_scanout(bus, &self.contiguous)
            .map_err(|what| SnapshotError::BadValue {
                section: "AVST",
                what,
            })?;
        self.video
            .validate_memory(&self.contiguous, &self.eeprom)
            .map_err(|what| SnapshotError::BadValue {
                section: "AVST",
                what,
            })
    }

    pub fn current_thread(&self) -> u32 {
        self.threads.current()
    }
    pub const fn child_thread(&self) -> Option<GuestThread> {
        self.threads.child
    }

    /// Só a sessão coordena a continuação CPU após consumir a criação Ready.
    pub fn start_ready_thread(&mut self) -> Option<GuestThread> {
        let child = self.threads.child.as_mut()?;
        if child.phase != ThreadPhase::Ready {
            return None;
        }
        child.phase = ThreadPhase::Running;
        Some(*child)
    }

    pub fn terminate_running_thread(&mut self, status: u32) -> Result<(), Erro> {
        if self.current_thread() != CHILD_THREAD {
            return Err(Erro::ThreadState(
                "término da thread inicial ainda sem modelo",
            ));
        }
        if self.mutants.has_owner(CHILD_THREAD) {
            return Err(Erro::ThreadState(
                "término com mutants adquiridos exige abandono ainda sem modelo",
            ));
        }
        if let Some(child) = &mut self.threads.child {
            child.phase = ThreadPhase::Terminated(status);
        }
        Ok(())
    }

    /// Origem explícita; sem leitura automática de arquivo ou alteração dos bytes.
    pub fn with_eeprom(eeprom: EepromState) -> Self {
        Self {
            eeprom,
            ..Self::default()
        }
    }

    pub const fn eeprom(&self) -> &EepromState {
        &self.eeprom
    }

    pub const fn pci(&self) -> &PciConfig {
        &self.pci
    }

    /// Referência UTC no power-on; não consulta o relógio host.
    pub fn with_system_time(system_time: SystemTimeReference) -> Self {
        Self {
            system_time,
            ..Self::default()
        }
    }

    pub const fn system_time(&self) -> &SystemTimeReference {
        &self.system_time
    }

    /// Contexto inicial explícito da única CPU guest. Não executa uma transição
    /// de prioridade nem entrega interrupções pendentes.
    pub fn with_irql(irql: Irql) -> Self {
        Self {
            irql,
            ..Self::default()
        }
    }

    /// Autoridade de IRQL deste perfil; independente de EFLAGS.IF.
    pub const fn irql(&self) -> Irql {
        self.irql
    }

    /// Chamadas atendidas por um serviço implementado.
    pub const fn atendidas(&self) -> u64 {
        self.atendidas
    }

    /// LED do SMC programado pelo guest (ADR-0090).
    pub const fn smbus(&self) -> &Smbus {
        &self.smbus
    }

    /// Chamadas recusadas por falta de serviço.
    pub const fn recusadas(&self) -> u64 {
        self.recusadas
    }

    /// Consulta de diagnóstico; não expõe ponteiro host ao título.
    pub fn mutant(&self, handle: u32) -> Option<&Mutant> {
        self.mutants.get(handle)
    }

    /// Atende a chamada de kernel em `pc`, sem contexto temporal.
    /// `KeQuerySystemTime` exige `atender_com_clock` e recusa esta entrada.
    ///
    /// `pc` tem de ser um trampolim instalado nesta imagem — [`Call::read`] verifica
    /// isso antes de qualquer efeito. A CPU não é alterada aqui: o chamador aplica
    /// `eax`, `esp` e `eip` depois, para que uma falha não deixe estado pela metade.
    pub fn atender(
        &mut self,
        bus: &mut Bus,
        pc: u32,
        regs: &[u32; 8],
    ) -> Result<Atendimento, Erro> {
        self.dispatch(bus, pc, regs, None, false)
    }

    /// Usa o Clock autoritativo da sessão; não cria outro contador de tempo.
    pub fn atender_com_clock(
        &mut self,
        bus: &mut Bus,
        pc: u32,
        regs: &[u32; 8],
        clock: &xbox_core::time::Clock,
    ) -> Result<Atendimento, Erro> {
        self.dispatch(bus, pc, regs, Some(clock), false)
    }

    /// Entrada da sessão com escalonador explícito; pedidos de bloqueio nunca
    /// são tratados como retornos concluídos pelo chamador.
    pub fn atender_escalonado(
        &mut self,
        bus: &mut Bus,
        pc: u32,
        regs: &[u32; 8],
        clock: &xbox_core::time::Clock,
    ) -> Result<Atendimento, Erro> {
        self.dispatch(bus, pc, regs, Some(clock), true)
    }
    /// Troca lógica da thread selecionada na única CPU, coordenada pela sessão.
    pub fn select_thread(&mut self, id: u32) -> Result<(), Erro> {
        if !matches!(id, 1 | CHILD_THREAD) {
            return Err(Erro::ThreadState(
                "identidade fora do perfil de duas threads",
            ));
        }
        if id == CHILD_THREAD && self.threads.child.is_none() {
            return Err(Erro::ThreadState("filha inexistente"));
        }
        if let Some(child) = &mut self.threads.child {
            if matches!(child.phase, ThreadPhase::Terminated(_)) {
                if id == CHILD_THREAD {
                    return Err(Erro::ThreadState("filha terminada"));
                }
            } else {
                child.phase = if id == CHILD_THREAD {
                    ThreadPhase::Running
                } else {
                    ThreadPhase::Ready
                };
            }
        }
        Ok(())
    }
    #[allow(clippy::too_many_lines)] // Tabela plana ordinal → serviço, uma linha por ordinal.
    fn dispatch(
        &mut self,
        bus: &mut Bus,
        pc: u32,
        regs: &[u32; 8],
        clock: Option<&xbox_core::time::Clock>,
        scheduled: bool,
    ) -> Result<Atendimento, Erro> {
        let call = Call::read(bus, pc, regs)?;
        let export = call.export;

        let resultado = match export.ordinal {
            ORDINAL_AV_SET_DISPLAY_MODE => {
                self.video
                    .set_mode(bus, &call, &self.eeprom, &self.contiguous)
            }
            ORDINAL_MM_ALLOCATE_CONTIGUOUS_MEMORY_EX => {
                self.contiguous.allocate(bus, &call, self.irql)
            }
            ORDINAL_MM_ALLOCATE_CONTIGUOUS_MEMORY => {
                self.contiguous.allocate_simple(bus, &call, self.irql)
            }
            ORDINAL_MM_QUERY_ALLOCATION_SIZE => self.contiguous.query_size(bus, &call, self.irql),
            ORDINAL_MM_CLAIM_GPU_INSTANCE_MEMORY => {
                self.gpu_instance
                    .query(bus, &call, self.irql, &mut self.contiguous)
            }
            ORDINAL_HAL_WRITE_SMBUS_VALUE => self.smbus.escrever(bus, &call),
            ORDINAL_MM_FREE_CONTIGUOUS_MEMORY => self.contiguous.free(
                bus,
                &call,
                self.irql,
                [
                    self.video.mode().map(DisplayMode::framebuffer),
                    bus.nv2a_pgraph().scanout().map(xbox_mem::GuestPhys),
                ],
                self.gpu_instance.allocation_base(),
            ),
            ORDINAL_AV_GET_SAVED_DATA_ADDRESS => Ok(Atendimento {
                eax: VideoState::SAVED_DATA_ADDRESS,
            }),
            ORDINAL_AV_SEND_TV_ENCODER_OPTION => self.video.encoder(bus, &call, &self.eeprom),
            ORDINAL_IO_CREATE_SYMBOLIC_LINK => self.links.create(bus, &call, self.irql),
            ORDINAL_NT_OPEN_SYMBOLIC_LINK_OBJECT => {
                self.links.open(bus, &call, self.irql, &mut self.mutants)
            }
            ORDINAL_NT_CLOSE => self.nt_close(bus, &call, scheduled),
            ORDINAL_NT_CREATE_FILE => self.nt_create_file(bus, &call),
            ORDINAL_NT_OPEN_FILE => self.nt_open_file(bus, &call),
            ORDINAL_NT_QUERY_DIRECTORY_FILE => self.nt_query_directory_file(bus, &call),
            ORDINAL_NT_READ_FILE => self.nt_read_file(bus, &call),
            ORDINAL_NT_QUERY_INFORMATION_FILE => self.nt_query_information_file(bus, &call),
            ORDINAL_PS_CREATE_SYSTEM_THREAD_EX => self.threads.create(
                bus,
                &call,
                &mut self.virtual_memory,
                &mut self.mutants,
                self.irql,
            ),
            ORDINAL_PS_TERMINATE_SYSTEM_THREAD => {
                let status = call.argument(bus, 0)?;
                self.terminate_running_thread(status)?;
                Ok(Atendimento { eax: 0 })
            }
            ORDINAL_KE_GET_CURRENT_THREAD => {
                if self.current_thread() == CHILD_THREAD {
                    Ok(Atendimento {
                        eax: self
                            .threads
                            .child
                            .ok_or(Erro::ThreadState("filha ausente"))?
                            .object(),
                    })
                } else {
                    Err(threads::variant(
                        bus,
                        &call,
                        "KTHREAD da thread inicial ainda não modelado",
                    ))
                }
            }
            ORDINAL_EX_QUERY_NON_VOLATILE_SETTING => self.eeprom.query(bus, &call),
            ORDINAL_HAL_READ_WRITE_PCI_SPACE => self.pci.transfer(bus, &call),
            ORDINAL_INTERLOCKED_EXCHANGE => interlocked::exchange(bus, &call),
            ORDINAL_KE_GET_CURRENT_IRQL => Ok(Atendimento {
                eax: u32::from(self.irql.level()),
            }),
            ORDINAL_KE_QUERY_SYSTEM_TIME => self.system_time.query(bus, &call, clock),
            ORDINAL_NT_ALLOCATE_VIRTUAL_MEMORY => {
                self.virtual_memory.allocate(bus, &call, self.irql)
            }
            ORDINAL_NT_CREATE_MUTANT => self.nt_create_mutant(bus, &call),
            ORDINAL_NT_RELEASE_MUTANT => self.nt_release_mutant(bus, &call),
            ORDINAL_NT_CREATE_EVENT
            | ORDINAL_NT_SET_EVENT
            | ORDINAL_NT_CLEAR_EVENT
            | ORDINAL_NT_PULSE_EVENT
            | ORDINAL_HAL_GET_INTERRUPT_VECTOR
            | ORDINAL_HAL_REGISTER_SHUTDOWN_NOTIFICATION
            | ORDINAL_KE_INITIALIZE_INTERRUPT
            | ORDINAL_KE_CONNECT_INTERRUPT
            | ORDINAL_KE_DISCONNECT_INTERRUPT
            | ORDINAL_KE_INITIALIZE_DPC
            | ORDINAL_KE_INSERT_QUEUE_DPC
            | ORDINAL_KE_REMOVE_QUEUE_DPC => self.dispatch_signals(bus, &call),
            ORDINAL_NT_WAIT_FOR_SINGLE_OBJECT => {
                self.nt_wait_for_single_object(bus, &call, scheduled)
            }
            ORDINAL_KE_DELAY_EXECUTION_THREAD => {
                self.ke_delay_execution_thread(bus, &call, scheduled)
            }
            ORDINAL_RTL_INIT_ANSI_STRING => rtl_init_ansi_string(bus, &call),
            ORDINAL_RTL_INITIALIZE_CRITICAL_SECTION => critical::initialize(bus, &call),
            ORDINAL_RTL_ENTER_CRITICAL_SECTION => {
                critical::enter(bus, &call, false, self.current_thread())
            }
            ORDINAL_RTL_TRY_ENTER_CRITICAL_SECTION => {
                critical::enter(bus, &call, true, self.current_thread())
            }
            ORDINAL_RTL_LEAVE_CRITICAL_SECTION => {
                critical::leave(bus, &call, self.current_thread())
            }
            ORDINAL_XC_SHA_INIT => sha::init(bus, &call),
            ORDINAL_XC_SHA_UPDATE => sha::update(bus, &call),
            ORDINAL_XC_SHA_FINAL => sha::finish(bus, &call),
            _ => {
                self.recusadas = self.recusadas.saturating_add(1);
                return Err(Erro::NaoImplementado {
                    nome: export.name,
                    ordinal: export.ordinal,
                    chamador: call.return_pc,
                    argumentos: descrever_argumentos(bus, &call),
                });
            }
        };

        if resultado.is_ok() {
            self.atendidas = self.atendidas.saturating_add(1);
        } else if matches!(resultado, Err(Erro::VarianteNaoImplementada { .. })) {
            self.recusadas = self.recusadas.saturating_add(1);
        }
        resultado
    }

    /// `NTSTATUS NtWaitForSingleObject(HANDLE, BOOLEAN Alertable, PLARGE_INTEGER Timeout)`
    ///
    /// Espera imediata em mutant ou thread já terminada. A espera
    /// **nunca bloqueia a thread do host**: ou a aquisição é
    /// imediata, ou o estado é um que este perfil não representa e vira erro
    /// identificável. É assim que o `mtx_lock` do nxdk chega aqui — handle, `FALSE`,
    /// timeout nulo — esperando `STATUS_WAIT_0`.
    fn nt_wait_for_single_object(
        &mut self,
        bus: &mut Bus,
        call: &Call,
        scheduled: bool,
    ) -> Resultado {
        let handle = call.argument(bus, 0)?;
        // BOOLEAN ocupa uma palavra na ABI, mas só o byte baixo importa.
        let alertable = call.argument(bus, 1)? as u8 != 0;
        let timeout = call.argument(bus, 2)?;

        let variante = |detalhe| {
            Err(Erro::VarianteNaoImplementada {
                nome: call.export.name,
                ordinal: call.export.ordinal,
                chamador: call.return_pc,
                detalhe,
                argumentos: descrever_argumentos(bus, call),
            })
        };

        if alertable {
            // Espera alertável exige entrega de APC, que não existe.
            return variante("espera alertável exige APCs, ainda não modeladas");
        }
        if timeout != 0 {
            // `mtx_timedlock`/`mtx_trylock` passam intervalo; exige relógio de espera.
            return variante("espera com timeout ainda não modelada");
        }

        if scheduled && self.irql != Irql::Passive {
            return variante("bloqueio escalonado exige Passive neste perfil");
        }
        if let Some(child) = self
            .child_thread()
            .filter(|child| child.handle == handle && !self.threads.handle_closed)
        {
            return if matches!(child.phase, ThreadPhase::Terminated(_)) {
                Ok(Atendimento { eax: STATUS_WAIT_0 })
            } else if scheduled {
                self.pending_block = true;
                Ok(Atendimento { eax: STATUS_WAIT_0 })
            } else {
                variante("espera de thread viva exige yield/escalonamento ainda não modelado")
            };
        }

        if self.files.contains(handle) {
            return Err(Erro::VarianteNaoImplementada {
                nome: call.export.name,
                ordinal: call.export.ordinal,
                chamador: call.return_pc,
                detalhe: "espera de objeto de arquivo fora do perfil de I/O síncrona",
                argumentos: descrever_argumentos(bus, call),
            });
        }
        if self.links.contains(handle) {
            return variante("link simbólico não é objeto aguardável");
        }
        if let Some(espera) = self.events.wait(handle) {
            return match espera {
                events::Espera::Satisfeita => Ok(Atendimento { eax: STATUS_WAIT_0 }),
                // Só um scheduler ou uma interrupção o sinalizariam; nenhum existe.
                events::Espera::Bloquearia => {
                    if scheduled || self.interrupt_can_signal(bus) {
                        // Há uma interrupção que pode sinalizar o evento: a sessão deixa o
                        // tempo correr até o vblank, entrega ISR e DPC e repete esta
                        // chamada. O retorno não é aplicado enquanto o bloqueio durar.
                        self.events.register_waiter(handle);
                        self.pending_block = true;
                        Ok(Atendimento { eax: STATUS_WAIT_0 })
                    } else {
                        variante("espera de evento não sinalizado exige scheduler ou interrupção")
                    }
                }
            };
        }
        // Handles de thread/link foram distinguidos; os demais pertencem a mutants.
        let Some(aquisicao) = self.mutants.acquire(handle, self.current_thread()) else {
            return Ok(Atendimento {
                eax: STATUS_INVALID_HANDLE,
            });
        };

        match aquisicao {
            Aquisicao::Adquirido(_) => Ok(Atendimento { eax: STATUS_WAIT_0 }),
            Aquisicao::LimiteDeRecursao => Ok(Atendimento {
                eax: STATUS_MUTANT_LIMIT_EXCEEDED,
            }),
            // Outro proprietário exige yield ainda ausente neste perfil de bootstrap.
            Aquisicao::OutroProprietario(_) if scheduled => {
                self.pending_block = true;
                Ok(Atendimento { eax: STATUS_WAIT_0 })
            }
            Aquisicao::OutroProprietario(_) => {
                variante("espera bloqueante exige scheduler de threads guest")
            }
        }
    }

    /// `NTSTATUS NtReleaseMutant(HANDLE MutantHandle, PLONG PreviousCount)`
    ///
    /// Par do serviço acima: é por aqui que o `mtx_unlock` do nxdk passa, com o handle
    /// e `PreviousCount` nulo.
    fn nt_release_mutant(&mut self, bus: &mut Bus, call: &Call) -> Resultado {
        let handle = call.argument(bus, 0)?;
        let previous_count = call.argument(bus, 1)?;

        if previous_count != 0 {
            // `PreviousCount` devolve o estado de sinal anterior do mutant, que não é a
            // contagem de recursão: a convenção do kernel relaciona os dois, e não
            // temos como verificar a relação exata com o alvo atual, que passa nulo.
            // Escrever um valor derivado de suposição seria resultado errado em
            // silêncio — o pior caso. Recusar a variante mantém o erro visível.
            return Err(Erro::VarianteNaoImplementada {
                nome: call.export.name,
                ordinal: call.export.ordinal,
                chamador: call.return_pc,
                detalhe: "PreviousCount exige a convenção de estado de sinal, não verificada",
                argumentos: descrever_argumentos(bus, call),
            });
        }

        let Some(liberacao) = self.mutants.release(handle, self.current_thread()) else {
            return Ok(Atendimento {
                eax: STATUS_INVALID_HANDLE,
            });
        };
        Ok(Atendimento {
            eax: match liberacao {
                Liberacao::Liberado { .. } => STATUS_SUCCESS,
                Liberacao::NaoEraProprietario => STATUS_MUTANT_NOT_OWNED,
            },
        })
    }

    fn nt_create_mutant(&mut self, bus: &mut Bus, call: &Call) -> Resultado {
        let output = call.argument(bus, 0)?;
        let attributes = call.argument(bus, 1)?;
        // BOOLEAN é de 8 bits, embora ocupe uma palavra na ABI.
        let initial_owner = call.argument(bus, 2)? as u8 != 0;
        if attributes != 0 {
            // OBJECT_ATTRIBUTES do Xbox tem 12 bytes: RootDirectory, ObjectName,
            // Attributes. Não usar o layout Win32/NT de desktop.
            if !ram_range(bus, attributes, 12, Acesso::Leitura) {
                return Ok(Atendimento {
                    eax: STATUS_ACCESS_VIOLATION,
                });
            }
            let mut bytes = [0; 12];
            bus.ler_bytes(GuestVirt(attributes), &mut bytes)?;
            if bytes != [0; 12] {
                return Err(Erro::VarianteNaoImplementada {
                    nome: call.export.name,
                    ordinal: call.export.ordinal,
                    chamador: call.return_pc,
                    detalhe: "nomes, diretórios e flags de OBJECT_ATTRIBUTES ainda não modelados",
                    argumentos: descrever_argumentos(bus, call),
                });
            }
        }
        if !ram_range(bus, output, 4, Acesso::Escrita) {
            return Ok(Atendimento {
                eax: STATUS_ACCESS_VIOLATION,
            });
        }
        let Some((handle, mut object)) = self.mutants.prepare(initial_owner) else {
            return Ok(Atendimento {
                eax: STATUS_INSUFFICIENT_RESOURCES,
            });
        };
        if initial_owner {
            object.owner = Some(self.current_thread());
        }
        // O store inteiro é validado pelo Bus antes de escrever; uma falta não
        // publica objeto, não consome handle e não deixa metade da saída escrita.
        bus.escrever_bytes(GuestVirt(output), &handle.to_le_bytes())?;
        self.mutants.commit(handle, object);
        Ok(Atendimento {
            eax: STATUS_SUCCESS,
        })
    }
}

/// Restringe ponteiros HLE a RAM. MMIO sem implementação não pode aceitar um
/// handle que seria descartado pelo barramento. Também evita wrap do endereço.
fn ram_range(bus: &Bus, address: u32, size: u32, access: Acesso) -> bool {
    address != 0
        && (0..size).all(|offset| {
            address
                .checked_add(offset)
                .and_then(|va| bus.traduzir(GuestVirt(va), access).ok())
                .is_some_and(|pa| (pa.0 as usize) < bus.ram())
        })
}

impl Snapshot for Dispatcher {
    fn save(&self, out: &mut SnapshotWriter) {
        out.section("KRNL", 15);
        out.u64(self.atendidas);
        out.u64(self.recusadas);
        self.mutants.save(out);
        self.eeprom.save(out);
        self.pci.save(out);
        self.system_time.save(out);
        self.irql.save(out);
        self.virtual_memory.save(out);
        self.threads.save(out);
        self.links.save(out);
        self.video.save(out);
        self.contiguous.save(out);
        self.events.save(out);
        self.interrupts.save(out);
        self.gpu_instance.save(out);
        self.files.save(out);
        self.smbus.save(out);
    }

    fn load(&mut self, inp: &mut SnapshotReader<'_>) -> Result<(), SnapshotError> {
        let version = inp.section("KRNL", 15)?;
        if version == 0 {
            return Err(SnapshotError::BadValue {
                section: "KRNL",
                what: "versão de kernel desconhecida",
            });
        }
        let atendidas = inp.u64()?;
        let recusadas = inp.u64()?;
        let mut mutants = Mutants::default();
        mutants.load(inp)?;
        // v1 não tinha EEPROM: migração explícita para o perfil sintético inicial,
        // independente dos bytes/configuração presentes na sessão de destino.
        let mut eeprom = EepromState::default();
        if version >= 2 {
            eeprom.load(inp)?;
        }
        let mut pci = PciConfig::default();
        if version >= 3 {
            pci.load(inp)?;
        }
        // v1–v3 não tinham referência UTC; migrar para o perfil determinístico,
        // nunca herdar a configuração da sessão de destino.
        let mut system_time = SystemTimeReference::default();
        if version >= 4 {
            system_time.load(inp)?;
        }
        // v1–v4 não modelavam IRQL, nem entregavam IRQ/DPC/APC. O contexto de
        // entrada passa explicitamente a PASSIVE, sem inferência de EFLAGS.IF.
        let mut irql = Irql::default();
        if version >= 5 {
            irql.load(inp)?;
        }
        let mut virtual_memory = VirtualMemory::legacy();
        if version >= 6 {
            virtual_memory.load(inp)?;
        }
        let mut threads = threads::Threads::default();
        if version >= 7 {
            threads.load(inp)?;
        }
        threads.validate(&mutants, &virtual_memory, None)?;
        let mut links = links::Links::default();
        if version >= 8 {
            links.load(inp)?;
        }
        links.validate(&mutants, threads.child)?;
        let mut video = VideoState::default();
        if version >= 9 {
            video.load(inp)?;
        }
        let mut contiguous = ContiguousMemory::default();
        if version >= 10 {
            contiguous.load(inp)?;
        }
        // v1–v10 não tinham eventos: migram para a tabela vazia.
        let mut events = Events::default();
        if version >= 11 {
            events.load(inp)?;
        }
        events.validate(&mutants, &links, threads.child)?;
        // v1–v11 não tinham interrupções nem DPCs: migram para listas vazias. A conferência
        // contra a memória guest acontece em `restore_memory_context`, quando o Bus já
        // foi restaurado.
        let mut interrupts = interrupts::Interrupts::default();
        if version >= 12 {
            interrupts.load(inp)?;
        }
        let mut gpu_instance = GpuInstanceMemory::default();
        if version >= 13 {
            gpu_instance.load(inp)?;
        }
        gpu_instance.validate(&contiguous, None)?;
        let mut files = files::Files::default();
        if version >= 14 {
            files.load(inp)?;
        }
        // v1–v14 não tinham SMBus: nenhuma escrita de LED registrada.
        let mut smbus = smbus::Smbus::default();
        if version >= 15 {
            smbus.load(inp)?;
        }
        video
            .validate_memory(&contiguous, &eeprom)
            .map_err(|what| SnapshotError::BadValue {
                section: "AVST",
                what,
            })?;
        *self = Self {
            atendidas,
            recusadas,
            mutants,
            events,
            interrupts,
            eeprom,
            pci,
            system_time,
            irql,
            virtual_memory,
            threads,
            links,
            video,
            contiguous,
            gpu_instance,
            files,
            smbus,
            // Deliberadamente `None`: o pedido de tempo vive só entre o atendimento e a
            // aplicação pela sessão, que ocorre antes de qualquer ponto de salvamento.
            // Um snapshot nunca o contém, e carregar um não pode ressuscitar um avanço
            // de relógio que já foi aplicado (ou descartado) na sessão de origem.
            pending_time: None,
            pending_block: false,
        };
        self.files.validate(self)?;
        Ok(())
    }
}

/// Descreve os argumentos conforme a ABI, para o relato de serviço faltante.
///
/// Um argumento ilegível aparece como `?`: a mensagem de diagnóstico não pode falhar
/// por causa do que ela está tentando diagnosticar.
fn descrever_argumentos(bus: &Bus, call: &Call) -> String {
    let (palavras, variadico) = match call.export.kind {
        Kind::Function(Abi::Stdcall(bytes) | Abi::Fastcall(bytes)) => (u32::from(bytes) / 4, false),
        // Cdecl não declara quantidade; mostrar as primeiras é pista, não assinatura.
        Kind::Function(Abi::Cdecl) => (4, true),
        Kind::Data => (0, false),
    };
    if palavras == 0 {
        return "nenhum".to_owned();
    }
    let mut partes = Vec::new();
    for indice in 0..palavras {
        match call.argument(bus, indice) {
            Ok(valor) => partes.push(format!("0x{valor:08X}")),
            Err(_) => partes.push("?".to_owned()),
        }
    }
    if variadico {
        partes.push("… (cdecl, quantidade não declarada)".to_owned());
    }
    partes.join(", ")
}

/// `VOID RtlInitAnsiString(PANSI_STRING DestinationString, PCSZ SourceString)`.
///
/// Efeito inteiro em memória guest: mede a string de origem e preenche os três campos
/// do destino. Função pura sobre a memória — nenhum estado de kernel, nada inventado,
/// e o resultado é verificável lendo o destino.
///
/// `SourceString` nulo zera o destino, que é o comportamento documentado.
fn rtl_init_ansi_string(bus: &mut Bus, call: &Call) -> Result<Atendimento, Erro> {
    let destino = call.argument(bus, 0)?;
    let origem = call.argument(bus, 1)?;
    let nome = call.export.name;
    let ordinal = call.export.ordinal;

    if destino == 0 {
        return Err(Erro::ArgumentoInvalido {
            nome,
            ordinal,
            detalhe: "DestinationString nulo".to_owned(),
        });
    }

    let (comprimento, maximo) = if origem == 0 {
        (0u16, 0u16)
    } else {
        let medido = medir_cstring(bus, origem).map_err(|detalhe| Erro::ArgumentoInvalido {
            nome,
            ordinal,
            detalhe,
        })?;
        (medido, medido + 1)
    };

    bus.escrever_u16(GuestVirt(destino), comprimento)
        .map_err(|e| Erro::Fronteira(e.into()))?;
    bus.escrever_u16(GuestVirt(destino + 2), maximo)
        .map_err(|e| Erro::Fronteira(e.into()))?;
    bus.escrever_u32(GuestVirt(destino + 4), origem)
        .map_err(|e| Erro::Fronteira(e.into()))?;

    // A função é VOID; zero é convenção nossa e o chamador não deve ler EAX.
    Ok(Atendimento { eax: 0 })
}

/// Comprimento de uma string terminada em NUL na memória guest.
///
/// Recusa string sem NUL dentro do que `ANSI_STRING` consegue representar, em vez de
/// truncar em silêncio.
fn medir_cstring(bus: &Bus, endereco: u32) -> Result<u16, String> {
    for deslocamento in 0..=MAX_ANSI {
        let atual = endereco
            .checked_add(deslocamento)
            .ok_or_else(|| format!("string em 0x{endereco:08X} transborda o espaço x86"))?;
        let byte = bus
            .ler_u8(GuestVirt(atual))
            .map_err(|e| format!("string em 0x{endereco:08X} ilegível: {e}"))?;
        if byte == 0 {
            return u16::try_from(deslocamento)
                .map_err(|_| format!("string em 0x{endereco:08X} excede ANSI_STRING"));
        }
    }
    Err(format!(
        "string em 0x{endereco:08X} não termina em NUL dentro de {MAX_ANSI} bytes"
    ))
}

/// Converte uma falta de memória em erro do dispatcher, preservando a mensagem.
impl From<xbox_mem::Falta> for Erro {
    fn from(f: xbox_mem::Falta) -> Self {
        Self::Fronteira(crate::loader::Error::Memory(f))
    }
}

/// Permite usar `?` em funções de serviço que chamam a fronteira de imports.
impl From<Erro> for crate::loader::Error {
    fn from(e: Erro) -> Self {
        match e {
            Erro::Fronteira(inner) => inner,
            outro => Self::Format(xbox_core::Error::format(outro.to_string())),
        }
    }
}

/// Resultado das funções deste módulo, para quem preferir o alias.
pub type Resultado = Result<Atendimento, Erro>;

/// Garante em tempo de compilação que o alias do loader continua utilizável aqui.
const _: fn() -> LoaderResult<()> = || Ok(());
