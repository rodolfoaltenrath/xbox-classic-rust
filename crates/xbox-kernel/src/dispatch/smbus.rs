//! `HalWriteSMBusValue` para o LED frontal do SMC (ADR-0090).
//!
//! Recorte guiado pelo consumidor real (`lib/hal/led.c` do nxdk fixado, usado pelo
//! sample `led`): escrita de **byte** no SMC (endereço de escrita 0x20) com o comando
//! 0x07 (modo do LED: 0 automático, 1 manual) ou 0x08 (sequência de quatro tempos:
//! vermelho nos bits 7–4, verde nos bits 3–0, o primeiro tempo no bit mais alto de
//! cada metade; `XLED_GREEN = 0x01`, `XLED_RED = 0x10`). O estado fica observável e vai no
//! snapshot; não há LED físico, temporização da sequência nem outros dispositivos do
//! barramento (encoder de vídeo, sensor de temperatura). Qualquer outro endereço,
//! comando ou escrita de palavra é recusado com a variante e os argumentos.

use xbox_core::error::SnapshotError;
use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_mem::Bus;

use super::{Atendimento, Erro, Resultado, descrever_argumentos};
use crate::imports::Call;

/// Endereço de escrita do SMC no barramento de gerenciamento.
pub(super) const SMC_ESCRITA: u8 = 0x20;
/// Registrador de modo do LED.
pub(super) const SMC_LED_MODO: u8 = 0x07;
/// Registrador de sequência do LED.
pub(super) const SMC_LED_SEQUENCIA: u8 = 0x08;

/// Estado do LED que o guest programou. `None` = nunca escrito nesta sessão (o
/// firmware mantém o padrão automático, que não é modelado).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Smbus {
    modo: Option<u8>,
    sequencia: Option<u8>,
    escritas: u64,
}

impl Smbus {
    /// Último modo escrito (0 automático, 1 manual).
    pub const fn modo(&self) -> Option<u8> {
        self.modo
    }

    /// Última sequência escrita.
    pub const fn sequencia(&self) -> Option<u8> {
        self.sequencia
    }

    /// Escritas aceitas no SMC.
    pub const fn escritas(&self) -> u64 {
        self.escritas
    }

    /// `NTSTATUS HalWriteSMBusValue(UCHAR SlaveAddress, UCHAR CommandCode,
    /// BOOLEAN WriteWordValue, ULONG DataValue)`.
    pub(super) fn escrever(&mut self, bus: &Bus, call: &Call) -> Resultado {
        let endereco = call.argument(bus, 0)? as u8;
        let comando = call.argument(bus, 1)? as u8;
        let palavra = call.argument(bus, 2)? as u8 != 0;
        let valor = call.argument(bus, 3)?;
        let variante = |detalhe| {
            Err(Erro::VarianteNaoImplementada {
                nome: call.export.name,
                ordinal: call.export.ordinal,
                chamador: call.return_pc,
                detalhe,
                argumentos: descrever_argumentos(bus, call),
            })
        };
        if endereco != SMC_ESCRITA {
            return variante("dispositivo SMBus diferente do SMC (0x20) sem modelo");
        }
        if palavra {
            return variante("escrita de palavra no SMC sem modelo");
        }
        let byte = u8::try_from(valor).ok();
        match (comando, byte) {
            (SMC_LED_MODO, Some(modo @ (0 | 1))) => self.modo = Some(modo),
            (SMC_LED_SEQUENCIA, Some(sequencia)) => self.sequencia = Some(sequencia),
            (SMC_LED_MODO | SMC_LED_SEQUENCIA, _) => {
                return variante("valor do LED fora do byte/modos 0 e 1");
            }
            _ => return variante("comando do SMC sem modelo (só LED 0x07/0x08)"),
        }
        self.escritas = self.escritas.saturating_add(1);
        Ok(Atendimento { eax: 0 }) // STATUS_SUCCESS
    }

    /// Descrição da sequência: quatro tempos, cada um verde/vermelho/laranja/apagado.
    pub fn descrever_sequencia(sequencia: u8) -> String {
        (0..4)
            .map(|tempo| {
                let bit = 3 - tempo;
                // (vermelho, verde)
                match (sequencia >> (4 + bit) & 1, sequencia >> bit & 1) {
                    (1, 1) => "laranja",
                    (0, 1) => "verde",
                    (1, 0) => "vermelho",
                    _ => "apagado",
                }
            })
            .collect::<Vec<_>>()
            .join(",")
    }
}

impl Snapshot for Smbus {
    fn save(&self, out: &mut SnapshotWriter) {
        out.section("SMBC", 1);
        for campo in [self.modo, self.sequencia] {
            out.bool(campo.is_some());
            out.u8(campo.unwrap_or(0));
        }
        out.u64(self.escritas);
    }

    fn load(&mut self, inp: &mut SnapshotReader<'_>) -> Result<(), SnapshotError> {
        let bad = |what| SnapshotError::BadValue {
            section: "SMBC",
            what,
        };
        if inp.section("SMBC", 1)? != 1 {
            return Err(bad("versão SMBus desconhecida"));
        }
        let mut ler = || -> Result<Option<u8>, SnapshotError> {
            let presente = inp.bool()?;
            let valor = inp.u8()?;
            if !presente && valor != 0 {
                return Err(bad("valor de LED sem marca de presença"));
            }
            Ok(presente.then_some(valor))
        };
        let modo = ler()?;
        let sequencia = ler()?;
        if modo.is_some_and(|m| m > 1) {
            return Err(bad("modo do LED fora de 0/1"));
        }
        let escritas = inp.u64()?;
        // Cada escrita aceita define modo ou sequência: os dois lados são equivalentes.
        if (escritas == 0) != (modo.is_none() && sequencia.is_none()) {
            return Err(bad("contador de escritas incoerente com o estado do LED"));
        }
        *self = Self {
            modo,
            sequencia,
            escritas,
        };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Smbus;

    #[test]
    fn sequencia_do_sample_led_e_verde_vermelho_apagado_apagado() {
        // XSetCustomLED(GREEN = 0x01, RED = 0x10, OFF, OFF), como em `led.c`:
        // ((t1 & 0x11) << 3) | ((t2 & 0x11) << 2) | ((t3 & 0x11) << 1) | (t4 & 0x11).
        let (verde, vermelho) = (0x01u8, 0x10u8);
        let sequencia = ((verde & 0x11) << 3) | ((vermelho & 0x11) << 2);
        assert_eq!(sequencia, 0x48);
        assert_eq!(
            Smbus::descrever_sequencia(sequencia),
            "verde,vermelho,apagado,apagado"
        );
        assert_eq!(
            Smbus::descrever_sequencia(0xFF),
            "laranja,laranja,laranja,laranja"
        );
        assert_eq!(
            Smbus::descrever_sequencia(0),
            "apagado,apagado,apagado,apagado"
        );
    }
}
