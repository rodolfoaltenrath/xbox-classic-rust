//! Dados exportados pelo kernel, em memória guest.
//!
//! Imports de dados não são funções: o slot do thunk recebe o **endereço do próprio
//! símbolo**, e o código compilado desreferencia a partir dali. Isso vale porque o
//! nxdk declara os exports com `__declspec(dllimport)` e o `.def` os marca `DATA`:
//! `XeImageFileName->Buffer` compila para carregar o slot e então ler `[addr + 4]`.
//!
//! ## As três formas, que são diferentes entre si
//!
//! | Export | Declaração no nxdk | O que isto exige |
//! |---|---|---|
//! | `LaunchDataPage` (164) | `PLAUNCH_DATA_PAGE LaunchDataPage;` | **ponteiro de 4 bytes gravável**, começando nulo |
//! | `PsThreadObjectType` (259) | `OBJECT_TYPE PsThreadObjectType;` | struct de 28 bytes num endereço estável |
//! | `XeImageFileName` (326) | `ANSI_STRING XeImageFileName[1];` | struct de 8 bytes **mais** um buffer com o caminho |
//! | `KeTickCount` (156) | `volatile ULONG KeTickCount;` | contador de ms que o **kernel** atualiza e o título só lê |
//!
//! Tratar os três como "uma tabela de zeros" seria errado de três maneiras distintas:
//!
//! - `LaunchDataPage = 0` é o estado inicial **correto** — `XGetLaunchInfo` testa nulo
//!   e aloca a página. Mas ele precisa ser **gravável**, porque a rotina escreve o
//!   ponteiro de volta na variável;
//! - `PsThreadObjectType` zerado passa enquanto só a identidade do endereço importar, e
//!   falha silenciosamente no instante em que algum serviço seguir um dos ponteiros de
//!   procedimento. Daí o envenenamento descrito abaixo;
//! - `XeImageFileName` com `Length = 0` **não** causa falta: o `strncpy` deste SDK
//!   curto-circuita em `n` antes de desreferenciar `Buffer`, e `nxGetCurrentXbeNtPath`
//!   devolve caminho vazio. É resultado errado sem erro — o pior dos casos — e por isso
//!   o campo recebe um caminho real e declarado.
//!
//! ## Duas páginas, porque permissão é por página
//!
//! A MMU do guest tem granularidade de 4 KiB, e o guest escreve `LaunchDataPage` mas
//! só lê os outros dois. Juntar tudo numa página obrigaria a torná-la gravável e
//! perderíamos a detecção de escrita indevida em dado de kernel. São duas páginas.
//!
//! ## Envenenamento em vez de zero
//!
//! Os ponteiros de procedimento de `OBJECT_TYPE` apontam, no console, para rotinas do
//! kernel. Não temos essas rotinas como endereço guest, e preenchê-los com zero
//! transformaria um uso indevido em salto para o endereço 0 — difícil de distinguir de
//! outros defeitos. Em vez disso, cada campo recebe um endereço distinto numa página
//! **não mapeada**: qualquer uso falha na hora, e o endereço da falta diz **qual campo**
//! foi usado.

use xbox_core::Error as CoreError;
use xbox_mem::{Acesso, Bus, GuestVirt, Perm};

use super::Result;

/// Página somente-leitura: tipos de objeto, `ANSI_STRING` e o buffer do caminho.
pub const DATA_RO_BASE: u32 = 0x7FFF_0000;
/// Página gravável: variáveis que o próprio título escreve.
pub const DATA_RW_BASE: u32 = 0x7FFF_1000;
/// Tamanho de cada página da região.
pub const DATA_SIZE: u32 = 4096;

/// Base dos endereços de envenenamento. Não é mapeada em nenhum momento.
pub const POISON_BASE: u32 = 0x7FFF_F000;

/// `OBJECT_TYPE`: cinco procedimentos, `DefaultObject` e `PoolTag`.
pub const OBJECT_TYPE_SIZE: u32 = 28;
/// `ANSI_STRING`: `Length`, `MaximumLength` e `Buffer`.
pub const ANSI_STRING_SIZE: u32 = 8;
/// Espaço reservado para o caminho da imagem, com o NUL final.
pub const MAX_IMAGE_PATH: usize = 256;

/// Deslocamento de `PsThreadObjectType` na página somente-leitura.
pub const OFF_THREAD_OBJECT_TYPE: u32 = 0x0000;
/// Deslocamento de `XeImageFileName` na página somente-leitura.
pub const OFF_IMAGE_FILE_NAME: u32 = 0x0100;
/// Deslocamento do buffer do caminho na página somente-leitura.
pub const OFF_IMAGE_PATH: u32 = 0x0200;
/// Deslocamento de `LaunchDataPage` na página gravável.
pub const OFF_LAUNCH_DATA_PAGE: u32 = 0x0000;
/// Deslocamento de `KeTickCount` na página somente-leitura.
pub const OFF_KE_TICK_COUNT: u32 = 0x0300;

/// Caminho NT da imagem exposto em `XeImageFileName`.
///
/// **Valor declarado, não derivado de mídia.** O console expõe aqui o caminho real de
/// onde o título foi carregado; não temos montagem de mídia ligada ao kernel ainda
/// (ver `xbox_formats::vfs`). O valor é um caminho NT bem formado para que
/// `nxGetCurrentXbeNtPath` produza algo coerente, e está registrado como pendência em
/// vez de ser derivado por adivinhação.
pub const DEFAULT_IMAGE_PATH: &str = r"\Device\CdRom0\default.xbe";

/// Ordinal de `LaunchDataPage`.
pub const ORDINAL_LAUNCH_DATA_PAGE: u32 = 164;
/// Ordinal de `PsThreadObjectType`.
pub const ORDINAL_PS_THREAD_OBJECT_TYPE: u32 = 259;
/// Ordinal de `XeImageFileName`.
pub const ORDINAL_XE_IMAGE_FILE_NAME: u32 = 326;
/// Ordinal de `KeTickCount`.
pub const ORDINAL_KE_TICK_COUNT: u32 = 156;

/// Endereço guest de um dado exportado que já tem representação.
///
/// `None` significa que o dado é conhecido pelo catálogo mas **não** foi modelado; o
/// planejamento de imports recusa a imagem nesse caso, em vez de apontar o slot para
/// um endereço qualquer.
pub const fn address(ordinal: u32) -> Option<u32> {
    match ordinal {
        ORDINAL_LAUNCH_DATA_PAGE => Some(DATA_RW_BASE + OFF_LAUNCH_DATA_PAGE),
        ORDINAL_PS_THREAD_OBJECT_TYPE => Some(DATA_RO_BASE + OFF_THREAD_OBJECT_TYPE),
        ORDINAL_XE_IMAGE_FILE_NAME => Some(DATA_RO_BASE + OFF_IMAGE_FILE_NAME),
        ORDINAL_KE_TICK_COUNT => Some(DATA_RO_BASE + OFF_KE_TICK_COUNT),
        _ => None,
    }
}

/// Endereço de envenenamento do campo de índice `indice`.
pub const fn poison(indice: u32) -> u32 {
    POISON_BASE + indice * 4
}

/// Instala a região de dados do kernel na memória guest.
///
/// Escreve o conteúdo com as páginas graváveis e só então aplica as permissões finais,
/// o mesmo padrão que o loader usa para as seções da imagem.
pub fn install(bus: &mut Bus, caminho_da_imagem: &str) -> Result<()> {
    let bytes = caminho_da_imagem.as_bytes();
    if bytes.is_empty() || bytes.len() + 1 > MAX_IMAGE_PATH {
        return Err(CoreError::format(format!(
            "caminho da imagem com {} bytes não cabe em {MAX_IMAGE_PATH}",
            bytes.len()
        ))
        .into());
    }
    if !bytes.is_ascii() {
        // ANSI_STRING é byte a byte; um caminho não-ASCII exigiria decidir a página de
        // código do console, o que não está definido neste ponto do projeto.
        return Err(CoreError::format("caminho da imagem precisa ser ASCII").into());
    }
    let comprimento = u16::try_from(bytes.len())
        .map_err(|_| CoreError::format("caminho da imagem excede ANSI_STRING"))?;

    super::startup::map_fresh(bus, DATA_RO_BASE, DATA_SIZE)?;
    super::startup::map_fresh(bus, DATA_RW_BASE, DATA_SIZE)?;

    // ── PsThreadObjectType ──
    // Procedimentos e DefaultObject envenenados; PoolTag fica zero porque não é
    // ponteiro e inventar a etiqueta do console não acrescentaria nada verificável.
    for campo in 0..6 {
        let endereco = DATA_RO_BASE + OFF_THREAD_OBJECT_TYPE + campo * 4;
        bus.escrever_u32(GuestVirt(endereco), poison(campo))?;
    }
    bus.escrever_u32(
        GuestVirt(DATA_RO_BASE + OFF_THREAD_OBJECT_TYPE + 24),
        0, // PoolTag
    )?;

    // ── XeImageFileName: ANSI_STRING + buffer ──
    let buffer = DATA_RO_BASE + OFF_IMAGE_PATH;
    bus.escrever_u16(GuestVirt(DATA_RO_BASE + OFF_IMAGE_FILE_NAME), comprimento)?;
    bus.escrever_u16(
        GuestVirt(DATA_RO_BASE + OFF_IMAGE_FILE_NAME + 2),
        comprimento + 1, // MaximumLength inclui o NUL
    )?;
    bus.escrever_u32(GuestVirt(DATA_RO_BASE + OFF_IMAGE_FILE_NAME + 4), buffer)?;
    bus.escrever_bytes(GuestVirt(buffer), bytes)?;
    bus.escrever_u8(GuestVirt(buffer + u32::from(comprimento)), 0)?;

    // ── LaunchDataPage ──
    // Nulo é o estado inicial correto: XGetLaunchInfo testa nulo e aloca a página.
    bus.escrever_u32(GuestVirt(DATA_RW_BASE + OFF_LAUNCH_DATA_PAGE), 0)?;

    // Permissões finais. A página RO não pode ficar gravável, senão perderíamos a
    // detecção de escrita indevida em dado de kernel.
    let frame = bus.traduzir(GuestVirt(DATA_RO_BASE), Acesso::Leitura)?;
    bus.mapear(GuestVirt(DATA_RO_BASE), frame, 1, Perm::RO)?;
    Ok(())
}

/// Publica o valor de `KeTickCount` (milissegundos desde o boot) na memória guest.
///
/// A página é somente-leitura **para o guest**: o título só lê o contador, então uma
/// escrita dele seria um defeito a detectar. O kernel escreve pelo endereço físico, que
/// não passa pela permissão da MMU — é o dono do dado, como o hardware seria.
///
/// O valor é de 32 bits e dá a volta em ~49,7 dias, como o `ULONG` do console.
/// O AOT `ClockLinkedV1` publica ao cruzar cada milissegundo, além dos saltos
/// ociosos do scheduler. Snapshots antigos conservam `BoundariesOnly` (ADR-0071).
pub fn publish_tick_count(bus: &mut Bus, milissegundos: u64) -> Result<()> {
    // A região só existe quando a imagem importa algum dado do kernel (`install`). Sem
    // ela ninguém lê o contador, e publicar seria falhar por um dado que não foi pedido.
    let va = GuestVirt(DATA_RO_BASE + OFF_KE_TICK_COUNT);
    if bus.mmu().consultar(va).is_none() {
        return Ok(());
    }
    let frame = bus.traduzir(va, Acesso::Leitura)?;
    // Truncar é o contrato: `ULONG` com volta, não saturação.
    #[allow(clippy::cast_possible_truncation)]
    let valor = milissegundos as u32;
    Ok(bus.escrever_fisico(frame, &valor.to_le_bytes())?)
}

#[cfg(test)]
mod tests {
    use super::{
        ANSI_STRING_SIZE, DATA_RO_BASE, DATA_RW_BASE, DATA_SIZE, OBJECT_TYPE_SIZE,
        OFF_IMAGE_FILE_NAME, OFF_IMAGE_PATH, OFF_KE_TICK_COUNT, OFF_LAUNCH_DATA_PAGE,
        OFF_THREAD_OBJECT_TYPE, ORDINAL_LAUNCH_DATA_PAGE, ORDINAL_PS_THREAD_OBJECT_TYPE,
        ORDINAL_XE_IMAGE_FILE_NAME, POISON_BASE, address, poison,
    };

    #[test]
    fn layout_nao_se_sobrepoe_dentro_da_pagina() {
        let regioes = [
            (OFF_THREAD_OBJECT_TYPE, OBJECT_TYPE_SIZE),
            (OFF_IMAGE_FILE_NAME, ANSI_STRING_SIZE),
            (OFF_KE_TICK_COUNT, 4),
            (OFF_IMAGE_PATH, super::MAX_IMAGE_PATH as u32),
        ];
        for (i, (a_ini, a_len)) in regioes.iter().enumerate() {
            assert!(a_ini + a_len <= DATA_SIZE, "região {i} passa da página");
            for (j, (b_ini, b_len)) in regioes.iter().enumerate() {
                if i != j {
                    assert!(
                        a_ini >= &(b_ini + b_len) || b_ini >= &(a_ini + a_len),
                        "regiões {i} e {j} se sobrepõem"
                    );
                }
            }
        }
    }

    #[test]
    fn enderecos_ficam_na_pagina_de_permissao_correta() {
        // LaunchDataPage é escrito pelo guest: tem de estar na página gravável.
        let launch = address(ORDINAL_LAUNCH_DATA_PAGE).unwrap();
        assert!((DATA_RW_BASE..DATA_RW_BASE + DATA_SIZE).contains(&launch));

        // Os outros dois são só lidos: página somente-leitura.
        for ordinal in [
            ORDINAL_PS_THREAD_OBJECT_TYPE,
            ORDINAL_XE_IMAGE_FILE_NAME,
            super::ORDINAL_KE_TICK_COUNT,
        ] {
            let endereco = address(ordinal).unwrap();
            assert!(
                (DATA_RO_BASE..DATA_RO_BASE + DATA_SIZE).contains(&endereco),
                "ordinal {ordinal} fora da página somente-leitura"
            );
        }
    }

    #[test]
    fn dado_nao_modelado_nao_recebe_endereco() {
        // Ordinais de dados que o catálogo conhece mas que ninguém modelou ainda.
        for ordinal in [16, 102, 154, 322, 324] {
            assert_eq!(address(ordinal), None, "ordinal {ordinal} ganhou endereço");
        }
        assert_eq!(address(0), None);
        assert_eq!(address(u32::MAX), None);
    }

    #[test]
    fn envenenamento_fica_fora_das_paginas_instaladas() {
        for campo in 0..7 {
            let p = poison(campo);
            assert!(p >= POISON_BASE);
            assert!(
                !(DATA_RO_BASE..DATA_RO_BASE + DATA_SIZE).contains(&p)
                    && !(DATA_RW_BASE..DATA_RW_BASE + DATA_SIZE).contains(&p),
                "envenenamento {p:#X} cairia numa página instalada"
            );
        }
        // Campos distintos têm endereços distintos: a falta diz qual campo foi usado.
        assert_ne!(poison(0), poison(1));
    }

    #[test]
    fn offsets_sao_alinhados() {
        for off in [
            OFF_THREAD_OBJECT_TYPE,
            OFF_IMAGE_FILE_NAME,
            OFF_IMAGE_PATH,
            OFF_KE_TICK_COUNT,
            OFF_LAUNCH_DATA_PAGE,
        ] {
            assert_eq!(off % 4, 0, "offset {off:#X} desalinhado");
        }
    }
}
