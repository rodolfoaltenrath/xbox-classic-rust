//! Dados exportados do kernel: layout, ponteiros, permissões e conteúdo.
//!
//! Os três dados que o homebrew do nxdk importa têm formas diferentes entre si, e cada
//! uma falharia de maneira própria se fosse preenchida com zeros. Estes testes fixam o
//! que cada forma exige, para que a implementação dos serviços (próxima entrega) não
//! possa afrouxar nenhuma delas sem o teste acusar.

use xbox_formats::fixtures::xbe_com_imports;
use xbox_kernel::imports::{self, Kind};
use xbox_kernel::loader::Image;
use xbox_kernel::loader::data::{
    self, DATA_RO_BASE, DATA_RW_BASE, DEFAULT_IMAGE_PATH, OFF_IMAGE_FILE_NAME,
    OFF_LAUNCH_DATA_PAGE, OFF_THREAD_OBJECT_TYPE, ORDINAL_LAUNCH_DATA_PAGE,
    ORDINAL_PS_THREAD_OBJECT_TYPE, ORDINAL_XE_IMAGE_FILE_NAME,
};
use xbox_mem::{Acesso, GuestVirt};

/// Imagem com os três dados importados e uma função, como o alvo real.
fn imagem_com_os_tres_dados() -> Vec<u8> {
    xbe_com_imports(
        &[0xC3],
        &[
            ORDINAL_LAUNCH_DATA_PAGE,
            ORDINAL_PS_THREAD_OBJECT_TYPE,
            ORDINAL_XE_IMAGE_FILE_NAME,
            8, // DbgPrint, para cobrir função e dado na mesma tabela
        ],
    )
    .unwrap()
}

#[test]
fn os_tres_dados_deixam_de_bloquear_a_preparacao() {
    // Antes desta entrega, `prepare_aot` recusava qualquer dado exportado.
    let startup = Image::prepare_aot(&imagem_com_os_tres_dados()).expect("os três são aceitos");
    assert!(
        startup.image.pages > 0,
        "a imagem precisa ter páginas mapeadas"
    );
}

#[test]
fn slot_do_thunk_recebe_o_endereco_do_simbolo_nao_um_trampolim() {
    // `__declspec(dllimport)` em dado faz o código carregar o slot e desreferenciar a
    // partir do valor. Logo o slot guarda o endereço do símbolo, não de um trampolim.
    let bytes = imagem_com_os_tres_dados();
    let startup = Image::prepare_aot(&bytes).unwrap();
    let bus = &startup.image.bus;
    let thunk = startup.image.kernel_thunk;

    for (index, ordinal) in [
        ORDINAL_LAUNCH_DATA_PAGE,
        ORDINAL_PS_THREAD_OBJECT_TYPE,
        ORDINAL_XE_IMAGE_FILE_NAME,
    ]
    .into_iter()
    .enumerate()
    {
        let slot = bus
            .ler_u32(GuestVirt(thunk + index as u32 * 4))
            .expect("slot legível");
        assert_eq!(
            slot,
            data::address(ordinal).unwrap(),
            "slot do ordinal {ordinal} não aponta para o dado"
        );
        // E não pode coincidir com a página de trampolins.
        assert_ne!(slot, imports::trampoline(ordinal));
    }

    // A função na mesma tabela continua recebendo trampolim.
    let slot_funcao = bus.ler_u32(GuestVirt(thunk + 3 * 4)).unwrap();
    assert_eq!(slot_funcao, imports::trampoline(8));
}

#[test]
fn launch_data_page_e_ponteiro_nulo_e_gravavel_pelo_guest() {
    // `XGetLaunchInfo` testa nulo, aloca a página e **escreve de volta** na variável.
    // Nulo é o estado inicial correto; gravável é requisito, não conveniência.
    let startup = Image::prepare_aot(&imagem_com_os_tres_dados()).unwrap();
    let mut bus = startup.image.bus;
    let endereco = GuestVirt(DATA_RW_BASE + OFF_LAUNCH_DATA_PAGE);

    assert_eq!(bus.ler_u32(endereco).unwrap(), 0, "deve começar nulo");
    bus.traduzir(endereco, Acesso::Escrita)
        .expect("o guest precisa poder escrever LaunchDataPage");
    bus.escrever_u32(endereco, 0x0010_0000).unwrap();
    assert_eq!(bus.ler_u32(endereco).unwrap(), 0x0010_0000);

    // Dado de kernel não é executável.
    assert!(bus.traduzir(endereco, Acesso::Execucao).is_err());
}

#[test]
fn xe_image_file_name_tem_ansi_string_coerente_com_buffer_real() {
    // Com Length = 0 o `strncpy` do SDK não lê Buffer e produz caminho vazio: resultado
    // errado sem erro. Por isso o campo precisa de comprimento e buffer de verdade.
    let startup = Image::prepare_aot(&imagem_com_os_tres_dados()).unwrap();
    let bus = &startup.image.bus;
    let base = DATA_RO_BASE + OFF_IMAGE_FILE_NAME;

    let length = bus.ler_u16(GuestVirt(base)).unwrap();
    let maximum = bus.ler_u16(GuestVirt(base + 2)).unwrap();
    let buffer = bus.ler_u32(GuestVirt(base + 4)).unwrap();

    assert_eq!(usize::from(length), DEFAULT_IMAGE_PATH.len());
    assert_eq!(maximum, length + 1, "MaximumLength inclui o NUL");
    assert_ne!(
        buffer, 0,
        "Buffer nulo faria strncpy produzir caminho vazio"
    );

    // O buffer tem de estar dentro da região instalada e conter o caminho.
    assert!((DATA_RO_BASE..DATA_RO_BASE + data::DATA_SIZE).contains(&buffer));
    let mut lido = vec![0u8; usize::from(length)];
    bus.ler_bytes(GuestVirt(buffer), &mut lido).unwrap();
    assert_eq!(
        String::from_utf8(lido).unwrap(),
        DEFAULT_IMAGE_PATH,
        "conteúdo do caminho diverge"
    );
    // E termina em NUL, porque `nxGetCurrentXbeNtPath` escreve a partir de Length.
    assert_eq!(
        bus.ler_u8(GuestVirt(buffer + u32::from(length))).unwrap(),
        0
    );
}

#[test]
fn ps_thread_object_type_tem_procedimentos_envenenados_e_nao_zerados() {
    // Zero transformaria uso indevido em salto para o endereço 0. Envenenado, qualquer
    // uso falha na hora e o endereço da falta diz qual campo foi seguido.
    let startup = Image::prepare_aot(&imagem_com_os_tres_dados()).unwrap();
    let bus = &startup.image.bus;
    let base = DATA_RO_BASE + OFF_THREAD_OBJECT_TYPE;

    let mut vistos = Vec::new();
    for campo in 0..6 {
        let valor = bus.ler_u32(GuestVirt(base + campo * 4)).unwrap();
        assert_ne!(valor, 0, "campo {campo} ficou zerado");
        assert_eq!(valor, data::poison(campo));
        assert!(
            bus.mmu().consultar(GuestVirt(valor)).is_none(),
            "envenenamento do campo {campo} está mapeado e não falharia"
        );
        vistos.push(valor);
    }
    vistos.dedup();
    assert_eq!(
        vistos.len(),
        6,
        "campos distintos precisam de endereços distintos"
    );

    // PoolTag não é ponteiro: fica zero em vez de etiqueta inventada.
    assert_eq!(bus.ler_u32(GuestVirt(base + 24)).unwrap(), 0);
}

#[test]
fn pagina_de_dados_somente_leitura_recusa_escrita_do_guest() {
    let startup = Image::prepare_aot(&imagem_com_os_tres_dados()).unwrap();
    let mut bus = startup.image.bus;

    for offset in [
        OFF_THREAD_OBJECT_TYPE,
        OFF_IMAGE_FILE_NAME,
        data::OFF_IMAGE_PATH,
    ] {
        let endereco = GuestVirt(DATA_RO_BASE + offset);
        assert!(
            bus.ler_u32(endereco).is_ok(),
            "{endereco} deveria ser legível"
        );
        assert!(
            bus.traduzir(endereco, Acesso::Escrita).is_err(),
            "{endereco} não pode ser gravável"
        );
        assert!(
            bus.escrever_u32(endereco, 0xDEAD_BEEF).is_err(),
            "escrita em {endereco} foi aceita"
        );
        assert!(bus.traduzir(endereco, Acesso::Execucao).is_err());
    }
}

#[test]
fn regiao_nao_e_instalada_quando_a_imagem_nao_importa_dado() {
    // Só funções: as páginas de dados não devem ser alocadas nem mapeadas.
    let bytes = xbe_com_imports(&[0xC3], &[8]).unwrap();
    let startup = Image::prepare_aot(&bytes).unwrap();
    let bus = &startup.image.bus;

    assert!(bus.mmu().consultar(GuestVirt(DATA_RO_BASE)).is_none());
    assert!(bus.mmu().consultar(GuestVirt(DATA_RW_BASE)).is_none());
    // O trampolim da função continua instalado.
    assert!(
        bus.mmu()
            .consultar(GuestVirt(imports::trampoline(8)))
            .is_some()
    );
}

#[test]
fn dado_conhecido_mas_nao_modelado_continua_recusado() {
    // Ter nome e assinatura no catálogo não é implementar: `ExEventObjectType` (16) é
    // dado conhecido e sem representação, e a imagem precisa ser recusada com o nome.
    let bytes = xbe_com_imports(&[0xC3], &[16]).unwrap();
    let erro = Image::prepare_aot(&bytes).unwrap_err().to_string();
    assert!(
        erro.contains("ExEventObjectType") && erro.contains("16"),
        "mensagem não identifica o dado: {erro}"
    );
}

#[test]
fn imagem_que_ocupa_a_regiao_de_dados_e_recusada() {
    // A reserva vale mesmo para imagem sem imports, para que a validação de layout não
    // dependa de quais imports existem.
    let mut bytes = xbe_com_imports(&[0xC3], &[8]).unwrap();
    // Aponta a primeira seção para dentro da página de dados do kernel.
    bytes[0x300 + 0x04..0x300 + 0x08].copy_from_slice(&DATA_RO_BASE.to_le_bytes());
    let erro = Image::prepare_aot(&bytes).unwrap_err().to_string();
    assert!(
        erro.contains("reservada") || erro.contains("fora da imagem"),
        "mensagem inesperada: {erro}"
    );
}

#[test]
fn todo_dado_do_catalogo_sem_endereco_tem_kind_data() {
    // Coerência entre catálogo e representação: nenhum ordinal pode ter endereço de
    // dado sem estar marcado como dado no catálogo.
    for ordinal in 0..400u32 {
        if let Some(_endereco) = data::address(ordinal) {
            let export = imports::lookup(ordinal)
                .unwrap_or_else(|| panic!("ordinal {ordinal} tem endereço e não está no catálogo"));
            assert_eq!(
                export.kind,
                Kind::Data,
                "ordinal {ordinal} tem endereço de dado mas não é Kind::Data"
            );
        }
    }
}
