//! Dispositivos de MMIO e o registro de acessos desconhecidos.
//!
//! Na Fase 2 **nenhum dispositivo é emulado**. Esta camada existe para duas coisas:
//!
//! 1. dar ao bus um destino bem definido para acesso a faixa de dispositivo, em vez de
//!    tratar como erro — porque hardware real responde a isso, e tratar como falha de
//!    página faria o guest tomar `#PF` onde o console não toma;
//! 2. **registrar** cada acesso com endereço, operação, tamanho e contexto, para que a
//!    Fase 5 (NV2A) e a Fase 6 (APU) comecem com a lista exata de registradores que os
//!    jogos tocam, em vez de uma leitura especulativa de documentação.
//!
//! ## Nada de comportamento inventado
//!
//! Leitura de registrador não implementado devolve **zero**, e isso é um *placeholder
//! declarado*, não comportamento observado em hardware. Escrita é descartada. As duas
//! coisas são contadas. Quando a Fase 5 implementar o NV2A de verdade, o valor passa a
//! vir do modelo do dispositivo; até lá, o zero não deve ser confundido com emulação —
//! e é por isso que [`Telemetria`] distingue "não implementado" de "desconhecido".

use std::collections::BTreeMap;
use std::sync::Mutex;

use crate::addr::{GuestPhys, GuestVirt};

/// Limite de registros distintos guardados pela telemetria.
///
/// Evita que um jogo varrendo o espaço de MMIO consuma memória sem limite. Registros
/// além do limite são contados em [`Telemetria::descartados`].
pub const MAX_REGISTROS: usize = 4096;

/// Dispositivos que o mapa físico reconhece.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Dispositivo {
    /// GPU NV2A.
    Nv2a,
    /// Unidade de áudio (APU).
    Apu,
    /// Codec AC97.
    Ac97,
    /// Controlador USB OHCI 0.
    UsbOhci0,
    /// Controlador USB OHCI 1.
    UsbOhci1,
    /// Interface de rede nForce.
    Nic,
}

impl Dispositivo {
    /// Nome curto, para log e relatório.
    pub const fn nome(self) -> &'static str {
        match self {
            Self::Nv2a => "nv2a",
            Self::Apu => "apu",
            Self::Ac97 => "ac97",
            Self::UsbOhci0 => "usb0",
            Self::UsbOhci1 => "usb1",
            Self::Nic => "nic",
        }
    }

    /// Fase do roadmap que implementa este dispositivo.
    pub const fn fase(self) -> &'static str {
        match self {
            Self::Nv2a => "Fase 5",
            Self::Apu | Self::Ac97 => "Fase 6",
            Self::UsbOhci0 | Self::UsbOhci1 => "Fase 7",
            Self::Nic => "Fase 11",
        }
    }
}

/// Tipo de operação em um acesso registrado.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Operacao {
    /// Leitura.
    Leitura,
    /// Escrita.
    Escrita,
}

impl Operacao {
    /// Nome curto.
    pub const fn nome(self) -> &'static str {
        match self {
            Self::Leitura => "leitura",
            Self::Escrita => "escrita",
        }
    }
}

/// Por que o acesso foi registrado.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Motivo {
    /// Faixa de um dispositivo que conhecemos, mas que ainda não é emulado.
    NaoImplementado(Dispositivo),
    /// Endereço físico que não cai em nenhuma região conhecida.
    Desconhecido,
    /// Janela *tiled*, reconhecida e deliberadamente não implementada.
    Tiled,
    /// Flash ROM, que não emulamos por decisão (ADR-0001).
    Flash,
}

impl Motivo {
    /// Descrição legível.
    pub fn descricao(self) -> String {
        match self {
            Self::NaoImplementado(d) => {
                format!("{} não implementado (chega na {})", d.nome(), d.fase())
            }
            Self::Desconhecido => "endereço físico fora do mapa conhecido".to_owned(),
            Self::Tiled => "janela tiled, não implementada (Fase 5)".to_owned(),
            Self::Flash => "flash ROM, não emulada (ADR-0001)".to_owned(),
        }
    }
}

/// Contexto do guest no momento do acesso.
///
/// Na Fase 2 não existe CPU, então vem vazio. O campo existe agora para que a Fase 3
/// preencha sem mudar assinatura nenhuma — e para que o relatório de MMIO já nasça com
/// lugar para a informação que torna o registro acionável: *quem* tocou o registrador.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Contexto {
    /// Endereço da instrução que fez o acesso, quando conhecido.
    pub pc: Option<GuestVirt>,
    /// Identificador da thread do guest, quando houver escalonador (Fase 4).
    pub thread: Option<u32>,
}

/// Chave de agregação: um registro por (endereço, operação, tamanho).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Chave {
    endereco: u32,
    operacao: Operacao,
    tamanho: u8,
}

/// Um acesso registrado, agregado por quantidade de ocorrências.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registro {
    /// Endereço físico acessado.
    pub endereco: GuestPhys,
    /// Leitura ou escrita.
    pub operacao: Operacao,
    /// Tamanho do acesso, em bytes.
    pub tamanho: u8,
    /// Por que foi registrado.
    pub motivo: Motivo,
    /// Quantas vezes aconteceu.
    pub ocorrencias: u64,
    /// Valor da primeira escrita observada.
    pub primeiro_valor: Option<u32>,
    /// Contexto do guest na primeira ocorrência.
    pub primeiro_contexto: Contexto,
}

/// Registro de acessos a MMIO não emulado.
///
/// Usa `Mutex` para que o bus possa registrar durante uma **leitura** (`&self`). O
/// custo não importa: estes caminhos são, por definição, os não otimizados — e quando
/// um deles passar a ser quente, o certo é implementar o dispositivo, não acelerar a
/// telemetria.
#[derive(Debug, Default)]
pub struct Telemetria {
    registros: Mutex<BTreeMap<Chave, Registro>>,
    descartados: Mutex<u64>,
}

impl Telemetria {
    /// Telemetria vazia.
    pub fn nova() -> Self {
        Self::default()
    }

    /// Registra um acesso.
    pub fn registrar(
        &self,
        endereco: GuestPhys,
        operacao: Operacao,
        tamanho: u8,
        motivo: Motivo,
        valor: Option<u32>,
        contexto: Contexto,
    ) {
        let chave = Chave {
            endereco: endereco.0,
            operacao,
            tamanho,
        };

        let Ok(mut registros) = self.registros.lock() else {
            // Mutex envenenado por pânico em outra thread. Telemetria é diagnóstico:
            // perder um registro é aceitável, derrubar a emulação não.
            return;
        };

        if let Some(r) = registros.get_mut(&chave) {
            r.ocorrencias += 1;
            return;
        }
        if registros.len() >= MAX_REGISTROS {
            if let Ok(mut d) = self.descartados.lock() {
                *d += 1;
            }
            return;
        }

        tracing::debug!(
            endereco = %endereco,
            operacao = operacao.nome(),
            tamanho,
            motivo = %motivo.descricao(),
            "acesso a MMIO não emulado"
        );
        registros.insert(
            chave,
            Registro {
                endereco,
                operacao,
                tamanho,
                motivo,
                ocorrencias: 1,
                primeiro_valor: valor,
                primeiro_contexto: contexto,
            },
        );
    }

    /// Todos os registros, ordenados por endereço.
    pub fn registros(&self) -> Vec<Registro> {
        self.registros
            .lock()
            .map(|r| r.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Quantos registros distintos foram descartados por exceder [`MAX_REGISTROS`].
    pub fn descartados(&self) -> u64 {
        self.descartados.lock().map(|d| *d).unwrap_or(0)
    }

    /// Soma de todas as ocorrências.
    pub fn total_de_acessos(&self) -> u64 {
        self.registros
            .lock()
            .map(|r| r.values().map(|x| x.ocorrencias).sum())
            .unwrap_or(0)
    }

    /// Relatório em texto, ordenado por quantidade de ocorrências.
    ///
    /// É isto que a Fase 5 vai ler para saber quais registradores do NV2A importam de
    /// verdade, em vez de implementar na ordem da documentação.
    pub fn relatorio(&self) -> String {
        let mut rs = self.registros();
        rs.sort_by(|a, b| {
            b.ocorrencias
                .cmp(&a.ocorrencias)
                .then(a.endereco.cmp(&b.endereco))
        });

        let mut s = String::new();
        s.push_str(&format!(
            "{} registradores distintos, {} acessos no total",
            rs.len(),
            self.total_de_acessos()
        ));
        let d = self.descartados();
        if d > 0 {
            s.push_str(&format!(", {d} registros descartados por limite"));
        }
        s.push('\n');
        for r in rs {
            s.push_str(&format!(
                "  {} {:<8} {}B x{:<8} {}",
                r.endereco,
                r.operacao.nome(),
                r.tamanho,
                r.ocorrencias,
                r.motivo.descricao()
            ));
            if let Some(v) = r.primeiro_valor {
                s.push_str(&format!("  primeiro valor 0x{v:X}"));
            }
            if let Some(pc) = r.primeiro_contexto.pc {
                s.push_str(&format!("  de {pc}"));
            }
            s.push('\n');
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::{Contexto, Dispositivo, MAX_REGISTROS, Motivo, Operacao, Telemetria};
    use crate::addr::{GuestPhys, GuestVirt};

    #[test]
    fn agrega_por_endereco_operacao_e_tamanho() {
        let t = Telemetria::nova();
        let m = Motivo::NaoImplementado(Dispositivo::Nv2a);
        for _ in 0..5 {
            t.registrar(
                GuestPhys(0xFD00_0100),
                Operacao::Leitura,
                4,
                m,
                None,
                Contexto::default(),
            );
        }
        t.registrar(
            GuestPhys(0xFD00_0100),
            Operacao::Escrita,
            4,
            m,
            Some(1),
            Contexto::default(),
        );
        t.registrar(
            GuestPhys(0xFD00_0100),
            Operacao::Leitura,
            2,
            m,
            None,
            Contexto::default(),
        );

        let rs = t.registros();
        assert_eq!(rs.len(), 3, "três chaves distintas");
        assert_eq!(t.total_de_acessos(), 7);
        let leitura4 = rs
            .iter()
            .find(|r| r.operacao == Operacao::Leitura && r.tamanho == 4)
            .unwrap();
        assert_eq!(leitura4.ocorrencias, 5);
    }

    #[test]
    fn guarda_primeiro_valor_e_contexto() {
        let t = Telemetria::nova();
        let ctx = Contexto {
            pc: Some(GuestVirt(0x0021_B1CE)),
            thread: Some(1),
        };
        t.registrar(
            GuestPhys(0xFD00_0200),
            Operacao::Escrita,
            4,
            Motivo::NaoImplementado(Dispositivo::Nv2a),
            Some(0xDEAD),
            ctx,
        );
        // Uma segunda escrita com outro valor não sobrescreve o primeiro.
        t.registrar(
            GuestPhys(0xFD00_0200),
            Operacao::Escrita,
            4,
            Motivo::NaoImplementado(Dispositivo::Nv2a),
            Some(0xBEEF),
            Contexto::default(),
        );

        let r = &t.registros()[0];
        assert_eq!(r.primeiro_valor, Some(0xDEAD));
        assert_eq!(r.primeiro_contexto.pc, Some(GuestVirt(0x0021_B1CE)));
        assert_eq!(r.ocorrencias, 2);
    }

    #[test]
    fn limite_de_registros_e_respeitado() {
        let t = Telemetria::nova();
        for i in 0..(MAX_REGISTROS as u32 + 100) {
            t.registrar(
                GuestPhys(0xFD00_0000 + i * 4),
                Operacao::Leitura,
                4,
                Motivo::NaoImplementado(Dispositivo::Nv2a),
                None,
                Contexto::default(),
            );
        }
        assert_eq!(t.registros().len(), MAX_REGISTROS);
        assert_eq!(t.descartados(), 100);
    }

    #[test]
    fn relatorio_ordena_pelo_que_mais_aparece() {
        let t = Telemetria::nova();
        let m = Motivo::NaoImplementado(Dispositivo::Nv2a);
        t.registrar(
            GuestPhys(0xFD00_0100),
            Operacao::Leitura,
            4,
            m,
            None,
            Contexto::default(),
        );
        for _ in 0..50 {
            t.registrar(
                GuestPhys(0xFD00_0200),
                Operacao::Escrita,
                4,
                m,
                Some(7),
                Contexto::default(),
            );
        }

        let r = t.relatorio();
        let pos_200 = r.find("0xFD000200").expect("endereço quente no relatório");
        let pos_100 = r.find("0xFD000100").expect("endereço frio no relatório");
        assert!(
            pos_200 < pos_100,
            "o mais frequente deveria vir primeiro:\n{r}"
        );
        assert!(
            r.contains("Fase 5"),
            "o relatório deve dizer quem implementa"
        );
    }

    #[test]
    fn motivo_diz_a_fase_responsavel() {
        assert!(
            Motivo::NaoImplementado(Dispositivo::Apu)
                .descricao()
                .contains("Fase 6")
        );
        assert!(Motivo::Tiled.descricao().contains("Fase 5"));
        assert!(Motivo::Flash.descricao().contains("ADR-0001"));
        assert!(Motivo::Desconhecido.descricao().contains("fora do mapa"));
    }
}
