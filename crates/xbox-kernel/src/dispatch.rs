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
//! resultado na CPU é o chamador, por `Session::complete_kernel_call`. Isso também
//! mantém o dispatcher testável sem gerar e compilar código.
//!
//! ## Estado
//!
//! [`Dispatcher`] guarda o estado dos serviços. Hoje guarda apenas contadores de
//! diagnóstico, porque **um** serviço está implementado. Objetos, handles, threads e
//! tempo entram conforme os caminhos realmente executados exigirem.

use xbox_mem::{Bus, GuestVirt};

use crate::imports::{Abi, Call, Kind};
use crate::loader::Result as LoaderResult;

/// Ordinal de `RtlInitAnsiString`.
pub const ORDINAL_RTL_INIT_ANSI_STRING: u32 = 289;

/// Ordinais cujo serviço está implementado.
///
/// Fonte única de verdade: [`crate::imports::servico_implementado`] e o relatório de
/// ordinais consultam esta lista, e um teste exige que ela case exatamente com o que
/// [`Dispatcher::atender`] sabe atender.
pub const IMPLEMENTADOS: &[u32] = &[ORDINAL_RTL_INIT_ANSI_STRING];

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
}

impl Dispatcher {
    /// Dispatcher sem estado acumulado.
    pub fn new() -> Self {
        Self::default()
    }

    /// Chamadas atendidas por um serviço implementado.
    pub const fn atendidas(&self) -> u64 {
        self.atendidas
    }

    /// Chamadas recusadas por falta de serviço.
    pub const fn recusadas(&self) -> u64 {
        self.recusadas
    }

    /// Atende a chamada de kernel em `pc`.
    ///
    /// `pc` tem de ser um trampolim instalado nesta imagem — [`Call::read`] verifica
    /// isso antes de qualquer efeito. A CPU não é alterada aqui: o chamador aplica
    /// `eax`, `esp` e `eip` depois, para que uma falha não deixe estado pela metade.
    // Com um serviço implementado o `match` parece uma comparação de igualdade, mas é a
    // tabela de despacho: cada serviço novo é um braço. Trocar por `if` agora só para
    // satisfazer o lint seria desfeito no próximo serviço.
    #[allow(clippy::single_match_else)]
    pub fn atender(
        &mut self,
        bus: &mut Bus,
        pc: u32,
        regs: &[u32; 8],
    ) -> Result<Atendimento, Erro> {
        let call = Call::read(bus, pc, regs)?;
        let export = call.export;

        let resultado = match export.ordinal {
            ORDINAL_RTL_INIT_ANSI_STRING => rtl_init_ansi_string(bus, &call),
            _ => {
                self.recusadas += 1;
                return Err(Erro::NaoImplementado {
                    nome: export.name,
                    ordinal: export.ordinal,
                    chamador: call.return_pc,
                    argumentos: descrever_argumentos(bus, &call),
                });
            }
        };

        if resultado.is_ok() {
            self.atendidas += 1;
        }
        resultado
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
