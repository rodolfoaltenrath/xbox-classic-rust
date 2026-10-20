use std::path::Path;
use xbox_core::{Error, Result};
use xbox_formats::{Media, Xdvdfs};
use xbox_kernel::loader::{Image, Mode};

pub(super) fn check(path: &Path) -> Result<()> {
    let bytes = read_xbe(path)?;
    let image = Image::load(&bytes, Mode::Inspect).map_err(|e| Error::format(e.to_string()))?;
    let report = image
        .verify(&bytes)
        .map_err(|e| Error::format(e.to_string()))?;
    println!("Validação de memória T14 concluída:");
    println!("  seções ................. {}", report.sections);
    println!("  páginas da imagem ...... {}", report.pages);
    println!("  bytes verificados ...... {}", report.checked_bytes);
    println!(
        "  entry point ............ 0x{:08X} = 0x{:08X}",
        report.entry, report.entry_word
    );
    println!(
        "  ordinais pelo bus ...... {} (iguais ao arquivo)",
        report.imports.len()
    );
    println!("  alias por PA ........... bytes iguais");
    println!(
        "  página ausente ......... 0x{:08X} (falta confirmada)",
        report.unmapped_probe
    );
    Ok(())
}

fn read_xbe(path: &Path) -> Result<Vec<u8>> {
    const MAX_XBE: u64 = 128 * 1024 * 1024;
    let file = if path.is_dir() {
        path.join("default.xbe")
    } else {
        path.to_owned()
    };
    let media = Media::open(&file)?;
    if *media.read_at(0, 4)? == xbox_formats::xbe::MAGIC {
        if media.len() > MAX_XBE {
            return Err(Error::format("XBE excede limite de inspeção de 128 MiB"));
        }
        return Ok(media.read_at(0, media.len() as usize)?.into_owned());
    }
    let disc = Xdvdfs::open(media)?;
    let entry = disc.default_xbe()?;
    if u64::from(entry.size) > MAX_XBE {
        return Err(Error::format(
            "default.xbe excede limite de inspeção de 128 MiB",
        ));
    }
    Ok(disc.read(&entry)?.into_owned())
}
