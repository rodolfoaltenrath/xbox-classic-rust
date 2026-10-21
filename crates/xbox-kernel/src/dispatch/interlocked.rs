//! Troca de LONG guest sob posse exclusiva do Bus, sem yield (ADR-0040).
//! ABI Xbox FASTCALL/8: Target em ECX, Value em EDX. Retorna o LONG anterior.
//! Assinatura: nxdk xboxkrnl.h; semântica: documentação pública `InterlockedExchange`.

use xbox_mem::{Acesso, Bus, GuestVirt};

use super::{Atendimento, Erro, Resultado, ram_range};
use crate::imports::Call;

pub(super) fn exchange(bus: &mut Bus, call: &Call) -> Resultado {
    // Capturar argumentos antes do efeito: Target pode apontar à pilha do guest.
    let target = call.argument(bus, 0)?;
    let value = call.argument(bus, 1)?;
    if !ram_range(bus, target, 4, Acesso::Escrita) || !ram_range(bus, target, 4, Acesso::Leitura) {
        return Err(Erro::ArgumentoInvalido {
            nome: call.export.name,
            ordinal: call.export.ordinal,
            detalhe: "Target deve ocupar quatro bytes de RAM guest legível e gravável".to_owned(),
        });
    }
    // RAM é síncrona/exclusiva; não há observador concorrente, leitura pendente
    // ou yield entre load/store. MMIO foi recusado ANTES de qualquer acesso.
    // Filas/DMA concorrente futuros exigem ampliar o protocolo, não apenas fence host.
    bus.ordenar_escritas();
    let previous = bus.ler_u32(GuestVirt(target))?;
    bus.escrever_u32(GuestVirt(target), value)?;
    bus.ordenar_escritas();
    Ok(Atendimento { eax: previous })
}
