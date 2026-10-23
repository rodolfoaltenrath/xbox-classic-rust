//! O barramento: a única porta de acesso à memória do guest.
//!
//! Três visões sobre uma implementação, como fixado em `docs/arch/memoria.md` §3:
//!
//! | Visão | Entrada | Para quem |
//! |---|---|---|
//! | virtual | [`GuestVirt`], passa pela MMU | CPU (Fase 3) |
//! | física | [`GuestPhys`], já traduzido | kernel HLE (Fase 4) |
//! | DMA | [`GuestPhys`], fatia contígua | GPU (Fase 5) |
//!
//! Ter uma implementação só é o que evita que dois caminhos de acesso divirjam — o tipo
//! de bug que aparece como "funciona no interpretador e não no JIT".
//!
//! ## Semântica de memória do x86
//!
//! Acesso desalinhado é permitido, inclusive atravessando página: o x86 permite, e o
//! código de jogo usa. Por isso todo acesso é implementado sobre um laço que quebra a
//! faixa em pedaços por página, traduz cada pedaço em separado e trata cada um conforme
//! a região física onde ele cai. Um `u32` desalinhado cujas duas metades estão em
//! páginas diferentes — uma mapeada e outra não — falha com o endereço correto em
//! `CR2`, que é o endereço do byte que faltou, não o do início do acesso.
//!
//! ## Endereço inválido nunca vira acesso indevido
//!
//! Toda tradução devolve `Result`, toda classificação de região é explícita e o acesso
//! final é indexação verificada em `&[u8]`. Não existe caminho em que endereço de guest
//! inválido alcance memória do host (ver `docs/arch/memoria-unsafe.md`).

use std::cell::Cell;

use xbox_core::error::SnapshotError;
use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_core::{PAGE_SIZE, RAM_SIZE, Result};

use crate::addr::{Acesso, ArenaOff, GuestPhys, GuestVirt, Perm};
use crate::arena::Arena;
use crate::boot_dma::BootDmaPage;
use crate::dispositivos::{Contexto, Dispositivo, Motivo, Operacao, Telemetria};
use crate::frames::AlocadorDeFrames;
use crate::mmu::{Falta, Mmu, MotivoDeFalta};
use crate::nv2a_irq::Nv2aIrq;
use crate::nv2a_pfb::Nv2aPfb;
use crate::nv2a_pfifo::Nv2aPfifo;
use crate::nv2a_pramdac::Nv2aPramdac;
use crate::nv2a_pramin::Nv2aPramin;
use crate::regioes::{MapaFisico, Regiao};

/// O barramento de memória do console.
#[derive(Debug)]
pub struct Bus {
    arena: Arena,
    mmu: Mmu,
    mapa: MapaFisico,
    frames: AlocadorDeFrames,
    boot_dma: BootDmaPage,
    telemetria: Telemetria,
    nv2a_irq: Nv2aIrq,
    nv2a_pramdac: Nv2aPramdac,
    nv2a_pfb: Nv2aPfb,
    nv2a_pramin: Nv2aPramin,
    nv2a_pfifo: Nv2aPfifo,
    contexto: Cell<Contexto>,
}

impl Bus {
    /// Barramento com a RAM do console (64 MiB, ou 128 com `ram128`).
    pub fn novo() -> Result<Self> {
        Self::com_arena(Arena::nova()?)
    }

    /// Barramento sobre uma arena já criada.
    ///
    /// Útil para teste com RAM pequena, e para quem quiser controlar a criação da arena.
    pub fn com_arena(arena: Arena) -> Result<Self> {
        let tamanho = arena.len();
        Ok(Self {
            arena,
            mmu: Mmu::nova(),
            mapa: MapaFisico::novo(tamanho),
            frames: AlocadorDeFrames::novo(tamanho),
            boot_dma: BootDmaPage::default(),
            telemetria: Telemetria::nova(),
            nv2a_irq: Nv2aIrq::default(),
            nv2a_pramdac: Nv2aPramdac::default(),
            nv2a_pfb: Nv2aPfb::for_ram(tamanho),
            nv2a_pramin: Nv2aPramin::default(),
            nv2a_pfifo: Nv2aPfifo::default(),
            contexto: Cell::new(Contexto::default()),
        })
    }

    /// Quantidade de RAM emulada.
    pub const fn ram(&self) -> usize {
        self.arena.len()
    }

    /// A MMU, para consulta.
    pub const fn mmu(&self) -> &Mmu {
        &self.mmu
    }

    /// A MMU, para mapear e desmapear.
    pub const fn mmu_mut(&mut self) -> &mut Mmu {
        &mut self.mmu
    }

    /// O alocador de frames físicos.
    pub const fn frames(&self) -> &AlocadorDeFrames {
        &self.frames
    }

    /// O alocador de frames físicos, para alocar.
    pub const fn frames_mut(&mut self) -> &mut AlocadorDeFrames {
        &mut self.frames
    }

    /// Posse da página D3D reservada pelo loader; ausência preserva layouts legados.
    pub const fn boot_dma_page(&self) -> BootDmaPage {
        self.boot_dma
    }

    /// Instalar antes de carregar a imagem. Repetir preserva os bytes já escritos.
    pub fn reserve_boot_dma_page(&mut self) -> Result<()> {
        if self.boot_dma.reserved() {
            self.boot_dma
                .validate(self)
                .map_err(xbox_core::Error::format)?;
        } else {
            self.boot_dma = BootDmaPage::install(self)?;
        }
        Ok(())
    }

    /// O registro de acessos a MMIO não emulado.
    pub const fn telemetria(&self) -> &Telemetria {
        &self.telemetria
    }

    /// Os registradores de interrupção de vblank do NV2A (ADR-0055).
    pub const fn nv2a_irq(&self) -> &Nv2aIrq {
        &self.nv2a_irq
    }
    /// Perfil e coeficiente autoritativos; programação ocorre pelos acessos MMIO.
    pub const fn nv2a_pramdac(&self) -> &Nv2aPramdac {
        &self.nv2a_pramdac
    }

    /// Perfil de memória PFB e configuração programada via MMIO (ADR-0058).
    pub const fn nv2a_pfb(&self) -> &Nv2aPfb {
        &self.nv2a_pfb
    }

    /// Memória de instância PRAMIN; armazenamento independente provisório (P14).
    pub const fn nv2a_pramin(&self) -> &Nv2aPramin {
        &self.nv2a_pramin
    }

    /// Controles PFIFO programados; não executa DMA neste perfil.
    pub const fn nv2a_pfifo(&self) -> &Nv2aPfifo {
        &self.nv2a_pfifo
    }

    /// Consome até `words` palavras DMA, sem executar instruções ou avançar Clock.
    pub fn service_gpu(
        &mut self,
        words: u32,
    ) -> Result<crate::nv2a_pfifo::StepReport, crate::nv2a_pfifo::GpuFault> {
        self.nv2a_pfifo
            .run(self.arena.bytes(), &self.nv2a_pramin, words)
    }

    /// Idem, para o relógio de vblank travar a pendência.
    pub const fn nv2a_irq_mut(&mut self) -> &mut Nv2aIrq {
        &mut self.nv2a_irq
    }

    /// O mapa físico.
    pub const fn mapa(&self) -> MapaFisico {
        self.mapa
    }

    /// A arena, para quem precisa de acesso direto à RAM (loader, snapshot).
    pub const fn arena(&self) -> &Arena {
        &self.arena
    }

    /// Informa o contexto do guest, para enriquecer a telemetria.
    ///
    /// A CPU da Fase 3 chama isto ao trocar de instrução; na Fase 2 ninguém chama, e a
    /// telemetria registra contexto vazio.
    pub fn definir_contexto(&self, contexto: Contexto) {
        self.contexto.set(contexto);
    }

    /// O que existe em um endereço físico.
    pub fn classificar(&self, pa: GuestPhys) -> Regiao {
        self.mapa.classificar(pa)
    }

    // ─── visão da CPU: endereço virtual ───────────────────────────────────────────

    /// Traduz um endereço virtual, verificando a permissão.
    pub fn traduzir(&self, va: GuestVirt, acesso: Acesso) -> Result<GuestPhys, Falta> {
        self.mmu.traduzir(va, acesso)
    }

    /// Lê bytes de um endereço virtual.
    pub fn ler_bytes(&self, va: GuestVirt, destino: &mut [u8]) -> Result<(), Falta> {
        self.ler_com_acesso(va, destino, Acesso::Leitura)
    }

    /// Lê bytes como busca de instrução (exige permissão de execução).
    pub fn buscar(&self, va: GuestVirt, destino: &mut [u8]) -> Result<(), Falta> {
        self.ler_com_acesso(va, destino, Acesso::Execucao)
    }

    fn ler_com_acesso(
        &self,
        va: GuestVirt,
        destino: &mut [u8],
        acesso: Acesso,
    ) -> Result<(), Falta> {
        let mut restante: &mut [u8] = destino;
        let mut atual = va;
        while !restante.is_empty() {
            let pa = self.mmu.traduzir(atual, acesso)?;
            // O fatiamento por página não transforma um acesso largo em dois
            // words PRAMIN válidos. Valide antes de alterar o buffer de saída.
            if let Regiao::Dispositivo {
                id: Dispositivo::Nv2a,
                deslocamento,
            } = self.mapa.classificar(pa)
            {
                self.nv2a_pfifo
                    .validate_access(deslocamento, restante.len(), false)
                    .map_err(|o_que| Falta {
                        endereco: atual,
                        acesso,
                        motivo: MotivoDeFalta::NaoImplementado { fisico: pa, o_que },
                    })?;
                self.nv2a_pramin
                    .validate_access(deslocamento, restante.len())
                    .map_err(|o_que| Falta {
                        endereco: atual,
                        acesso,
                        motivo: MotivoDeFalta::NaoImplementado { fisico: pa, o_que },
                    })?;
            }
            let n = restante.len().min(bytes_ate_fim_da_pagina(atual));
            let (agora, depois) = restante.split_at_mut(n);
            self.ler_regiao(pa, agora, acesso).map_err(|motivo| Falta {
                endereco: atual,
                acesso,
                motivo,
            })?;
            restante = depois;
            if restante.is_empty() {
                // Não avança: um acesso que termina exatamente em 0xFFFFFFFF é válido,
                // e somar aqui transbordaria sem motivo.
                break;
            }
            atual = atual.checked_add(n as u32).ok_or(Falta {
                endereco: atual,
                acesso,
                motivo: MotivoDeFalta::Transbordo,
            })?;
        }
        Ok(())
    }

    /// Escreve bytes em um endereço virtual.
    pub fn escrever_bytes(&mut self, va: GuestVirt, origem: &[u8]) -> Result<(), Falta> {
        // Validar todas as páginas antes de tocar RAM ou dispositivos. Um store
        // desalinhado não pode deixar metade do elemento REP escrita numa falta.
        self.validar_escrita(va, origem)?;
        let mut restante = origem;
        let mut atual = va;
        while !restante.is_empty() {
            let pa = self.mmu.traduzir(atual, Acesso::Escrita)?;
            let n = restante.len().min(bytes_ate_fim_da_pagina(atual));
            let (agora, depois) = restante.split_at(n);
            self.escrever_regiao(pa, agora).map_err(|motivo| Falta {
                endereco: atual,
                acesso: Acesso::Escrita,
                motivo,
            })?;
            restante = depois;
            if restante.is_empty() {
                break;
            }
            atual = atual.checked_add(n as u32).ok_or(Falta {
                endereco: atual,
                acesso: Acesso::Escrita,
                motivo: MotivoDeFalta::Transbordo,
            })?;
        }
        Ok(())
    }

    /// Ordena stores anteriores aos posteriores (SFENCE do guest).
    ///
    /// No perfil atual, RAM é escrita sincronicamente e só pode ser observada
    /// por acessos sequenciais ao bus/arena exclusivos. Não há store buffer ou
    /// write-combining para drenar; a ordenação já decorre desse contrato.
    /// Não é uma fence de GPU, nem conclui MMIO: dispositivos não implementados
    /// continuam somente registrando/descartando seus acessos.
    ///
    /// Ao introduzir escritas diferidas, workers ou observadores concorrentes,
    /// este ponto precisa ordenar/publicar os stores antes dos posteriores.
    /// Uma fence atômica do host sozinha não implementaria esse protocolo guest.
    #[allow(
        clippy::unused_self,
        reason = "a barreira exige acesso exclusivo ao bus mesmo no perfil sem filas de stores"
    )]
    pub fn ordenar_escritas(&mut self) {}

    fn validar_escrita(&mut self, va: GuestVirt, origem: &[u8]) -> Result<(), Falta> {
        let mut atual = va;
        let mut restante = origem;
        while !restante.is_empty() {
            let pa = self.mmu.traduzir(atual, Acesso::Escrita)?;
            let n = restante.len().min(bytes_ate_fim_da_pagina(atual));
            if let Regiao::Dispositivo {
                id: Dispositivo::Nv2a,
                deslocamento,
            } = self.mapa.classificar(pa)
            {
                self.nv2a_pfifo
                    .validate_access(deslocamento, restante.len(), true)
                    .map_err(|o_que| Falta {
                        endereco: atual,
                        acesso: Acesso::Escrita,
                        motivo: MotivoDeFalta::NaoImplementado { fisico: pa, o_que },
                    })?;
                self.nv2a_pramin
                    .validate_access(deslocamento, restante.len())
                    .map_err(|o_que| Falta {
                        endereco: atual,
                        acesso: Acesso::Escrita,
                        motivo: MotivoDeFalta::NaoImplementado { fisico: pa, o_que },
                    })?;
                self.nv2a_pfb
                    .validate_access(deslocamento, n)
                    .map_err(|o_que| Falta {
                        endereco: atual,
                        acesso: Acesso::Escrita,
                        motivo: MotivoDeFalta::NaoImplementado { fisico: pa, o_que },
                    })?;
                self.nv2a_pramdac
                    .validate_access(deslocamento, n)
                    .map_err(|o_que| Falta {
                        endereco: atual,
                        acesso: Acesso::Escrita,
                        motivo: MotivoDeFalta::NaoImplementado { fisico: pa, o_que },
                    })?;
            }
            if matches!(
                self.mapa.classificar(pa),
                Regiao::Tiled { .. } | Regiao::Buraco
            ) {
                // Estas regiões só registram o diagnóstico e devolvem erro;
                // nenhuma escrita de RAM/MMIO é executada nesta passagem.
                self.escrever_regiao(pa, &restante[..n])
                    .map_err(|motivo| Falta {
                        endereco: atual,
                        acesso: Acesso::Escrita,
                        motivo,
                    })?;
            }
            restante = &restante[n..];
            if !restante.is_empty() {
                atual = atual.checked_add(n as u32).ok_or(Falta {
                    endereco: atual,
                    acesso: Acesso::Escrita,
                    motivo: MotivoDeFalta::Transbordo,
                })?;
            }
        }
        Ok(())
    }

    /// Lê um `u8` de um endereço virtual.
    pub fn ler_u8(&self, va: GuestVirt) -> Result<u8, Falta> {
        let mut b = [0u8; 1];
        self.ler_bytes(va, &mut b)?;
        Ok(b[0])
    }

    /// Lê um `u16` little-endian, possivelmente desalinhado.
    pub fn ler_u16(&self, va: GuestVirt) -> Result<u16, Falta> {
        let mut b = [0u8; 2];
        self.ler_bytes(va, &mut b)?;
        Ok(u16::from_le_bytes(b))
    }

    /// Lê um `u32` little-endian, possivelmente desalinhado e atravessando página.
    pub fn ler_u32(&self, va: GuestVirt) -> Result<u32, Falta> {
        let mut b = [0u8; 4];
        self.ler_bytes(va, &mut b)?;
        Ok(u32::from_le_bytes(b))
    }

    /// Lê um `u64` little-endian.
    pub fn ler_u64(&self, va: GuestVirt) -> Result<u64, Falta> {
        let mut b = [0u8; 8];
        self.ler_bytes(va, &mut b)?;
        Ok(u64::from_le_bytes(b))
    }

    /// Escreve um `u8`.
    pub fn escrever_u8(&mut self, va: GuestVirt, v: u8) -> Result<(), Falta> {
        self.escrever_bytes(va, &[v])
    }

    /// Escreve um `u16` little-endian.
    pub fn escrever_u16(&mut self, va: GuestVirt, v: u16) -> Result<(), Falta> {
        self.escrever_bytes(va, &v.to_le_bytes())
    }

    /// Escreve um `u32` little-endian.
    pub fn escrever_u32(&mut self, va: GuestVirt, v: u32) -> Result<(), Falta> {
        self.escrever_bytes(va, &v.to_le_bytes())
    }

    /// Escreve um `u64` little-endian.
    pub fn escrever_u64(&mut self, va: GuestVirt, v: u64) -> Result<(), Falta> {
        self.escrever_bytes(va, &v.to_le_bytes())
    }

    // ─── visão do kernel: endereço físico ─────────────────────────────────────────

    /// Lê bytes de um endereço físico, sem passar pela MMU.
    pub fn ler_fisico(&self, pa: GuestPhys, destino: &mut [u8]) -> Result<(), Falta> {
        self.ler_regiao(pa, destino, Acesso::Leitura)
            .map_err(|motivo| Falta {
                endereco: GuestVirt(pa.0),
                acesso: Acesso::Leitura,
                motivo,
            })
    }

    /// Escreve bytes em um endereço físico, sem passar pela MMU.
    pub fn escrever_fisico(&mut self, pa: GuestPhys, origem: &[u8]) -> Result<(), Falta> {
        self.escrever_regiao(pa, origem).map_err(|motivo| Falta {
            endereco: GuestVirt(pa.0),
            acesso: Acesso::Escrita,
            motivo,
        })
    }

    /// Mapeia uma faixa virtual sobre uma física, com permissão.
    ///
    /// Atalho para `mmu_mut().mapear`, que é o que o kernel HLE vai chamar.
    pub fn mapear(
        &mut self,
        va: GuestVirt,
        pa: GuestPhys,
        paginas: u32,
        perm: Perm,
    ) -> Result<(), Falta> {
        self.mmu.mapear(va, pa, paginas, perm)
    }

    // ─── visão da GPU: DMA ────────────────────────────────────────────────────────

    /// Fatia contígua da RAM, por endereço físico.
    ///
    /// É assim que a GPU lerá vértices, texturas e pushbuffer na Fase 5: o NV2A faz DMA
    /// por endereço físico e não passa pela MMU. Erra se a faixa não estiver inteira
    /// dentro da RAM — o que, para a GPU, significa objeto DMA mal montado pelo título.
    pub fn dma(&self, pa: GuestPhys, len: u32) -> Result<&[u8], Falta> {
        let erro = |motivo| Falta {
            endereco: GuestVirt(pa.0),
            acesso: Acesso::Leitura,
            motivo,
        };
        if !self.mapa.faixa_e_ram(pa, len) {
            return Err(erro(MotivoDeFalta::RegiaoInvalida(pa)));
        }
        self.arena
            .faixa(ArenaOff(pa.0), len as usize)
            .map_err(|_| erro(MotivoDeFalta::RegiaoInvalida(pa)))
    }

    // ─── acesso por região ────────────────────────────────────────────────────────

    /// Lê de uma região física. O pedaço nunca atravessa página.
    fn ler_regiao(
        &self,
        pa: GuestPhys,
        destino: &mut [u8],
        acesso: Acesso,
    ) -> std::result::Result<(), MotivoDeFalta> {
        let tamanho = tamanho_para_telemetria(destino.len());
        match self.mapa.classificar(pa) {
            Regiao::Ram(off) => self
                .arena
                .ler(off, destino)
                .map_err(|_| MotivoDeFalta::RegiaoInvalida(pa)),
            Regiao::Dispositivo { id, deslocamento } => {
                if id == Dispositivo::Nv2a
                    && let Some(value) = self
                        .ler_nv2a(deslocamento, destino.len())
                        .map_err(|o_que| MotivoDeFalta::NaoImplementado { fisico: pa, o_que })?
                {
                    destino.copy_from_slice(&value.to_le_bytes()[..destino.len()]);
                    return Ok(());
                }
                self.telemetria.registrar(
                    pa,
                    Operacao::Leitura,
                    tamanho,
                    Motivo::NaoImplementado(id),
                    None,
                    self.contexto.get(),
                );
                // Zero é placeholder declarado, não comportamento observado em
                // hardware: o dispositivo chega na fase indicada pela telemetria.
                destino.fill(0);
                Ok(())
            }
            Regiao::Flash { .. } => {
                self.telemetria.registrar(
                    pa,
                    Operacao::Leitura,
                    tamanho,
                    Motivo::Flash,
                    None,
                    self.contexto.get(),
                );
                // Flash apagada lê 0xFF. Não bootamos BIOS (ADR-0001).
                destino.fill(0xFF);
                Ok(())
            }
            Regiao::Tiled { .. } => {
                self.telemetria.registrar(
                    pa,
                    Operacao::Leitura,
                    tamanho,
                    Motivo::Tiled,
                    None,
                    self.contexto.get(),
                );
                Err(MotivoDeFalta::NaoImplementado {
                    fisico: pa,
                    o_que: "janela tiled, Fase 5",
                })
            }
            Regiao::Buraco => {
                self.telemetria.registrar(
                    pa,
                    if acesso.e_escrita() {
                        Operacao::Escrita
                    } else {
                        Operacao::Leitura
                    },
                    tamanho,
                    Motivo::Desconhecido,
                    None,
                    self.contexto.get(),
                );
                Err(MotivoDeFalta::RegiaoInvalida(pa))
            }
        }
    }

    fn ler_nv2a(&self, offset: u32, len: usize) -> std::result::Result<Option<u32>, &'static str> {
        if let Some(value) = self.nv2a_pfifo.read(offset, len)? {
            Ok(Some(value))
        } else if self.nv2a_pramin.validate_access(offset, len)? {
            Ok(self.nv2a_pramin.read(offset))
        } else if self.nv2a_pfb.validate_access(offset, len)? {
            Ok(self.nv2a_pfb.read(offset))
        } else if self.nv2a_pramdac.validate_access(offset, len)? {
            Ok(self.nv2a_pramdac.coefficient())
        } else if len == 4 {
            Ok(self.nv2a_irq.read(offset))
        } else {
            Ok(None)
        }
    }

    fn escrever_nv2a(&mut self, offset: u32, bytes: &[u8]) -> Result<bool, &'static str> {
        if self.nv2a_pfifo.validate_access(offset, bytes.len(), true)? {
            self.nv2a_pfifo.write(
                offset,
                u32::from_le_bytes(bytes.try_into().expect("largura validada")),
            );
        } else if self.nv2a_pramin.validate_access(offset, bytes.len())? {
            self.nv2a_pramin.write(
                offset,
                u32::from_le_bytes(bytes.try_into().expect("largura validada")),
            );
        } else if self.nv2a_pfb.validate_access(offset, bytes.len())? {
            self.nv2a_pfb.write(
                offset,
                u32::from_le_bytes(bytes.try_into().expect("largura validada")),
            );
        } else if self.nv2a_pramdac.validate_access(offset, bytes.len())? {
            self.nv2a_pramdac.write(u32::from_le_bytes(
                bytes.try_into().expect("largura validada"),
            ));
        } else if let Ok(word) = <[u8; 4]>::try_from(bytes) {
            return Ok(self.nv2a_irq.write(offset, u32::from_le_bytes(word)));
        } else {
            return Ok(false);
        }
        Ok(true)
    }

    /// Escreve em uma região física. O pedaço nunca atravessa página.
    fn escrever_regiao(
        &mut self,
        pa: GuestPhys,
        origem: &[u8],
    ) -> std::result::Result<(), MotivoDeFalta> {
        let tamanho = tamanho_para_telemetria(origem.len());
        let valor = valor_para_telemetria(origem);
        match self.mapa.classificar(pa) {
            Regiao::Ram(off) => self
                .arena
                .escrever(off, origem)
                .map_err(|_| MotivoDeFalta::RegiaoInvalida(pa)),
            Regiao::Dispositivo { id, deslocamento } => {
                if id == Dispositivo::Nv2a
                    && self
                        .escrever_nv2a(deslocamento, origem)
                        .map_err(|o_que| MotivoDeFalta::NaoImplementado { fisico: pa, o_que })?
                {
                    return Ok(());
                }
                self.telemetria.registrar(
                    pa,
                    Operacao::Escrita,
                    tamanho,
                    Motivo::NaoImplementado(id),
                    valor,
                    self.contexto.get(),
                );
                // Descartada. Quando o dispositivo existir, a escrita vai para o modelo
                // dele; até lá, registrar é o comportamento honesto.
                Ok(())
            }
            Regiao::Flash { .. } => {
                self.telemetria.registrar(
                    pa,
                    Operacao::Escrita,
                    tamanho,
                    Motivo::Flash,
                    valor,
                    self.contexto.get(),
                );
                Ok(())
            }
            Regiao::Tiled { .. } => {
                self.telemetria.registrar(
                    pa,
                    Operacao::Escrita,
                    tamanho,
                    Motivo::Tiled,
                    valor,
                    self.contexto.get(),
                );
                Err(MotivoDeFalta::NaoImplementado {
                    fisico: pa,
                    o_que: "janela tiled, Fase 5",
                })
            }
            Regiao::Buraco => {
                self.telemetria.registrar(
                    pa,
                    Operacao::Escrita,
                    tamanho,
                    Motivo::Desconhecido,
                    valor,
                    self.contexto.get(),
                );
                Err(MotivoDeFalta::RegiaoInvalida(pa))
            }
        }
    }

    /// Copia bytes para a RAM por endereço físico, direto na arena.
    ///
    /// É o que o loader usa para despejar uma seção do XBE: não passa pela MMU (as
    /// páginas podem ainda não estar mapeadas) e não registra telemetria.
    pub fn carregar_em_fisico(&mut self, pa: GuestPhys, dados: &[u8]) -> Result<()> {
        let len = u32::try_from(dados.len()).unwrap_or(u32::MAX);
        if !self.mapa.faixa_e_ram(pa, len) {
            return Err(xbox_core::Error::format(format!(
                "carga de {len} bytes em {pa} não cabe inteira na RAM"
            )));
        }
        self.arena.escrever(ArenaOff(pa.0), dados)
    }
}

/// Quantos bytes faltam até o fim da página de 4 KiB que contém `va`.
fn bytes_ate_fim_da_pagina(va: GuestVirt) -> usize {
    PAGE_SIZE - va.deslocamento() as usize
}

/// Tamanho do acesso para a telemetria, saturado no que cabe em `u8`.
fn tamanho_para_telemetria(len: usize) -> u8 {
    u8::try_from(len).unwrap_or(u8::MAX)
}

/// Valor de uma escrita pequena, para a telemetria.
fn valor_para_telemetria(origem: &[u8]) -> Option<u32> {
    match origem.len() {
        1 => Some(u32::from(origem[0])),
        2 => Some(u32::from(u16::from_le_bytes([origem[0], origem[1]]))),
        4 => Some(u32::from_le_bytes([
            origem[0], origem[1], origem[2], origem[3],
        ])),
        _ => None,
    }
}

impl Snapshot for Bus {
    fn save(&self, out: &mut SnapshotWriter) {
        out.section("BUS0", 8);
        out.u64(self.arena.len() as u64);
        self.arena.save(out);
        self.mmu.save(out);
        self.frames.save(out);
        self.nv2a_irq.save(out);
        self.nv2a_pramdac.save(out);
        self.nv2a_pfb.save(out);
        self.nv2a_pramin.save(out);
        self.boot_dma.save(out);
        self.nv2a_pfifo.save(out);
    }

    fn load(&mut self, inp: &mut SnapshotReader<'_>) -> std::result::Result<(), SnapshotError> {
        let versao = inp.section("BUS0", 8)?;
        if !matches!(versao, 1..=8) {
            return Err(SnapshotError::BadValue {
                section: "BUS0",
                what: "versão de bus desconhecida",
            });
        }
        let tamanho = inp.u64()? as usize;
        if tamanho != self.arena.len() {
            return Err(SnapshotError::BadValue {
                section: "BUS0",
                what: "tamanho de RAM diferente do build atual",
            });
        }
        self.arena.load(inp)?;
        self.mmu.load(inp)?;
        self.frames.load(inp)?;
        // v1 não tinha os registradores de interrupção: migra para o estado de RESET.
        if versao >= 2 {
            self.nv2a_irq.load(inp)?;
        } else {
            self.nv2a_irq = Nv2aIrq::default();
        }
        if versao >= 3 {
            self.nv2a_pramdac.load(inp)?;
        } else {
            self.nv2a_pramdac = Nv2aPramdac::legacy();
        }
        if versao >= 4 {
            self.nv2a_pfb.load(inp)?;
        } else {
            self.nv2a_pfb = Nv2aPfb::legacy();
        }
        if versao >= 5 {
            self.nv2a_pramin.load(inp)?;
        } else {
            self.nv2a_pramin = Nv2aPramin::legacy();
        }
        let mut boot_dma = BootDmaPage::default();
        if versao >= 6 {
            boot_dma.load(inp)?;
        }
        boot_dma
            .validate(self)
            .map_err(|what| SnapshotError::BadValue {
                section: "BTPG",
                what,
            })?;
        self.boot_dma = boot_dma;
        if versao >= 7 {
            self.nv2a_pfifo.load(inp)?;
        } else {
            self.nv2a_pfifo = Nv2aPfifo::legacy();
        }
        Ok(())
    }
}

/// Barramento com a RAM padrão do console, para quem só quer começar.
impl Default for Bus {
    fn default() -> Self {
        Self::novo()
            .unwrap_or_else(|e| panic!("não foi possível criar a RAM de {RAM_SIZE} bytes: {e}"))
    }
}
