//! Escreve as fixtures geradas por código em arquivos, para teste manual das
//! ferramentas (`xbetool`, `isotool`) e para inspeção com outras ferramentas.
//!
//! ```sh
//! cargo run -p xbox-formats --example gerar_fixtures -- <pasta>
//! ```

use std::path::PathBuf;

use xbox_formats::fixtures;

fn main() -> std::io::Result<()> {
    let destino: PathBuf = std::env::args_os()
        .nth(1)
        .map_or_else(|| PathBuf::from("."), PathBuf::from);
    std::fs::create_dir_all(&destino)?;

    let arquivos: [(&str, Vec<u8>); 3] = [
        ("fixture.xbe", fixtures::xbe_minimo()),
        ("fixture.iso", fixtures::xiso_minimo()),
        ("fixture-ciclo.iso", fixtures::xiso_com_ciclo()),
    ];

    for (nome, bytes) in arquivos {
        let caminho = destino.join(nome);
        std::fs::write(&caminho, &bytes)?;
        println!("{} ({} bytes)", caminho.display(), bytes.len());
    }
    Ok(())
}
