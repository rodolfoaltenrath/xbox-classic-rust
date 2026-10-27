//! Implementação Linux/X11 da verificação (ver `main.rs`).

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use xbox_platform::verificacao_x11::{VerificadorX11, teclas};

const LARGURA: u16 = 640;
const ALTURA: u16 = 480;
/// Inclui a inicialização do binário release; não inclui compilar.
const INICIO_MAXIMO: Duration = Duration::from_secs(300);
const FECHAMENTO_MAXIMO: Duration = Duration::from_secs(5);

type Resultado<T> = Result<T, String>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Modo {
    Apos,
    Durante,
    Continuo,
}

struct Execucao {
    filho: Child,
    stderr: PathBuf,
    saida: PathBuf,
    relatorio: Vec<String>,
}

impl Execucao {
    fn iniciar(executavel: &Path, saida: &Path, argumentos: &[String]) -> Resultado<Self> {
        let stderr = saida.join("stderr.log");
        let abrir = |p: &Path| File::create(p).map_err(|e| format!("{}: {e}", p.display()));
        let filho = Command::new(executavel)
            .args(argumentos)
            .stdout(Stdio::from(abrir(&saida.join("stdout.log"))?))
            .stderr(Stdio::from(abrir(&stderr)?))
            .spawn()
            .map_err(|e| format!("não executou {}: {e}", executavel.display()))?;
        Ok(Self {
            filho,
            stderr,
            saida: saida.to_path_buf(),
            relatorio: vec![format!("argumentos: {}", argumentos.join(" "))],
        })
    }

    /// Linhas completas do stderr (a última pode estar sendo escrita).
    fn linhas(&self) -> Vec<String> {
        let texto = fs::read_to_string(&self.stderr).unwrap_or_default();
        let completas = texto.rsplit_once('\n').map_or("", |(c, _)| c);
        completas.lines().map(str::to_owned).collect()
    }

    fn contar(&self, trecho: &str) -> usize {
        self.linhas().iter().filter(|l| l.contains(trecho)).count()
    }

    /// Espera até `trecho` aparecer `vezes` vezes no stderr. Falha se o processo sair
    /// antes ou o prazo vencer.
    fn esperar_linha(&mut self, trecho: &str, vezes: usize, prazo: Duration) -> Resultado<()> {
        let inicio = Instant::now();
        loop {
            if self.contar(trecho) >= vezes {
                return Ok(());
            }
            if let Ok(Some(status)) = self.filho.try_wait() {
                return Err(format!("processo saiu ({status}) antes de {trecho:?}"));
            }
            if inicio.elapsed() > prazo {
                return Err(format!("{trecho:?} não apareceu em {prazo:?}"));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Fecha pela janela e mede até o processo sair.
    fn fechar(&mut self, v: &mut VerificadorX11, janela: u32) -> Resultado<(Duration, i32)> {
        let inicio = Instant::now();
        v.fechar(janela).map_err(|e| e.to_string())?;
        loop {
            if let Some(status) = self.filho.try_wait().map_err(|e| e.to_string())? {
                let tempo = inicio.elapsed();
                return Ok((tempo, status.code().unwrap_or(-1)));
            }
            if inicio.elapsed() > FECHAMENTO_MAXIMO {
                let _ = self.filho.kill();
                return Err(format!(
                    "processo não saiu em {FECHAMENTO_MAXIMO:?} após fechar"
                ));
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn anotar(&mut self, linha: String) {
        println!("{linha}");
        self.relatorio.push(linha);
    }
}

impl Drop for Execucao {
    fn drop(&mut self) {
        let _ = self.filho.kill();
        let _ = self.filho.wait();
        let _ = fs::write(
            self.saida.join("resultado.txt"),
            self.relatorio.join("\n") + "\n",
        );
    }
}

/// Espera a janela do Obsidian aparecer e ficar visível (captura sem `BadMatch`).
fn localizar(v: &mut VerificadorX11, execucao: &mut Execucao) -> Resultado<u32> {
    let inicio = Instant::now();
    loop {
        if let Some(&janela) = v.localizar().map_err(|e| e.to_string())?.first()
            && v.capturar(janela, LARGURA, ALTURA).is_ok()
        {
            return Ok(janela);
        }
        if let Ok(Some(status)) = execucao.filho.try_wait() {
            return Err(format!("processo saiu ({status}) sem abrir janela"));
        }
        if inicio.elapsed() > INICIO_MAXIMO {
            return Err("janela do Obsidian não apareceu".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Pixels com cor e linhas com algum pixel, contando como o `main` gerado.
fn medir(bgra: &[u8]) -> (usize, usize) {
    let linha = usize::from(LARGURA) * 4;
    let mut pixels = 0;
    let mut linhas = 0;
    for l in bgra.chunks_exact(linha) {
        let n = l
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[0] != 0 || p[1] != 0 || p[2] != 0)
            .count();
        pixels += n;
        linhas += usize::from(n > 0);
    }
    (pixels, linhas)
}

fn diferencas_rgb(a: &[u8], b: &[u8]) -> usize {
    a.as_chunks::<4>()
        .0
        .iter()
        .zip(b.as_chunks::<4>().0)
        .filter(|(x, y)| x[..3] != y[..3])
        .count()
}

fn gravar_ppm(caminho: &Path, bgra: &[u8]) -> Resultado<()> {
    let mut b = format!("P6\n{LARGURA} {ALTURA}\n255\n").into_bytes();
    for p in bgra.as_chunks::<4>().0 {
        b.extend_from_slice(&[p[2], p[1], p[0]]);
    }
    fs::write(caminho, b).map_err(|e| format!("{}: {e}", caminho.display()))
}

fn modo_apos(executavel: &Path, saida: &Path, fatias: &str) -> Resultado<()> {
    let guest = saida.join("guest.bgra");
    let args = ["--janela", "--fatias", fatias, "--quadro-apresentado"]
        .map(String::from)
        .into_iter()
        .chain([guest.display().to_string()])
        .collect::<Vec<_>>();
    let mut x = Execucao::iniciar(executavel, saida, &args)?;
    let mut v = VerificadorX11::conectar().map_err(|e| e.to_string())?;
    x.esperar_linha("janela mantida aberta", 1, INICIO_MAXIMO)?;
    let esperado = fs::read(&guest).map_err(|e| format!("{}: {e}", guest.display()))?;
    let janela = localizar(&mut v, &mut x)?;
    // O laço de espera repinta a cada 30 ms; a primeira captura pode anteceder o
    // primeiro PutImage depois do mapeamento.
    let inicio = Instant::now();
    let capturado = loop {
        let c = v
            .capturar(janela, LARGURA, ALTURA)
            .map_err(|e| e.to_string())?;
        if diferencas_rgb(&c, &esperado) == 0 || inicio.elapsed() > Duration::from_secs(5) {
            break c;
        }
        std::thread::sleep(Duration::from_millis(30));
    };
    fs::write(saida.join("display.bgra"), &capturado).map_err(|e| e.to_string())?;
    gravar_ppm(&saida.join("display.ppm"), &capturado)?;
    gravar_ppm(&saida.join("guest.ppm"), &esperado)?;
    let diferentes = diferencas_rgb(&capturado, &esperado);
    let (pixels, linhas) = medir(&capturado);
    let (pixels_guest, linhas_guest) = medir(&esperado);
    x.anotar(format!("diferencas_rgb: {diferentes}"));
    x.anotar(format!(
        "captura: {pixels} pixels coloridos em {linhas} linhas"
    ));
    x.anotar(format!(
        "exportacao: {pixels_guest} pixels coloridos em {linhas_guest} linhas"
    ));
    let (tempo, status) = x.fechar(&mut v, janela)?;
    x.anotar(format!(
        "fechamento_apos_orcamento_ms: {:.3}",
        tempo.as_secs_f64() * 1000.0
    ));
    x.anotar(format!("status: {status}"));
    if let Some(parada) = x.linhas().iter().rev().find(|l| l.starts_with("Error:")) {
        x.anotar(format!("parada: {parada}"));
    }
    if diferentes != 0 || capturado.len() != esperado.len() {
        return Err(format!(
            "{diferentes} pixels RGB diferentes entre janela e exportação"
        ));
    }
    Ok(())
}

fn modo_durante(executavel: &Path, saida: &Path, fatias: &str) -> Resultado<()> {
    let args = ["--janela", "--fatias", fatias].map(String::from).to_vec();
    let mut x = Execucao::iniciar(executavel, saida, &args)?;
    let mut v = VerificadorX11::conectar().map_err(|e| e.to_string())?;
    let janela = localizar(&mut v, &mut x)?;
    let capturado = v
        .capturar(janela, LARGURA, ALTURA)
        .map_err(|e| e.to_string())?;
    gravar_ppm(&saida.join("display-durante.ppm"), &capturado)?;
    let (pixels, linhas) = medir(&capturado);
    x.anotar(format!(
        "captura_durante: {pixels} pixels coloridos em {linhas} linhas"
    ));
    let (tempo, status) = x.fechar(&mut v, janela)?;
    x.anotar(format!(
        "fechamento_durante_ms: {:.3}",
        tempo.as_secs_f64() * 1000.0
    ));
    x.anotar(format!("status: {status}"));
    let host = x.contar("interrompida pelo host");
    x.anotar(format!("interrompida_pelo_host: {host}"));
    if host == 0 || status != 0 {
        return Err("fechar durante a execução não terminou como parada pelo host".into());
    }
    Ok(())
}

fn modo_continuo(executavel: &Path, saida: &Path, fatias: &str) -> Resultado<()> {
    let estados = saida.join("estados");
    let args = ["--janela", "--continuo", "--fatias", fatias, "--estados"]
        .map(String::from)
        .into_iter()
        .chain([estados.display().to_string()])
        .collect::<Vec<_>>();
    let mut x = Execucao::iniciar(executavel, saida, &args)?;
    let mut v = VerificadorX11::conectar().map_err(|e| e.to_string())?;
    let janela = localizar(&mut v, &mut x)?;
    for (tecla, nome, linha) in [
        (teclas::ESPACO, "Espaço", "Sessão pausada"),
        (teclas::ESPACO, "Espaço", "Sessão retomada"),
        (teclas::F5, "F5", "Estado salvo:"),
        (teclas::F9, "F9", "Estado restaurado:"),
    ] {
        let antes = x.contar(linha);
        v.tecla(janela, tecla).map_err(|e| e.to_string())?;
        x.esperar_linha(linha, antes + 1, Duration::from_secs(30))?;
        let ultima = x
            .linhas()
            .into_iter()
            .rev()
            .find(|l| l.contains(linha))
            .unwrap_or_default();
        x.anotar(format!("{nome}: {ultima}"));
    }
    let (tempo, status) = x.fechar(&mut v, janela)?;
    x.anotar(format!(
        "fechamento_continuo_ms: {:.3}",
        tempo.as_secs_f64() * 1000.0
    ));
    x.anotar(format!("status: {status}"));
    let host = x.contar("interrompida pelo host");
    x.anotar(format!("interrompida_pelo_host: {host}"));
    if host == 0 || status != 0 {
        return Err("fechar no modo contínuo não terminou como parada pelo host".into());
    }
    Ok(())
}

pub(crate) fn executar(args: &[String]) -> Resultado<()> {
    let [modo, executavel, saida, resto @ ..] = args else {
        return Err(
            "uso: verificar-janela <apos|durante|continuo> <executavel> <saida-nova> [fatias]"
                .into(),
        );
    };
    let modo = match modo.as_str() {
        "apos" => Modo::Apos,
        "durante" => Modo::Durante,
        "continuo" => Modo::Continuo,
        outro => return Err(format!("modo desconhecido: {outro}")),
    };
    let padrao = match modo {
        Modo::Apos | Modo::Continuo => "64",
        Modo::Durante => "1000000",
    };
    let fatias = match resto {
        [] => padrao,
        [f] if f.parse::<u32>().is_ok_and(|n| n > 0) => f.as_str(),
        _ => return Err("fatias deve ser um inteiro positivo".into()),
    };
    let saida = PathBuf::from(saida);
    fs::create_dir(&saida).map_err(|e| format!("saída deve ser um diretório novo: {e}"))?;
    let executavel = fs::canonicalize(executavel).map_err(|e| format!("{executavel}: {e}"))?;
    match modo {
        Modo::Apos => modo_apos(&executavel, &saida, fatias),
        Modo::Durante => modo_durante(&executavel, &saida, fatias),
        Modo::Continuo => modo_continuo(&executavel, &saida, fatias),
    }
}

#[cfg(test)]
mod tests {
    use super::{ALTURA, LARGURA, diferencas_rgb, executar, medir};

    #[test]
    fn medir_conta_pixels_e_linhas_ignorando_alfa() {
        let mut q = vec![0u8; usize::from(LARGURA) * usize::from(ALTURA) * 4];
        q[3] = 0xFF;
        q[4..8].copy_from_slice(&[0, 0, 9, 0]);
        let segunda = usize::from(LARGURA) * 4;
        q[segunda] = 1;
        assert_eq!(medir(&q), (2, 2));
    }

    #[test]
    fn diferencas_ignoram_o_alfa_da_captura() {
        assert_eq!(
            diferencas_rgb(&[1, 2, 3, 0, 4, 5, 6, 0], &[1, 2, 3, 255, 4, 5, 7, 255]),
            1
        );
    }

    #[test]
    fn argumentos_invalidos_falham_antes_de_criar_saida() {
        for args in [
            vec![],
            vec!["x", "a", "b"],
            vec!["apos", "a", "b", "0"],
            vec!["apos", "a", "b", "1", "2"],
        ] {
            let args: Vec<String> = args.into_iter().map(String::from).collect();
            assert!(executar(&args).is_err(), "{args:?}");
        }
        assert!(!std::path::Path::new("b").exists());
    }
}
