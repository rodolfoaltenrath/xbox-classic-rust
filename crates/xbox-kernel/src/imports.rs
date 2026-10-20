//! Importações por ordinal. Os trampolins são endereços guest, nunca ponteiros host.

use xbox_core::Error as CoreError;
use xbox_formats::Xbe;
use xbox_mem::{Acesso, Bus, GuestVirt};

use crate::loader::Result;

mod catalog;
pub use catalog::lookup;

pub const TRAMPOLINE_BASE: u32 = 0x7FFE_0000;
pub const TRAMPOLINE_SIZE: u32 = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Abi {
    /// Quantidade de bytes de argumentos na pilha, sem o endereço de retorno.
    Stdcall(u16),
    /// Quantidade total de bytes, incluindo os argumentos em ECX/EDX.
    Fastcall(u16),
    Cdecl,
}

impl Abi {
    pub const fn stack_cleanup(self) -> u32 {
        match self {
            Self::Stdcall(bytes) => bytes as u32,
            Self::Fastcall(bytes) => bytes.saturating_sub(8) as u32,
            Self::Cdecl => 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Function(Abi),
    Data,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Export {
    pub ordinal: u32,
    pub name: &'static str,
    pub kind: Kind,
}

pub const fn trampoline(ordinal: u32) -> u32 {
    TRAMPOLINE_BASE + ordinal * 8
}

pub(crate) fn marker(ordinal: u32) -> [u8; 8] {
    let mut bytes = *b"HLE!\0\0\0\0";
    bytes[4..].copy_from_slice(&ordinal.to_le_bytes());
    bytes
}

/// Valida toda a tabela antes de modificar a imagem. Dados exportados precisam
/// de implementação própria; não são convertidos em funções nem zerados.
pub fn plan(xbe: &Xbe<'_>) -> Result<Vec<Export>> {
    if xbe.header.non_kernel_import_dir_address != 0 {
        return Err(CoreError::format("imports não-kernel ainda não implementados").into());
    }
    xbe.kernel_imports()?
        .into_iter()
        .map(|ordinal| {
            let export = lookup(ordinal).ok_or_else(|| {
                CoreError::format(format!("ordinal de kernel desconhecido: {ordinal}"))
            })?;
            if export.kind == Kind::Data {
                return Err(CoreError::format(format!(
                    "dado de kernel ainda não implementado: {} (ordinal {ordinal})",
                    export.name
                ))
                .into());
            }
            Ok(export)
        })
        .collect()
}

/// Confirma que o endereço é um trampolim instalado e imutável neste bus.
pub fn entry(bus: &Bus, pc: u32) -> Result<Export> {
    let offset = pc
        .checked_sub(TRAMPOLINE_BASE)
        .filter(|v| *v < TRAMPOLINE_SIZE && v.is_multiple_of(8))
        .ok_or_else(|| CoreError::format("endereço não é trampolim do kernel"))?;
    let ordinal = offset / 8;
    let export = lookup(ordinal)
        .filter(|e| matches!(e.kind, Kind::Function(_)))
        .ok_or_else(|| CoreError::format("trampolim sem função conhecida"))?;
    bus.traduzir(GuestVirt(pc), Acesso::Execucao)?;
    if bus.traduzir(GuestVirt(pc), Acesso::Escrita).is_ok() {
        return Err(CoreError::format("trampolim de kernel não pode ser gravável").into());
    }
    let mut bytes = [0; 8];
    bus.ler_bytes(GuestVirt(pc), &mut bytes)?;
    if bytes != marker(ordinal) {
        return Err(CoreError::format("trampolim não foi instalado para esta imagem").into());
    }
    Ok(export)
}

/// Fronteira ABI: captura a chamada sem executar serviço ou modificar CPU/pilha.
/// O estado pendente é reconstruído de EIP, registradores e RAM após snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Call {
    pub export: Export,
    pub return_pc: u32,
    pub return_esp: u32,
    esp: u32,
    ecx: u32,
    edx: u32,
}

impl Call {
    pub fn read(bus: &Bus, pc: u32, regs: &[u32; 8]) -> Result<Self> {
        let export = entry(bus, pc)?;
        let Kind::Function(abi) = export.kind else {
            unreachable!()
        };
        let esp = regs[4];
        let return_esp = esp
            .checked_add(4 + abi.stack_cleanup())
            .ok_or_else(|| CoreError::format("pilha transborda ao retornar do kernel"))?;
        let return_pc = bus.ler_u32(GuestVirt(esp))?;
        Ok(Self {
            export,
            return_pc,
            return_esp,
            esp,
            ecx: regs[1],
            edx: regs[2],
        })
    }

    /// Argumentos são palavras de 32 bits. Valores de 64 bits ocupam duas palavras.
    /// Cdecl variádico é limitado a 64 palavras; a interpretação cabe ao serviço.
    pub fn argument(&self, bus: &Bus, index: u32) -> Result<u32> {
        let Kind::Function(abi) = self.export.kind else {
            unreachable!()
        };
        let words = match abi {
            Abi::Stdcall(bytes) | Abi::Fastcall(bytes) => u32::from(bytes) / 4,
            Abi::Cdecl => 64,
        };
        if index >= words {
            return Err(CoreError::format("argumento fora da assinatura de kernel").into());
        }
        let stack_index = if matches!(abi, Abi::Fastcall(_)) {
            match index {
                0 => return Ok(self.ecx),
                1 => return Ok(self.edx),
                _ => index - 2,
            }
        } else {
            index
        };
        let address = self
            .esp
            .checked_add(4 + stack_index * 4)
            .ok_or_else(|| CoreError::format("endereço de argumento transborda"))?;
        Ok(bus.ler_u32(GuestVirt(address))?)
    }
}
