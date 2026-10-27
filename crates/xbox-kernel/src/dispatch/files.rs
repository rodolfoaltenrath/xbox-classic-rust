//! Executável original como arquivo de mídia imutável (ADR-0083, P22).
//! Não abre caminhos do host nem representa um filesystem de disco completo.
use super::{
    Atendimento, Dispatcher, Erro, Irql, Resultado, STATUS_ACCESS_VIOLATION,
    STATUS_INSUFFICIENT_RESOURCES, STATUS_INVALID_HANDLE, STATUS_OBJECT_NAME_NOT_FOUND,
    descrever_argumentos, ram_range,
};
use crate::imports::Call;
use std::collections::BTreeMap;
use xbox_core::{Snapshot, SnapshotReader, SnapshotWriter, error::SnapshotError};
use xbox_mem::{Acesso, Bus, GuestVirt};
mod directories;

pub const MAX_MEDIA_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_FILE_HANDLES: usize = 256;
const MAX_READ: u32 = 1024 * 1024;
const EOF: u32 = 0xC0000011;
const SHARING: u32 = 0xC0000043;
const TOO_SMALL: u32 = 0xC0000004;
const PATH: &[u8] = b"\\Device\\CdRom0\\default.xbe";

#[derive(Debug, Clone, Copy)]
struct File {
    position: u64,
    share: u32,
}
#[derive(Debug, Default)]
pub(super) struct Files {
    image: Option<Vec<u8>>,
    handles: BTreeMap<u32, File>,
    directories: directories::Directories,
}
impl Files {
    pub(super) fn mount(&mut self, bytes: &[u8]) -> xbox_core::Result<()> {
        if self.image.is_some() || bytes.is_empty() || bytes.len() > MAX_MEDIA_BYTES {
            return Err(xbox_core::Error::format(
                "mídia exige imagem nova de 1 byte até 32 MiB",
            ));
        }
        self.image = Some(bytes.to_vec());
        self.directories.enable();
        Ok(())
    }
    pub(super) fn image(&self) -> Option<&[u8]> {
        self.image.as_deref()
    }
    pub(super) fn close(&mut self, handle: u32) -> bool {
        self.handles.remove(&handle).is_some() || self.directories.close(handle)
    }
    pub(super) fn contains(&self, handle: u32) -> bool {
        self.handles.contains_key(&handle) || self.directories.contains(handle)
    }
    pub(super) fn validate(&self, d: &Dispatcher) -> Result<(), SnapshotError> {
        self.directories.validate(self, d)?;
        if self.handles.len() > 1 && self.handles.values().any(|file| file.share & 1 == 0) {
            return Err(bad(
                "handles simultâneos violam compartilhamento de leitura",
            ));
        }
        for (&handle, file) in &self.handles {
            if self.image.is_none()
                || handle < 4
                || !handle.is_multiple_of(4)
                || handle >= d.mutants.next_handle()
                || d.mutants.get(handle).is_some()
                || d.links.contains(handle)
                || d.events.get(handle).is_some()
                || d.threads.child.is_some_and(|child| child.handle == handle)
                || file.position > i64::MAX as u64
                || file.share > 7
            {
                return Err(bad("handle/posição/share de arquivo inválido ou colidente"));
            }
        }
        Ok(())
    }
}
fn bad(what: &'static str) -> SnapshotError {
    SnapshotError::BadValue {
        section: "CDFS",
        what,
    }
}
impl Snapshot for Files {
    fn save(&self, out: &mut SnapshotWriter) {
        out.section("CDFS", 2);
        out.bool(self.image.is_some());
        if let Some(image) = &self.image {
            out.u32(image.len() as u32);
            out.raw(image);
        }
        out.u32(self.handles.len() as u32);
        for (&handle, file) in &self.handles {
            out.u32(handle);
            out.u64(file.position);
            out.u32(file.share);
        }
        self.directories.save(out);
    }
    fn load(&mut self, inp: &mut SnapshotReader<'_>) -> Result<(), SnapshotError> {
        let version = inp.section("CDFS", 2)?;
        if version == 0 {
            return Err(bad("versão desconhecida"));
        }
        let image = if inp.bool()? {
            let len = inp.u32()? as usize;
            if len == 0 || len > MAX_MEDIA_BYTES {
                return Err(bad("tamanho de mídia fora do limite"));
            }
            Some(inp.raw(len)?.to_vec())
        } else {
            None
        };
        let count = inp.u32()? as usize;
        if count > MAX_FILE_HANDLES || image.is_none() && count != 0 {
            return Err(bad("contagem de arquivos inválida"));
        }
        let mut handles = BTreeMap::new();
        let mut previous = 0;
        for _ in 0..count {
            let handle = inp.u32()?;
            let position = inp.u64()?;
            let share = inp.u32()?;
            if handle <= previous || position > i64::MAX as u64 || share > 7 {
                return Err(bad("registros de arquivo inválidos"));
            }
            handles.insert(handle, File { position, share });
            previous = handle;
        }
        let mut directories = directories::Directories::default();
        if version >= 2 {
            directories.load(inp)?;
        }
        *self = Self {
            image,
            handles,
            directories,
        };
        Ok(())
    }
}
fn variant(bus: &Bus, call: &Call, detalhe: &'static str) -> Erro {
    Erro::VarianteNaoImplementada {
        nome: call.export.name,
        ordinal: call.export.ordinal,
        chamador: call.return_pc,
        detalhe,
        argumentos: descrever_argumentos(bus, call),
    }
}
fn passive(d: &Dispatcher, bus: &Bus, call: &Call) -> Result<(), Erro> {
    if d.irql != Irql::Passive {
        return Err(variant(bus, call, "I/O de arquivo exige PASSIVE_LEVEL"));
    }
    Ok(())
}
fn overlap(bus: &Bus, a: u32, n: u32, b: u32, m: u32) -> bool {
    fn spans(bus: &Bus, mut at: u32, mut len: u32) -> Vec<(u64, u64)> {
        let mut result = Vec::new();
        while len != 0 {
            let count = len.min(4096 - (at & 4095));
            let physical = u64::from(
                bus.traduzir(GuestVirt(at), Acesso::Escrita)
                    .expect("saída validada")
                    .0,
            );
            result.push((physical, physical + u64::from(count)));
            at = at.wrapping_add(count);
            len -= count;
        }
        result
    }
    let first = spans(bus, a, n);
    spans(bus, b, m)
        .into_iter()
        .any(|(x, y)| first.iter().any(|&(p, q)| p < y && x < q))
}
fn iosb(bus: &mut Bus, at: u32, result: u32, count: u32) -> Result<(), Erro> {
    let mut bytes = [0u8; 8];
    bytes[..4].copy_from_slice(&result.to_le_bytes());
    bytes[4..].copy_from_slice(&count.to_le_bytes());
    bus.escrever_bytes(GuestVirt(at), &bytes)?;
    Ok(())
}
fn name(bus: &Bus, at: u32) -> Option<Vec<u8>> {
    if !ram_range(bus, at, 8, Acesso::Leitura) {
        return None;
    }
    let len = u32::from(bus.ler_u16(GuestVirt(at)).ok()?);
    let maximum = u32::from(bus.ler_u16(GuestVirt(at + 2)).ok()?);
    let pointer = bus.ler_u32(GuestVirt(at + 4)).ok()?;
    if len == 0 || len > 260 || len > maximum || !ram_range(bus, pointer, len, Acesso::Leitura) {
        return None;
    }
    let mut bytes = vec![0; len as usize];
    bus.ler_bytes(GuestVirt(pointer), &mut bytes).ok()?;
    Some(bytes)
}
fn validate_open_args(bus: &Bus, call: &Call, args: &[u32]) -> Result<(), Erro> {
    if args[1] & !0x80100081 != 0
        || args[1] & 0x80000000 == 0 && args[1] & 0x81 != 0x81
        || args[1] & 0x100000 == 0
        || args[4] != 0
        || args[6] > 7
        || args[7] != 1
        || !matches!(args[8], 0x20 | 0x60)
    {
        return Err(variant(
            bus,
            call,
            "somente FILE_OPEN/read/synchronous nonalert, sem allocation size",
        ));
    }
    Ok(())
}
impl Dispatcher {
    /// Publica o arquivo original de entrada uma única vez; sem handles/arquivos host.
    pub fn mount_executable_media(&mut self, bytes: &[u8]) -> xbox_core::Result<()> {
        self.files.mount(bytes)
    }
    pub fn executable_media(&self) -> Option<&[u8]> {
        self.files.image()
    }
    pub(super) fn nt_create_file(&mut self, bus: &mut Bus, call: &Call) -> Resultado {
        passive(self, bus, call)?;
        let args: Vec<u32> = (0..9)
            .map(|i| call.argument(bus, i))
            .collect::<Result<_, _>>()?;
        let (output, attributes, io) = (args[0], args[2], args[3]);
        if !ram_range(bus, output, 4, Acesso::Escrita)
            || !ram_range(bus, io, 8, Acesso::Escrita)
            || !ram_range(bus, attributes, 12, Acesso::Leitura)
        {
            return Ok(Atendimento {
                eax: STATUS_ACCESS_VIOLATION,
            });
        }
        if overlap(bus, output, 4, io, 8) {
            return Err(variant(bus, call, "saídas handle/IOSB sobrepostas"));
        }
        if self.files.image.is_none() {
            return Err(variant(bus, call, "sessão sem mídia de executável"));
        }
        validate_open_args(bus, call, &args)?;
        let path = match self.file_name(bus, call, attributes)? {
            Ok(path) => path,
            Err(status) => {
                if status == STATUS_OBJECT_NAME_NOT_FOUND {
                    iosb(bus, io, status, 0)?;
                }
                return Ok(Atendimento { eax: status });
            }
        };
        let flags = bus.ler_u32(GuestVirt(attributes + 8))?;
        let matches = if flags == 0x40 {
            path.eq_ignore_ascii_case(PATH)
        } else {
            path == PATH
        };
        if !matches {
            iosb(bus, io, STATUS_OBJECT_NAME_NOT_FOUND, 0)?;
            return Ok(Atendimento {
                eax: STATUS_OBJECT_NAME_NOT_FOUND,
            });
        }
        self.open_file_handle(bus, output, io, args[6])
    }
    // O resultado interno é NTSTATUS de memória/nome; o externo é variante sem modelo.
    fn file_name(
        &self,
        bus: &Bus,
        call: &Call,
        attributes: u32,
    ) -> Result<Result<Vec<u8>, u32>, Erro> {
        let root = bus.ler_u32(GuestVirt(attributes))?;
        let at = bus.ler_u32(GuestVirt(attributes + 4))?;
        let flags = bus.ler_u32(GuestVirt(attributes + 8))?;
        if !matches!(root, 0 | 0xFFFFFFFD) || !matches!(flags, 0 | 0x40) {
            return Err(variant(
                bus,
                call,
                "root/atributos OBJECT_ATTRIBUTES fora do perfil",
            ));
        }
        let Some(mut path) = name(bus, at) else {
            return Ok(Err(STATUS_ACCESS_VIOLATION));
        };
        if !path.is_ascii()
            || path.contains(&0)
            || path.contains(&b'/')
            || path.split(|b| *b == b'\\').any(|p| p == b"." || p == b"..")
        {
            return Err(variant(
                bus,
                call,
                "nome deve ser ASCII NT, sem NUL, slash ou traversal",
            ));
        }
        let dos = if root == 0xFFFFFFFD {
            Some(path.as_slice())
        } else {
            path.strip_prefix(b"\\??\\")
        };
        if let Some(relative) = dos {
            if relative.len() < 3
                || !relative[..2].eq_ignore_ascii_case(b"D:")
                || relative[2] != b'\\'
            {
                return Err(variant(bus, call, "somente namespace D: montado"));
            }
            let Some(target) = self.links.target() else {
                return Ok(Err(STATUS_OBJECT_NAME_NOT_FOUND));
            };
            let mut resolved = target.as_bytes().to_vec();
            resolved.extend(&relative[3..]);
            path = resolved;
        } else if root != 0 {
            return Err(variant(bus, call, "nome relativo sem namespace suportado"));
        }
        let device = b"\\Device\\CdRom0\\";
        if !path.eq_ignore_ascii_case(&device[..device.len() - 1])
            && !path
                .get(..device.len())
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(device))
        {
            return Err(variant(
                bus,
                call,
                "dispositivo fora da mídia CdRom0 modelada",
            ));
        }
        Ok(Ok(path))
    }
    fn open_file_handle(&mut self, bus: &mut Bus, output: u32, io: u32, share: u32) -> Resultado {
        if self
            .files
            .handles
            .values()
            .any(|file| file.share & 1 == 0 || share & 1 == 0)
        {
            iosb(bus, io, SHARING, 0)?;
            return Ok(Atendimento { eax: SHARING });
        }
        let Some((handle, _)) = self
            .mutants
            .prepare(false)
            .filter(|_| self.files.handles.len() + self.files.directories.len() < MAX_FILE_HANDLES)
        else {
            iosb(bus, io, STATUS_INSUFFICIENT_RESOURCES, 0)?;
            return Ok(Atendimento {
                eax: STATUS_INSUFFICIENT_RESOURCES,
            });
        };
        // Entradas capturadas antes dos stores; saídas podem sobrepor argumentos.
        bus.escrever_u32(GuestVirt(output), handle)?;
        iosb(bus, io, 0, 1)?;
        self.mutants.reserve_handle();
        self.files
            .handles
            .insert(handle, File { position: 0, share });
        Ok(Atendimento { eax: 0 })
    }
    pub(super) fn nt_read_file(&mut self, bus: &mut Bus, call: &Call) -> Resultado {
        passive(self, bus, call)?;
        let args: Vec<u32> = (0..8)
            .map(|i| call.argument(bus, i))
            .collect::<Result<_, _>>()?;
        let (handle, io, buffer, length, offset) = (args[0], args[4], args[5], args[6], args[7]);
        if self.files.directories.contains(handle) {
            return Err(variant(
                bus,
                call,
                "leitura de diretório exige NtQueryDirectoryFile",
            ));
        }
        if args[1..4] != [0, 0, 0] || length == 0 || length > MAX_READ {
            return Err(variant(
                bus,
                call,
                "leitura síncrona de 1 byte até 1 MiB, sem evento/APC",
            ));
        }
        let Some(file) = self.files.handles.get(&handle).copied() else {
            return Ok(Atendimento {
                eax: STATUS_INVALID_HANDLE,
            });
        };
        if !ram_range(bus, io, 8, Acesso::Escrita)
            || !ram_range(bus, buffer, length, Acesso::Escrita)
            || offset != 0 && !ram_range(bus, offset, 8, Acesso::Leitura)
        {
            return Ok(Atendimento {
                eax: STATUS_ACCESS_VIOLATION,
            });
        }
        if overlap(bus, io, 8, buffer, length) {
            return Err(variant(bus, call, "buffer e IOSB sobrepostos"));
        }
        let position = if offset == 0 {
            file.position
        } else {
            let low = bus.ler_u32(GuestVirt(offset))?;
            let high = bus.ler_u32(GuestVirt(offset + 4))?;
            match (high, low) {
                (u32::MAX, 0xFFFFFFFE) => file.position,
                (0..=0x7FFFFFFF, _) => u64::from(low) | (u64::from(high) << 32),
                _ => return Err(variant(bus, call, "offset negativo não suportado")),
            }
        };
        let bytes = self
            .files
            .image
            .as_deref()
            .expect("handle validado com mídia");
        let available = (bytes.len() as u64).saturating_sub(position);
        let count = u64::from(length).min(available) as usize;
        let result = if count == 0 { EOF } else { 0 };
        if count != 0 {
            bus.escrever_bytes(
                GuestVirt(buffer),
                &bytes[position as usize..position as usize + count],
            )?;
        }
        iosb(bus, io, result, count as u32)?;
        self.files
            .handles
            .get_mut(&handle)
            .expect("handle existente")
            .position = position + count as u64;
        Ok(Atendimento { eax: result })
    }
    pub(super) fn nt_query_information_file(&mut self, bus: &mut Bus, call: &Call) -> Resultado {
        passive(self, bus, call)?;
        let args: Vec<u32> = (0..5)
            .map(|i| call.argument(bus, i))
            .collect::<Result<_, _>>()?;
        if self.files.directories.contains(args[0]) {
            return Err(variant(
                bus,
                call,
                "informações de diretório fora do perfil",
            ));
        }
        let Some(file) = self.files.handles.get(&args[0]) else {
            return Ok(Atendimento {
                eax: STATUS_INVALID_HANDLE,
            });
        };
        let len = self.files.image.as_ref().expect("mídia do handle").len() as u64;
        let mut data = match args[4] {
            5 => vec![0u8; 24],
            14 => vec![0; 8],
            34 => vec![0; 56],
            _ => {
                return Err(variant(
                    bus,
                    call,
                    "classe de informação de arquivo fora do perfil",
                ));
            }
        };
        match args[4] {
            5 => {
                data[..8].copy_from_slice(&len.to_le_bytes());
                data[8..16].copy_from_slice(&len.to_le_bytes());
                data[16..20].copy_from_slice(&1u32.to_le_bytes());
            }
            14 => data.copy_from_slice(&file.position.to_le_bytes()),
            _ => {
                data[32..40].copy_from_slice(&len.to_le_bytes());
                data[40..48].copy_from_slice(&len.to_le_bytes());
                data[48..52].copy_from_slice(&1u32.to_le_bytes());
            }
        }
        let (io, output, length) = (args[1], args[2], args[3]);
        if !ram_range(bus, io, 8, Acesso::Escrita) {
            return Ok(Atendimento {
                eax: STATUS_ACCESS_VIOLATION,
            });
        }
        if length < data.len() as u32 {
            iosb(bus, io, TOO_SMALL, 0)?;
            return Ok(Atendimento { eax: TOO_SMALL });
        }
        if !ram_range(bus, output, data.len() as u32, Acesso::Escrita) {
            return Ok(Atendimento {
                eax: STATUS_ACCESS_VIOLATION,
            });
        }
        if overlap(bus, io, 8, output, data.len() as u32) {
            return Err(variant(bus, call, "informação e IOSB sobrepostos"));
        }
        bus.escrever_bytes(GuestVirt(output), &data)?;
        iosb(bus, io, 0, data.len() as u32)?;
        Ok(Atendimento { eax: 0 })
    }
}
