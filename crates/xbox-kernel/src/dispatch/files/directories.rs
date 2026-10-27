//! Diretório da mídia `ImageFile` declarada; ABI Xbox, ADR-0092.
use super::{
    Acesso, Atendimento, Bus, Call, Dispatcher, Erro, Files, GuestVirt, MAX_FILE_HANDLES, MAX_READ,
    Resultado, SHARING, STATUS_ACCESS_VIOLATION, STATUS_INSUFFICIENT_RESOURCES,
    STATUS_INVALID_HANDLE, STATUS_OBJECT_NAME_NOT_FOUND, Snapshot, SnapshotError, SnapshotReader,
    SnapshotWriter, bad, iosb, name, overlap, passive, ram_range, variant,
};
use std::collections::BTreeMap;

const NAME: &[u8] = b"default.xbe";
const ROOT: &[u8] = b"\\Device\\CdRom0\\";
const NO_MORE: u32 = 0x80000006;
const NO_SUCH: u32 = 0xC000000F;

#[derive(Debug, Default)]
pub(super) struct Directories {
    enabled: bool,
    handles: BTreeMap<u32, Directory>,
}
#[derive(Debug, Clone)]
struct Directory {
    share: u32,
    pattern: Option<Vec<u8>>,
    exhausted: bool,
}
impl Directories {
    pub(super) fn enable(&mut self) {
        self.enabled = true;
    }
    pub(super) fn len(&self) -> usize {
        self.handles.len()
    }
    pub(super) fn contains(&self, handle: u32) -> bool {
        self.handles.contains_key(&handle)
    }
    pub(super) fn close(&mut self, handle: u32) -> bool {
        self.handles.remove(&handle).is_some()
    }
    pub(super) fn validate(&self, files: &Files, d: &Dispatcher) -> Result<(), SnapshotError> {
        if (self.enabled && files.image.is_none())
            || (!self.enabled && !self.handles.is_empty())
            || files.handles.len() + self.handles.len() > MAX_FILE_HANDLES
            || self.handles.len() > 1 && self.handles.values().any(|v| v.share & 1 == 0)
        {
            return Err(bad(
                "diretórios incompatíveis com mídia, perfil ou compartilhamento",
            ));
        }
        for &handle in self.handles.keys() {
            if handle < 4
                || !handle.is_multiple_of(4)
                || handle >= d.mutants.next_handle()
                || files.handles.contains_key(&handle)
                || d.mutants.get(handle).is_some()
                || d.events.get(handle).is_some()
                || d.links.contains(handle)
                || d.threads.child.is_some_and(|v| v.handle == handle)
            {
                return Err(bad("handle de diretório inválido ou colidente"));
            }
        }
        Ok(())
    }
}
fn boolean(inp: &mut SnapshotReader<'_>) -> Result<bool, SnapshotError> {
    match inp.u8()? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(bad("booleano de diretório inválido")),
    }
}
impl Snapshot for Directories {
    fn save(&self, out: &mut SnapshotWriter) {
        out.section("DIRE", 1);
        out.bool(self.enabled);
        out.u32(self.handles.len() as u32);
        for (&handle, dir) in &self.handles {
            out.u32(handle);
            out.u32(dir.share);
            out.bool(dir.exhausted);
            out.bool(dir.pattern.is_some());
            if let Some(pattern) = &dir.pattern {
                out.u16(pattern.len() as u16);
                out.raw(pattern);
            }
        }
    }
    fn load(&mut self, inp: &mut SnapshotReader<'_>) -> Result<(), SnapshotError> {
        if inp.section("DIRE", 1)? != 1 {
            return Err(bad("versão de diretório desconhecida"));
        }
        let enabled = boolean(inp)?;
        let count = inp.u32()? as usize;
        if count > MAX_FILE_HANDLES || !enabled && count != 0 {
            return Err(bad("contagem de diretórios inválida"));
        }
        let mut handles = BTreeMap::new();
        let mut previous = 0;
        for _ in 0..count {
            let handle = inp.u32()?;
            let share = inp.u32()?;
            let exhausted = boolean(inp)?;
            let pattern = if boolean(inp)? {
                let len = usize::from(inp.u16()?);
                if !(1..=260).contains(&len) {
                    return Err(bad("tamanho do filtro inválido"));
                }
                let bytes = inp.raw(len)?;
                if !valid_pattern(bytes) {
                    return Err(bad("filtro de diretório inválido"));
                }
                Some(bytes.to_vec())
            } else {
                None
            };
            if handle <= previous || share > 7 || exhausted && pattern.is_none() {
                return Err(bad("registro de diretório inválido"));
            }
            handles.insert(
                handle,
                Directory {
                    share,
                    pattern,
                    exhausted,
                },
            );
            previous = handle;
        }
        *self = Self { enabled, handles };
        Ok(())
    }
}
fn valid_pattern(bytes: &[u8]) -> bool {
    !bytes.is_empty()
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || b"._-*?".contains(b))
}
// Glob ASCII de tamanho limitado; sem inferir os tokens DOS <, > ou ".
fn matches(pattern: &[u8], name: &[u8]) -> bool {
    let mut row = vec![false; name.len() + 1];
    row[0] = true;
    for &token in pattern {
        let mut next = vec![false; name.len() + 1];
        if token == b'*' {
            next[0] = row[0];
        }
        for n in 1..=name.len() {
            next[n] = if token == b'*' {
                row[n] || next[n - 1]
            } else {
                row[n - 1] && (token == b'?' || token.eq_ignore_ascii_case(&name[n - 1]))
            };
        }
        row = next;
    }
    row[name.len()]
}
fn filter(bus: &Bus, call: &Call, at: u32) -> Result<Option<Vec<u8>>, Erro> {
    if at == 0 {
        return Ok(Some(b"*".to_vec()));
    }
    if !ram_range(bus, at, 8, Acesso::Leitura) {
        return Ok(None);
    }
    // ANSI_STRING vazio significa nenhum filtro (observado no wrapper nxdk).
    if bus.ler_u16(GuestVirt(at))? == 0 {
        return Ok(Some(b"*".to_vec()));
    }
    let Some(pattern) = name(bus, at) else {
        return Ok(None);
    };
    if !valid_pattern(&pattern) {
        return Err(variant(bus, call, "filtro fora de glob ASCII (* e ?)"));
    }
    Ok(Some(pattern))
}
impl Dispatcher {
    pub(in crate::dispatch) fn nt_open_file(&mut self, bus: &mut Bus, call: &Call) -> Resultado {
        passive(self, bus, call)?;
        let args: Vec<_> = (0..6)
            .map(|i| call.argument(bus, i))
            .collect::<Result<_, _>>()?;
        if !self.files.directories.enabled {
            return Err(variant(
                bus,
                call,
                "perfil legado ou sessão sem diretório de mídia",
            ));
        }
        if args[1] != 0x100001 || args[4] > 7 || args[5] != 0x21 {
            return Err(variant(
                bus,
                call,
                "somente FILE_LIST_DIRECTORY/SYNCHRONIZE, DIRECTORY_FILE e SYNCHRONOUS_IO_NONALERT",
            ));
        }
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
        let path = match self.file_name(bus, call, attributes)? {
            Ok(path) => path,
            Err(status) => {
                if status == STATUS_OBJECT_NAME_NOT_FOUND {
                    iosb(bus, io, status, 0)?;
                }
                return Ok(Atendimento { eax: status });
            }
        };
        let insensitive = bus.ler_u32(GuestVirt(attributes + 8))? == 0x40;
        let root = &path[..];
        let equal = |a: &[u8], b: &[u8]| {
            if insensitive {
                a.eq_ignore_ascii_case(b)
            } else {
                a == b
            }
        };
        if !equal(root, ROOT) && !equal(root, &ROOT[..ROOT.len() - 1]) {
            iosb(bus, io, STATUS_OBJECT_NAME_NOT_FOUND, 0)?;
            return Ok(Atendimento {
                eax: STATUS_OBJECT_NAME_NOT_FOUND,
            });
        }
        let dirs = &self.files.directories;
        if dirs
            .handles
            .values()
            .any(|dir| dir.share & 1 == 0 || args[4] & 1 == 0)
        {
            iosb(bus, io, SHARING, 0)?;
            return Ok(Atendimento { eax: SHARING });
        }
        let Some((handle, _)) = self
            .mutants
            .prepare(false)
            .filter(|_| self.files.handles.len() + dirs.len() < MAX_FILE_HANDLES)
        else {
            iosb(bus, io, STATUS_INSUFFICIENT_RESOURCES, 0)?;
            return Ok(Atendimento {
                eax: STATUS_INSUFFICIENT_RESOURCES,
            });
        };
        bus.escrever_u32(GuestVirt(output), handle)?;
        iosb(bus, io, 0, 1)?;
        self.mutants.reserve_handle();
        self.files.directories.handles.insert(
            handle,
            Directory {
                share: args[4],
                pattern: None,
                exhausted: false,
            },
        );
        Ok(Atendimento { eax: 0 })
    }
    pub(in crate::dispatch) fn nt_query_directory_file(
        &mut self,
        bus: &mut Bus,
        call: &Call,
    ) -> Resultado {
        passive(self, bus, call)?;
        let a: Vec<_> = (0..10)
            .map(|i| call.argument(bus, i))
            .collect::<Result<_, _>>()?;
        if a[1..4] != [0, 0, 0] || a[7] != 1 || a[9] > 1 || !(75..=MAX_READ).contains(&a[6]) {
            return Err(variant(
                bus,
                call,
                "somente enumeração síncrona, classe 1, booleano canônico e buffer completo",
            ));
        }
        let Some(dir) = self.files.directories.handles.get(&a[0]).cloned() else {
            return Ok(Atendimento {
                eax: STATUS_INVALID_HANDLE,
            });
        };
        let (io, output, length) = (a[4], a[5], a[6]);
        if !ram_range(bus, io, 8, Acesso::Escrita)
            || !ram_range(bus, output, length, Acesso::Escrita)
        {
            return Ok(Atendimento {
                eax: STATUS_ACCESS_VIOLATION,
            });
        }
        if overlap(bus, io, 8, output, length) {
            return Err(variant(bus, call, "buffer de diretório e IOSB sobrepostos"));
        }
        let first = dir.pattern.is_none();
        let pattern = match &dir.pattern {
            Some(pattern) => pattern.clone(),
            None => match filter(bus, call, a[8])? {
                Some(pattern) => pattern,
                None => {
                    return Ok(Atendimento {
                        eax: STATUS_ACCESS_VIOLATION,
                    });
                }
            },
        };
        let exhausted = dir.exhausted && a[9] == 0;
        let found = !exhausted && matches(&pattern, NAME);
        let status = if found {
            0
        } else if first {
            NO_SUCH
        } else {
            NO_MORE
        };
        if found {
            let mut bytes = vec![0u8; 64 + NAME.len()];
            let len = self
                .files
                .image
                .as_ref()
                .expect("diretório com mídia")
                .len() as u64;
            bytes[40..48].copy_from_slice(&len.to_le_bytes());
            bytes[48..56].copy_from_slice(&len.to_le_bytes());
            bytes[56..60].copy_from_slice(&1u32.to_le_bytes());
            bytes[60..64].copy_from_slice(&(NAME.len() as u32).to_le_bytes());
            bytes[64..].copy_from_slice(NAME);
            bus.escrever_bytes(GuestVirt(output), &bytes)?;
        }
        iosb(bus, io, status, if found { 75 } else { 0 })?;
        let dir = self
            .files
            .directories
            .handles
            .get_mut(&a[0])
            .expect("handle validado");
        dir.pattern = Some(pattern);
        dir.exhausted = true;
        Ok(Atendimento { eax: status })
    }
}
