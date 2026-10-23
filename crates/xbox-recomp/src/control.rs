//! Política de sessão do host, sem estado novo no guest (ADR-0077).
use std::path::{Path, PathBuf};

use crate::checkpoint;
use crate::runtime::{Desfecho, Exit, Limites, Session, State};
use crate::{Error, Result};
use xbox_mem::Bus;

/// Pausa e gerações de save state. Não representa um controle Xbox/USB.
#[derive(Debug, Default)]
pub struct Control {
    paused: bool,
    directory: Option<PathBuf>,
    latest: Option<PathBuf>,
    generation: u64,
}

impl Control {
    pub fn new(directory: Option<PathBuf>) -> Self {
        Self {
            directory,
            ..Self::default()
        }
    }

    pub const fn paused(&self) -> bool {
        self.paused
    }

    pub fn toggle_pause(&mut self) {
        self.paused = !self.paused;
    }

    pub fn latest(&self) -> Option<&Path> {
        self.latest.as_deref()
    }

    /// Só publica latest após save completo. Gerações existentes são preservadas.
    pub fn save(
        &mut self,
        session: &Session,
        image: &[u8],
    ) -> std::result::Result<&Path, checkpoint::Error> {
        let directory = self
            .directory
            .as_ref()
            .ok_or(checkpoint::Error::Invalid("F5 exige --estados diretório"))?;
        std::fs::create_dir_all(directory)?;
        let path = loop {
            self.generation = self
                .generation
                .checked_add(1)
                .ok_or(checkpoint::Error::Invalid("contador de gerações esgotado"))?;
            let path = directory.join(format!("checkpoint-{:06}.obsstate", self.generation));
            if !path.try_exists()? {
                break path;
            }
        };
        checkpoint::save_new(&path, session, image)?;
        self.latest = Some(path);
        Ok(self.latest().expect("save publicado"))
    }

    /// A pausa é estado do host e permanece como estava. Só troca o guest após
    /// validação integral; não apaga a geração nem produz uma nova ao restaurar.
    pub fn restore(
        &self,
        session: &mut Session,
        image: &[u8],
    ) -> std::result::Result<(), checkpoint::Error> {
        let path = self.latest().ok_or(checkpoint::Error::Invalid(
            "nenhum estado salvo por F5 nesta execução",
        ))?;
        let restored = checkpoint::load_file(path, image)?;
        *session = restored;
        Ok(())
    }

    /// Avança um trecho finito. Só `BudgetExhausted` autoriza continuar. Pausado
    /// não chama run, apresentação, scheduler ou avanço de Clock.
    pub fn advance<R, A>(
        &self,
        session: &mut Session,
        limits: Limites,
        run: R,
        present: A,
    ) -> Result<Option<Desfecho>>
    where
        R: FnMut(&mut State, &mut Bus, u64) -> Result<Exit>,
        A: FnMut(&Session) -> Result<bool>,
    {
        if self.paused {
            return Ok(None);
        }
        match session.conduzir(limits, run, present) {
            Ok(outcome) => Ok(Some(outcome)),
            Err(Error::BudgetExhausted { .. }) => Ok(None),
            Err(error) => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static ID: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn pausa_nao_executa_guest_e_continuacao_so_aceita_orcamento() {
        let image = xbox_formats::fixtures::xbe_com_codigo(crate::DEMO_CODE).unwrap();
        let mut session = Session::load(&image).unwrap();
        let before = checkpoint::encode(&session, &image).unwrap();
        let mut control = Control::default();
        control.toggle_pause();
        assert!(control.paused());
        let limits = Limites {
            blocos_por_fatia: 1,
            fatias: 1,
        };
        assert_eq!(
            control
                .advance(
                    &mut session,
                    limits,
                    |_, _, _| panic!("guest durante pausa"),
                    |_| panic!("callback durante pausa")
                )
                .unwrap(),
            None
        );
        assert_eq!(checkpoint::encode(&session, &image).unwrap(), before);
        control.toggle_pause();
        assert_eq!(
            control
                .advance(
                    &mut session,
                    limits,
                    |_, _, _| Ok(Exit::Budget),
                    |_| Ok(false)
                )
                .unwrap(),
            None
        );
        assert!(matches!(
            control.advance(
                &mut session,
                limits,
                |_, _, _| Err(Error::UnknownBlock(123)),
                |_| Ok(false)
            ),
            Err(Error::UnknownBlock(123))
        ));
        assert!(matches!(
            control.advance(
                &mut session,
                limits,
                |_, _, _| Ok(Exit::Breakpoint { pc: 123 }),
                |_| Ok(false)
            ),
            Err(Error::GuestBreakpoint { pc: 123, .. })
        ));
        assert_eq!(
            control
                .advance(
                    &mut session,
                    limits,
                    |_, _, _| Ok(Exit::Budget),
                    |_| Ok(true)
                )
                .unwrap(),
            Some(Desfecho::ParadoPeloHost)
        );
    }

    #[test]
    fn geracoes_nao_substituem_arquivo_e_restore_falhado_preserva_sessao() {
        let image = xbox_formats::fixtures::xbe_com_codigo(crate::DEMO_CODE).unwrap();
        let mut session = Session::load(&image).unwrap();
        let directory = std::env::temp_dir().join(format!(
            "obsidian-control-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let collision = directory.join("checkpoint-000001.obsstate");
        std::fs::write(&collision, b"original").unwrap();
        let mut control = Control::new(Some(directory.clone()));
        assert!(control.restore(&mut session, &image).is_err());
        let first = control.save(&session, &image).unwrap().to_path_buf();
        assert!(first.ends_with("checkpoint-000002.obsstate"));
        let saved = checkpoint::encode(&session, &image).unwrap();
        session.state.regs[0] = 123;
        control.toggle_pause();
        control.restore(&mut session, &image).unwrap();
        assert!(control.paused());
        assert_eq!(checkpoint::encode(&session, &image).unwrap(), saved);
        session.state.regs[0] = 456;
        let second = control.save(&session, &image).unwrap().to_path_buf();
        assert_ne!(first, second);
        assert_eq!(std::fs::read(&first).unwrap(), saved);
        std::fs::write(&second, b"corrupt").unwrap();
        let before = checkpoint::encode(&session, &image).unwrap();
        assert!(control.restore(&mut session, &image).is_err());
        assert_eq!(checkpoint::encode(&session, &image).unwrap(), before);
        assert!(control.paused());
        assert_eq!(std::fs::read(collision).unwrap(), b"original");
        std::fs::remove_dir_all(directory).unwrap();
        assert!(Control::default().save(&session, &image).is_err());
    }
}
