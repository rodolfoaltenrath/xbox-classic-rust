//! Inventário de dependências reais, sem executar código guest.

use std::{fmt::Write, io::Read, path::PathBuf, process::ExitCode};

use clap::Parser;
use xbox_formats::Xbe;
use xbox_kernel::imports::{Abi, Kind, lookup};

#[derive(Debug, Parser)]
#[command(
    name = "ordinal-report",
    version,
    about = "Inspeciona TLS, ordinais e bloqueios AOT de um XBE"
)]
struct Cli {
    arquivo: PathBuf,
}

fn report(bytes: &[u8]) -> Result<String, Box<dyn std::error::Error>> {
    let xbe = Xbe::parse(bytes)?;
    xbox_kernel::loader::validate_layout(&xbe)?;
    let mut out = format!(
        "Entry: 0x{:08X}\nSeções: {}\n",
        xbe.entry_point,
        xbe.sections.len()
    );
    if let Some(tls) = xbe.tls {
        writeln!(
            out,
            "TLS: 0x{:08X}..0x{:08X}; zero-fill={}; índice em 0x{:08X}; callbacks=0x{:08X}; characteristics=0x{:08X}",
            tls.data_start_address,
            tls.data_end_address,
            tls.size_of_zero_fill,
            tls.index_address,
            tls.callback_address,
            tls.characteristics
        )?;
    } else {
        out.push_str("TLS: ausente\n");
    }
    let imports = xbe.kernel_imports()?;
    let mut functions = 0;
    let mut data = 0;
    let mut unknown = 0;
    // Contados a partir do estado real, consultando a mesma fonte que o planejamento de
    // imports usa. O contador anterior era um literal e por isso não media nada.
    let mut dados_resolvidos = 0;
    let mut servicos_implementados = 0;
    out.push_str("\nOrdinal  Nome                                 ABI          Estado\n");
    for ordinal in &imports {
        if let Some(export) = lookup(*ordinal) {
            let (abi, state) = match export.kind {
                Kind::Data => {
                    data += 1;
                    match xbox_kernel::loader::data::address(*ordinal) {
                        Some(endereco) => {
                            dados_resolvidos += 1;
                            ("data".to_owned(), format!("dado em 0x{endereco:08X}"))
                        }
                        None => ("data".to_owned(), "dado pendente".to_owned()),
                    }
                }
                Kind::Function(abi) => {
                    functions += 1;
                    let abi = match abi {
                        Abi::Cdecl => "cdecl".to_owned(),
                        Abi::Stdcall(bytes) => format!("stdcall/{bytes}"),
                        Abi::Fastcall(bytes) => format!("fastcall/{bytes}"),
                    };
                    if xbox_kernel::imports::servico_implementado(*ordinal) {
                        servicos_implementados += 1;
                        (abi, "serviço implementado".to_owned())
                    } else {
                        (abi, "trampolim disponível; serviço pendente".to_owned())
                    }
                }
            };
            writeln!(out, "{ordinal:7}  {:36} {abi:12} {state}", export.name)?;
        } else {
            unknown += 1;
            writeln!(out, "{ordinal:7}  desconhecido")?;
        }
    }
    writeln!(
        out,
        "\nTotal: {} imports; {functions} funções; {data} dados; {unknown} desconhecidos; \
         {dados_resolvidos}/{data} dados com representação; \
         {servicos_implementados}/{functions} serviços implementados",
        imports.len()
    )?;
    match xbox_recomp::analysis::analyze(bytes) {
        Ok(program) => writeln!(
            out,
            "Análise AOT: {} instruções, {} blocos; não significa que serviços de kernel estejam implementados",
            program.instructions.len(),
            program.blocks.len()
        )?,
        Err(error) => writeln!(out, "Primeiro bloqueio AOT: {error}")?,
    }
    Ok(out)
}

fn run(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    let mut file = std::fs::File::open(&cli.arquivo)?;
    let mut bytes = Vec::new();
    // Limite na leitura, mesmo se o arquivo crescer depois de aberto.
    file.by_ref()
        .take(128 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 128 * 1024 * 1024 {
        return Err("XBE excede 128 MiB".into());
    }
    print!("{}", report(&bytes)?);
    Ok(())
}

fn main() -> ExitCode {
    match run(&Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("erro: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn report_distingue_abi_dados_desconhecidos_e_nao_confunde_resolucao_com_servico() {
        let mut bytes = xbox_formats::fixtures::xbe_com_codigo(&[0xC3]).unwrap();
        for (i, ordinal) in [8u32, 165, 322, 500].into_iter().enumerate() {
            bytes[0x2000 + i * 4..0x2004 + i * 4]
                .copy_from_slice(&(0x80000000 | ordinal).to_le_bytes());
        }
        let text = report(&bytes).unwrap();
        assert!(text.contains("DbgPrint"));
        assert!(text.contains("stdcall/4"));
        assert!(text.contains("XboxHardwareInfo"));
        // `XboxHardwareInfo` (322) é dado conhecido e **sem** representação: o relatório
        // precisa dizer pendente, não contar como resolvido.
        assert!(text.contains("dado pendente"));
        assert!(text.contains(
            "4 imports; 2 funções; 1 dados; 1 desconhecidos; \
             0/1 dados com representação; 0/2 serviços implementados"
        ));
        assert!(text.contains("Primeiro bloqueio AOT"));
        assert!(report(b"XBEH").is_err());
    }

    #[test]
    fn report_conta_dado_com_representacao_sem_marcar_servico() {
        // Os três dados do homebrew têm representação; nenhum serviço existe ainda. O
        // contador antes era literal e não distinguia os dois casos.
        let mut bytes = xbox_formats::fixtures::xbe_com_codigo(&[0xC3]).unwrap();
        for (i, ordinal) in [164u32, 259, 326, 8].into_iter().enumerate() {
            bytes[0x2000 + i * 4..0x2004 + i * 4]
                .copy_from_slice(&(0x80000000 | ordinal).to_le_bytes());
        }
        let text = report(&bytes).unwrap();
        assert!(
            text.contains("dado em 0x7FFF"),
            "endereço do dado não aparece"
        );
        assert!(!text.contains("dado pendente"));
        assert!(text.contains(
            "4 imports; 1 funções; 3 dados; 0 desconhecidos; \
             3/3 dados com representação; 0/1 serviços implementados"
        ));
    }
}
