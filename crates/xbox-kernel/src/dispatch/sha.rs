//! SHA-1 sobre contexto opaco de 116 bytes do perfil HLE, sem estado host oculto.
//!
//! O nxdk publica tamanho/assinaturas, mas não o layout interno. Este formato
//! versionado é nosso (ADR-0033), não uma representação byte a byte de um kernel
//! físico. A compressão segue as equações SHA-1 descritas no RFC 3174 §6.1.

use xbox_mem::{Acesso, Bus, GuestVirt};

use super::{Atendimento, Erro, Resultado};
use crate::imports::Call;

pub const CONTEXT_SIZE: usize = 116;
const MAX_BYTES: u64 = u64::MAX / 8;
const INITIAL_HASH: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];

#[derive(Clone)]
struct Context {
    hash: [u32; 5],
    bytes: u64,
    buffer: [u8; 64],
}

impl Context {
    const fn new() -> Self {
        Self {
            hash: INITIAL_HASH,
            bytes: 0,
            buffer: [0; 64],
        }
    }

    fn decode(data: &[u8; CONTEXT_SIZE]) -> Result<Self, &'static str> {
        if &data[..4] != b"XSH1" || data[4..8] != 1u32.to_le_bytes() {
            return Err("contexto SHA não inicializado ou perfil desconhecido");
        }
        let mut context = Self::new();
        for (i, word) in context.hash.iter_mut().enumerate() {
            let start = 8 + i * 4;
            *word = u32::from_le_bytes([
                data[start],
                data[start + 1],
                data[start + 2],
                data[start + 3],
            ]);
        }
        let mut count = [0; 8];
        count.copy_from_slice(&data[28..36]);
        context.bytes = u64::from_le_bytes(count);
        context.buffer.copy_from_slice(&data[36..100]);
        let used = (context.bytes % 64) as usize;
        if context.bytes > MAX_BYTES
            || data[100..].iter().any(|b| *b != 0)
            || context.buffer[used..].iter().any(|b| *b != 0)
        {
            return Err("contador, reserva ou buffer SHA inválido");
        }
        Ok(context)
    }

    fn encode(&self) -> [u8; CONTEXT_SIZE] {
        let mut data = [0; CONTEXT_SIZE];
        data[..4].copy_from_slice(b"XSH1");
        data[4..8].copy_from_slice(&1u32.to_le_bytes());
        for (i, word) in self.hash.iter().enumerate() {
            data[8 + i * 4..12 + i * 4].copy_from_slice(&word.to_le_bytes());
        }
        data[28..36].copy_from_slice(&self.bytes.to_le_bytes());
        data[36..100].copy_from_slice(&self.buffer);
        data
    }

    /// O chamador valida o comprimento total antes de processar qualquer entrada.
    fn update(&mut self, mut data: &[u8]) {
        while !data.is_empty() {
            let used = (self.bytes % 64) as usize;
            let take = data.len().min(64 - used);
            self.buffer[used..used + take].copy_from_slice(&data[..take]);
            self.bytes += take as u64;
            data = &data[take..];
            if self.bytes.is_multiple_of(64) {
                compress(&mut self.hash, &self.buffer);
                self.buffer.fill(0);
            }
        }
    }

    /// Política HLE para o contexto `IN` opaco: finalizar uma cópia. Não é prova
    /// de que o kernel físico preserve cada byte interno após a finalização.
    fn digest(mut self) -> [u8; 20] {
        let used = (self.bytes % 64) as usize;
        self.buffer[used] = 0x80;
        if used >= 56 {
            compress(&mut self.hash, &self.buffer);
            self.buffer.fill(0);
        }
        self.buffer[56..].copy_from_slice(&(self.bytes * 8).to_be_bytes());
        compress(&mut self.hash, &self.buffer);
        let mut digest = [0; 20];
        for (i, word) in self.hash.iter().enumerate() {
            digest[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        digest
    }
}

#[allow(
    clippy::many_single_char_names,
    reason = "nomes A/B/C/D/E/F/K seguem as equações SHA-1 do RFC 3174"
)]
fn compress(hash: &mut [u32; 5], block: &[u8; 64]) {
    let mut words = [0u32; 80];
    for (word, bytes) in words[..16].iter_mut().zip(block.as_chunks::<4>().0) {
        *word = u32::from_be_bytes(*bytes);
    }
    for i in 16..80 {
        words[i] = (words[i - 3] ^ words[i - 8] ^ words[i - 14] ^ words[i - 16]).rotate_left(1);
    }
    let [mut a, mut b, mut c, mut d, mut e] = *hash;
    for (i, word) in words.into_iter().enumerate() {
        let (f, k) = match i {
            0..=19 => ((b & c) | (!b & d), 0x5A827999u32),
            20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
            40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
            _ => (b ^ c ^ d, 0xCA62C1D6),
        };
        let next = a
            .rotate_left(5)
            .wrapping_add(f)
            .wrapping_add(e)
            .wrapping_add(k)
            .wrapping_add(word);
        e = d;
        d = c;
        c = b.rotate_left(30);
        b = a;
        a = next;
    }
    for (word, value) in hash.iter_mut().zip([a, b, c, d, e]) {
        *word = word.wrapping_add(value);
    }
}

fn invalid(call: &Call, detail: impl Into<String>) -> Erro {
    Erro::ArgumentoInvalido {
        nome: call.export.name,
        ordinal: call.export.ordinal,
        detalhe: detail.into(),
    }
}

/// Preflight por página, sem acessar MMIO. Não percorre cada byte de um buffer grande.
fn validate_ram(
    bus: &Bus,
    address: u32,
    size: u32,
    access: Acesso,
    call: &Call,
) -> Result<(), Erro> {
    if size == 0 {
        return Ok(());
    }
    if address == 0 || address.checked_add(size - 1).is_none() {
        return Err(invalid(
            call,
            "buffer nulo ou faixa SHA transborda o espaço guest",
        ));
    }
    let mut offset = 0;
    while offset < size {
        let va = address + offset;
        let take = (4096 - (va & 4095)).min(size - offset);
        let pa = bus.traduzir(GuestVirt(va), access)?;
        if u64::from(pa.0) + u64::from(take) > bus.ram() as u64 {
            return Err(invalid(call, "buffers SHA exigem RAM guest, não MMIO"));
        }
        offset += take;
    }
    Ok(())
}

fn read_context(bus: &Bus, address: u32, call: &Call) -> Result<Context, Erro> {
    validate_ram(bus, address, CONTEXT_SIZE as u32, Acesso::Leitura, call)?;
    let mut data = [0; CONTEXT_SIZE];
    bus.ler_bytes(GuestVirt(address), &mut data)?;
    Context::decode(&data).map_err(|detail| invalid(call, detail))
}

pub(super) fn init(bus: &mut Bus, call: &Call) -> Resultado {
    let address = call.argument(bus, 0)?;
    validate_ram(bus, address, CONTEXT_SIZE as u32, Acesso::Escrita, call)?;
    bus.escrever_bytes(GuestVirt(address), &Context::new().encode())?;
    Ok(Atendimento { eax: 0 }) // VOID; convenção local do dispatcher.
}

pub(super) fn update(bus: &mut Bus, call: &Call) -> Resultado {
    let address = call.argument(bus, 0)?;
    let input = call.argument(bus, 1)?;
    let size = call.argument(bus, 2)?;
    validate_ram(bus, address, CONTEXT_SIZE as u32, Acesso::Escrita, call)?;
    let mut context = read_context(bus, address, call)?;
    if context
        .bytes
        .checked_add(u64::from(size))
        .is_none_or(|n| n > MAX_BYTES)
    {
        return Err(invalid(
            call,
            "comprimento SHA excede o limite de 64 bits em bits",
        ));
    }
    validate_ram(bus, input, size, Acesso::Leitura, call)?;
    // Hash em cópia local: falta tardia não publica estado parcial. Entrada pode
    // sobrepor o contexto, que só é escrito após consumir todos os bytes originais.
    let mut buffer = [0; 4096];
    let mut offset = 0;
    while offset < size {
        let take = (size - offset).min(buffer.len() as u32);
        bus.ler_bytes(GuestVirt(input + offset), &mut buffer[..take as usize])?;
        context.update(&buffer[..take as usize]);
        offset += take;
    }
    bus.escrever_bytes(GuestVirt(address), &context.encode())?;
    Ok(Atendimento { eax: 0 })
}

pub(super) fn finish(bus: &mut Bus, call: &Call) -> Resultado {
    let address = call.argument(bus, 0)?;
    let output = call.argument(bus, 1)?;
    let context = read_context(bus, address, call)?;
    validate_ram(bus, output, 20, Acesso::Escrita, call)?;
    bus.escrever_bytes(GuestVirt(output), &context.digest())?;
    Ok(Atendimento { eax: 0 })
}
