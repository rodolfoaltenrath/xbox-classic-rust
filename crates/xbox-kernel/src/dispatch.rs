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
//! fecharia um ciclo. Então a assinatura é `(&mut Bus, pc, regs)` e quem aplica o
//! resultado na CPU é o chamador, por `Session::atender_kernel`. Isso também
//! mantém o dispatcher testável sem gerar e compilar código.
//!
//! ## Estado
//!
//! [`Dispatcher`] guarda contadores, EEPROM, perfil PCI e mutexes guest serializáveis. A sessão AOT é
//! dona deste estado; salvar só CPU/RAM não preserva os objetos de kernel.

use xbox_core::error::SnapshotError;
use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_mem::{Acesso, Bus, GuestVirt};

mod critical;
mod interlocked;
mod mutants;
mod nvram;
mod pci;
mod sha;
pub use critical::SECTION_SIZE as CRITICAL_SECTION_SIZE;
use mutants::Mutants;
pub use mutants::{Aquisicao, INITIAL_THREAD, Liberacao, MAX_MUTANTS, MAX_RECURSION, Mutant};
pub use nvram::{EepromSource, EepromState, REG_BINARY, STATUS_BUFFER_TOO_SMALL, XC_MAX_ALL};
pub use pci::{PciConfig, PciProfile};
pub use sha::CONTEXT_SIZE as SHA_CONTEXT_SIZE;

use crate::imports::{Abi, Call, Kind};
use crate::loader::Result as LoaderResult;

/// Ordinal de `RtlInitAnsiString`.
pub const ORDINAL_RTL_INIT_ANSI_STRING: u32 = 289;
pub const ORDINAL_RTL_ENTER_CRITICAL_SECTION: u32 = 277;
pub const ORDINAL_RTL_INITIALIZE_CRITICAL_SECTION: u32 = 291;
pub const ORDINAL_RTL_LEAVE_CRITICAL_SECTION: u32 = 294;
pub const ORDINAL_RTL_TRY_ENTER_CRITICAL_SECTION: u32 = 306;
/// Ordinal de `NtCreateMutant` (stdcall/12, sem `DesiredAccess` no Xbox).
pub const ORDINAL_NT_CREATE_MUTANT: u32 = 192;
pub const ORDINAL_NT_RELEASE_MUTANT: u32 = 221;
pub const ORDINAL_NT_WAIT_FOR_SINGLE_OBJECT: u32 = 233;
pub const ORDINAL_EX_QUERY_NON_VOLATILE_SETTING: u32 = 24;
pub const ORDINAL_HAL_READ_WRITE_PCI_SPACE: u32 = 46;
pub const ORDINAL_INTERLOCKED_EXCHANGE: u32 = 54;
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
    ORDINAL_EX_QUERY_NON_VOLATILE_SETTING,
    ORDINAL_HAL_READ_WRITE_PCI_SPACE,
    ORDINAL_INTERLOCKED_EXCHANGE,
    ORDINAL_NT_CREATE_MUTANT,
    ORDINAL_NT_RELEASE_MUTANT,
    ORDINAL_NT_WAIT_FOR_SINGLE_OBJECT,
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
    eeprom: EepromState,
    pci: PciConfig,
}

impl Dispatcher {
    /// Dispatcher sem estado acumulado.
    pub fn new() -> Self {
        Self::default()
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

    /// Chamadas atendidas por um serviço implementado.
    pub const fn atendidas(&self) -> u64 {
        self.atendidas
    }

    /// Chamadas recusadas por falta de serviço.
    pub const fn recusadas(&self) -> u64 {
        self.recusadas
    }

    /// Consulta de diagnóstico; não expõe ponteiro host ao título.
    pub fn mutant(&self, handle: u32) -> Option<&Mutant> {
        self.mutants.get(handle)
    }

    /// Atende a chamada de kernel em `pc`.
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
        let call = Call::read(bus, pc, regs)?;
        let export = call.export;

        let resultado = match export.ordinal {
            ORDINAL_EX_QUERY_NON_VOLATILE_SETTING => self.eeprom.query(bus, &call),
            ORDINAL_HAL_READ_WRITE_PCI_SPACE => self.pci.transfer(bus, &call),
            ORDINAL_INTERLOCKED_EXCHANGE => interlocked::exchange(bus, &call),
            ORDINAL_NT_CREATE_MUTANT => self.nt_create_mutant(bus, &call),
            ORDINAL_NT_RELEASE_MUTANT => self.nt_release_mutant(bus, &call),
            ORDINAL_NT_WAIT_FOR_SINGLE_OBJECT => self.nt_wait_for_single_object(bus, &call),
            ORDINAL_RTL_INIT_ANSI_STRING => rtl_init_ansi_string(bus, &call),
            ORDINAL_RTL_INITIALIZE_CRITICAL_SECTION => critical::initialize(bus, &call),
            ORDINAL_RTL_ENTER_CRITICAL_SECTION => critical::enter(bus, &call, false),
            ORDINAL_RTL_TRY_ENTER_CRITICAL_SECTION => critical::enter(bus, &call, true),
            ORDINAL_RTL_LEAVE_CRITICAL_SECTION => critical::leave(bus, &call),
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
    /// Só sabe esperar em mutant, que é o único objeto que este kernel cria. A espera
    /// **nunca bloqueia a thread do host**: com uma thread guest, ou a aquisição é
    /// imediata, ou o estado é um que este perfil não representa e vira erro
    /// identificável. É assim que o `mtx_lock` do nxdk chega aqui — handle, `FALSE`,
    /// timeout nulo — esperando `STATUS_WAIT_0`.
    fn nt_wait_for_single_object(&mut self, bus: &mut Bus, call: &Call) -> Resultado {
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

        // Todo handle existente neste kernel vem de `NtCreateMutant`; nenhum outro
        // serviço cria objeto. Então handle fora da tabela é genuinamente inválido, e
        // não "objeto de tipo que não modelamos" — se outro tipo passar a existir, este
        // caminho precisa distinguir os dois casos.
        let Some(aquisicao) = self.mutants.acquire(handle, INITIAL_THREAD) else {
            return Ok(Atendimento {
                eax: STATUS_INVALID_HANDLE,
            });
        };

        match aquisicao {
            Aquisicao::Adquirido(_) => Ok(Atendimento { eax: STATUS_WAIT_0 }),
            Aquisicao::LimiteDeRecursao => Ok(Atendimento {
                eax: STATUS_MUTANT_LIMIT_EXCEEDED,
            }),
            // Inalcançável com uma thread só. Se acontecer, é bug nosso ou feature
            // nova: relatar, nunca bloquear o host nem devolver sucesso.
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

        let Some(liberacao) = self.mutants.release(handle, INITIAL_THREAD) else {
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
        let Some((handle, object)) = self.mutants.prepare(initial_owner) else {
            return Ok(Atendimento {
                eax: STATUS_INSUFFICIENT_RESOURCES,
            });
        };
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
        out.section("KRNL", 3);
        out.u64(self.atendidas);
        out.u64(self.recusadas);
        self.mutants.save(out);
        self.eeprom.save(out);
        self.pci.save(out);
    }

    fn load(&mut self, inp: &mut SnapshotReader<'_>) -> Result<(), SnapshotError> {
        let version = inp.section("KRNL", 3)?;
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
        *self = Self {
            atendidas,
            recusadas,
            mutants,
            eeprom,
            pci,
        };
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
