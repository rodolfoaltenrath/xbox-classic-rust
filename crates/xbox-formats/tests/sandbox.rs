//! Testes do sandbox das pastas montadas na VFS.
//!
//! Separado de `src/vfs.rs` porque criar link simbólico exige API específica de sistema
//! operacional, e a regra do ROADMAP §13.3 mantém `#[cfg(target_os)]` fora dos crates
//! de núcleo. Aqui é código de teste, não de produção.
//!
//! Ver ADR-0012 para o que está fechado e o que continua aberto.

use std::path::{Path, PathBuf};

use xbox_formats::{Mount, Vfs};

fn pasta(nome: &str) -> PathBuf {
    let p = std::env::temp_dir()
        .join("obsidian-sandbox-teste")
        .join(nome);
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// Cria um link simbólico de arquivo, ou devolve `false` se o sistema não permitir.
///
/// No Windows, criar link simbólico exige privilégio ou modo de desenvolvedor; quando
/// não há, o teste informa e passa, em vez de falhar por motivo alheio ao código.
fn link_de_arquivo(destino: &Path, link: &Path) -> bool {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(destino, link).is_ok()
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_file(destino, link).is_ok()
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (destino, link);
        false
    }
}

#[test]
fn link_simbolico_para_fora_da_raiz_e_bloqueado() {
    let raiz = pasta("link-arquivo");
    std::fs::write(raiz.join("dentro.bin"), b"ok").unwrap();

    // Um segredo fora da pasta montada, e um link para ele dentro dela.
    let fora = raiz.parent().unwrap().join("segredo.bin");
    std::fs::write(&fora, b"isto nao deveria ser legivel pelo titulo").unwrap();
    let link = raiz.join("atalho.bin");
    let _ = std::fs::remove_file(&link);

    if !link_de_arquivo(&fora, &link) {
        eprintln!(
            "aviso: este host não permite criar link simbólico; \
             o teste de fuga por link não foi exercitado"
        );
        return;
    }

    let mut vfs = Vfs::new();
    vfs.montar('T', Mount::Pasta(raiz.clone())).unwrap();

    // O caminho não tem `..` e está debaixo da raiz: só a canonicalização pega.
    let r = vfs.ler("T:\\atalho.bin");
    assert!(
        r.is_err(),
        "leitura através de link para fora foi permitida"
    );
    let msg = r.unwrap_err().to_string();
    assert!(
        msg.contains("link simbólico"),
        "bloqueou pelo motivo errado: {msg}"
    );

    // E o arquivo legítimo continua legível.
    assert_eq!(vfs.ler("T:\\dentro.bin").unwrap(), b"ok");

    std::fs::remove_file(&link).ok();
    std::fs::remove_file(&fora).ok();
    std::fs::remove_dir_all(&raiz).ok();
}

#[test]
fn link_simbolico_dentro_da_raiz_continua_funcionando() {
    // Link que aponta para dentro da própria pasta montada é legítimo: um título pode
    // ter um layout com atalho, e bloquear tudo seria restringir mais do que o console.
    let raiz = pasta("link-interno");
    std::fs::write(raiz.join("real.bin"), b"conteudo real").unwrap();
    let link = raiz.join("alias.bin");
    let _ = std::fs::remove_file(&link);

    if !link_de_arquivo(&raiz.join("real.bin"), &link) {
        eprintln!("aviso: host não permite link simbólico; teste não exercitado");
        return;
    }

    let mut vfs = Vfs::new();
    vfs.montar('T', Mount::Pasta(raiz.clone())).unwrap();
    assert_eq!(vfs.ler("T:\\alias.bin").unwrap(), b"conteudo real");

    std::fs::remove_file(&link).ok();
    std::fs::remove_dir_all(&raiz).ok();
}

#[test]
fn travessia_por_componente_continua_bloqueada() {
    // Cobre o que já estava coberto, para que a canonicalização não tenha afrouxado as
    // barreiras anteriores.
    let raiz = pasta("componentes");
    std::fs::write(raiz.join("ok.bin"), b"dentro").unwrap();
    let fora = raiz.parent().unwrap().join("fora.bin");
    std::fs::write(&fora, b"fora").unwrap();

    let mut vfs = Vfs::new();
    vfs.montar('T', Mount::Pasta(raiz.clone())).unwrap();

    for ataque in [
        "T:\\..\\fora.bin",
        "T:\\sub\\..\\..\\fora.bin",
        "T:/../fora.bin",
        "T:\\C:\\Windows",
    ] {
        assert!(vfs.ler(ataque).is_err(), "travessia aceita: {ataque}");
    }
    assert_eq!(vfs.ler("T:\\ok.bin").unwrap(), b"dentro");

    std::fs::remove_file(&fora).ok();
    std::fs::remove_dir_all(&raiz).ok();
}

#[test]
fn arquivo_inexistente_da_erro_de_io_e_nao_de_sandbox() {
    // A canonicalização falha em caminho que não existe; isso não pode virar erro de
    // segurança, senão a mensagem engana quem está depurando um jogo.
    let raiz = pasta("inexistente");
    let mut vfs = Vfs::new();
    vfs.montar('T', Mount::Pasta(raiz.clone())).unwrap();

    let msg = vfs.ler("T:\\nao-existe.bin").unwrap_err().to_string();
    assert!(
        !msg.contains("link simbólico") && !msg.contains("raiz montada"),
        "erro de arquivo ausente virou erro de sandbox: {msg}"
    );

    std::fs::remove_dir_all(&raiz).ok();
}

/// Cria um atalho de **diretório**, ou devolve `false` se o sistema não permitir.
///
/// No Windows, link simbólico exige privilégio, mas *junction* de diretório não — e a
/// canonicalização resolve as duas. É por isso que este teste consegue exercitar a
/// barreira em hosts onde o teste de link simbólico desiste.
fn link_de_diretorio(destino: &Path, link: &Path) -> bool {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(destino, link).is_ok()
    }
    #[cfg(windows)]
    {
        std::process::Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(link)
            .arg(destino)
            .output()
            .is_ok_and(|s| s.status.success())
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (destino, link);
        false
    }
}

#[test]
fn atalho_de_diretorio_para_fora_da_raiz_e_bloqueado() {
    let raiz = pasta("junction");
    std::fs::write(raiz.join("dentro.bin"), b"ok").unwrap();

    // Pasta secreta fora da montagem, com um arquivo dentro.
    let fora = raiz.parent().unwrap().join("segredos");
    std::fs::create_dir_all(&fora).unwrap();
    std::fs::write(fora.join("chave.bin"), b"isto nao deveria ser legivel").unwrap();

    let link = raiz.join("atalho");
    let _ = std::fs::remove_dir(&link);
    if !link_de_diretorio(&fora, &link) {
        eprintln!("aviso: host não permite atalho de diretório; teste não exercitado");
        return;
    }

    let mut vfs = Vfs::new();
    vfs.montar('T', Mount::Pasta(raiz.clone())).unwrap();

    // Nenhum componente é `..` e o caminho está sob a raiz: só a canonicalização pega.
    let r = vfs.ler(r"T:\atalho\chave.bin");
    assert!(
        r.is_err(),
        "leitura através de atalho para fora foi permitida"
    );
    let msg = r.unwrap_err().to_string();
    assert!(
        msg.contains("link simbólico"),
        "bloqueou pelo motivo errado: {msg}"
    );

    // Listar através do atalho também tem de falhar.
    assert!(
        vfs.listar(r"T:\atalho").is_err(),
        "listagem através do atalho passou"
    );

    // E o legítimo continua funcionando.
    assert_eq!(vfs.ler(r"T:\dentro.bin").unwrap(), b"ok");

    let _ = std::fs::remove_dir(&link);
    std::fs::remove_dir_all(&fora).ok();
    std::fs::remove_dir_all(&raiz).ok();
}
