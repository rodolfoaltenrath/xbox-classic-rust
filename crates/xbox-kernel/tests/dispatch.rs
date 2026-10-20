//! Dispatcher de serviços de kernel: ABI, diagnóstico e efeito verificável.
//!
//! A regra que estes testes protegem é a do guia do alpha: ordinal sem serviço devolve
//! **erro identificável**, nunca sucesso genérico. E o serviço implementado tem de ter
//! efeito observável na memória guest, não só um `EAX`.

use xbox_formats::fixtures::xbe_com_imports;
use xbox_kernel::dispatch::{Dispatcher, Erro, IMPLEMENTADOS, ORDINAL_RTL_INIT_ANSI_STRING};
use xbox_kernel::imports::{self, Kind};
use xbox_kernel::loader::{Image, STACK_BASE, STACK_SIZE};
use xbox_mem::{Bus, GuestVirt};

/// Monta uma chamada de kernel na pilha, como o Rust gerado deixaria ao sair.
///
/// Devolve `(bus, pc do trampolim, registradores)`. O endereço de retorno é um valor
/// reconhecível para conferir o diagnóstico de "chamado de".
/// Endereço de retorno das chamadas de teste, reconhecível no diagnóstico.
const RETORNO: u32 = 0x0001_1234;

fn chamada(ordinal: u32, argumentos: &[u32]) -> (Bus, u32, [u32; 8]) {
    let bytes = xbe_com_imports(&[0xC3], &[ordinal]).unwrap();
    let startup = Image::prepare_aot(&bytes).unwrap();
    let mut bus = startup.image.bus;

    let esp = STACK_BASE + STACK_SIZE - 0x100 - argumentos.len() as u32 * 4;
    bus.escrever_u32(GuestVirt(esp), RETORNO).unwrap();
    for (i, valor) in argumentos.iter().enumerate() {
        bus.escrever_u32(GuestVirt(esp + 4 + i as u32 * 4), *valor)
            .unwrap();
    }
    let mut regs = [0u32; 8];
    regs[4] = esp;
    (bus, imports::trampoline(ordinal), regs)
}

#[test]
fn ordinal_sem_servico_recusa_com_nome_ordinal_chamador_e_argumentos() {
    // `MmAllocateContiguousMemory` (165) é stdcall/4: um argumento declarado.
    let (mut bus, pc, regs) = chamada(165, &[0x1000]);
    let mut d = Dispatcher::new();

    let erro = d.atender(&mut bus, pc, &regs).unwrap_err();
    let texto = erro.to_string();
    assert!(
        matches!(erro, Erro::NaoImplementado { .. }),
        "veio {erro:?}"
    );
    assert!(texto.contains("MmAllocateContiguousMemory"), "{texto}");
    assert!(texto.contains("165"), "{texto}");
    assert!(texto.contains("0x00011234"), "falta o chamador: {texto}");
    assert!(texto.contains("0x00001000"), "falta o argumento: {texto}");

    assert_eq!(d.recusadas(), 1);
    assert_eq!(d.atendidas(), 0);
}

#[test]
fn recusa_nao_altera_a_cpu_nem_a_memoria_do_guest() {
    // O contrato de retomada depende disso: um serviço faltante pode ser implementado
    // e a execução continuar do mesmo ponto.
    let (mut bus, pc, regs) = chamada(165, &[0x1000]);
    let antes = bus.ler_u32(GuestVirt(regs[4])).unwrap();
    let regs_antes = regs;

    let mut d = Dispatcher::new();
    assert!(d.atender(&mut bus, pc, &regs).is_err());

    assert_eq!(
        regs, regs_antes,
        "o dispatcher não pode mexer em registradores"
    );
    assert_eq!(bus.ler_u32(GuestVirt(regs[4])).unwrap(), antes);
}

#[test]
fn cdecl_mostra_as_primeiras_palavras_e_diz_que_nao_declara_quantidade() {
    // `DbgPrint` (8) é cdecl: a quantidade de argumentos não está na assinatura.
    let (mut bus, pc, regs) = chamada(8, &[0x1111_1111, 0x2222_2222]);
    let mut d = Dispatcher::new();

    let texto = d.atender(&mut bus, pc, &regs).unwrap_err().to_string();
    assert!(texto.contains("DbgPrint"), "{texto}");
    assert!(texto.contains("0x11111111"), "{texto}");
    assert!(
        texto.contains("cdecl"),
        "deveria avisar que é variádico: {texto}"
    );
}

#[test]
fn servico_sem_argumentos_descreve_nenhum() {
    // `KeGetCurrentIrql` (103) é stdcall/0.
    let (mut bus, pc, regs) = chamada(103, &[]);
    let mut d = Dispatcher::new();
    let texto = d.atender(&mut bus, pc, &regs).unwrap_err().to_string();
    assert!(texto.contains("nenhum"), "{texto}");
}

#[test]
fn rtl_init_ansi_string_preenche_os_tres_campos_do_destino() {
    // Efeito inteiro em memória guest, verificável lendo o destino.
    let destino = 0x0001_2000u32;
    let origem = 0x0001_2100u32;
    let (mut bus, pc, regs) = chamada(ORDINAL_RTL_INIT_ANSI_STRING, &[destino, origem]);
    bus.escrever_bytes(GuestVirt(origem), b"D:\\default.xbe\0")
        .unwrap();

    let mut d = Dispatcher::new();
    let atendimento = d
        .atender(&mut bus, pc, &regs)
        .expect("serviço implementado");

    assert_eq!(bus.ler_u16(GuestVirt(destino)).unwrap(), 14, "Length");
    assert_eq!(
        bus.ler_u16(GuestVirt(destino + 2)).unwrap(),
        15,
        "MaximumLength inclui o NUL"
    );
    assert_eq!(
        bus.ler_u32(GuestVirt(destino + 4)).unwrap(),
        origem,
        "Buffer aponta para a origem, sem copiar"
    );
    // A função é VOID: zero é convenção nossa, documentada.
    assert_eq!(atendimento.eax, 0);
    assert_eq!(d.atendidas(), 1);
    assert_eq!(d.recusadas(), 0);
}

#[test]
fn rtl_init_ansi_string_com_origem_nula_zera_o_destino() {
    let destino = 0x0001_2000u32;
    let (mut bus, pc, regs) = chamada(ORDINAL_RTL_INIT_ANSI_STRING, &[destino, 0]);
    // Lixo no destino, para provar que os campos são escritos.
    bus.escrever_u32(GuestVirt(destino), 0xDEAD_BEEF).unwrap();
    bus.escrever_u32(GuestVirt(destino + 4), 0xDEAD_BEEF)
        .unwrap();

    let mut d = Dispatcher::new();
    d.atender(&mut bus, pc, &regs).unwrap();

    assert_eq!(bus.ler_u16(GuestVirt(destino)).unwrap(), 0);
    assert_eq!(bus.ler_u16(GuestVirt(destino + 2)).unwrap(), 0);
    assert_eq!(bus.ler_u32(GuestVirt(destino + 4)).unwrap(), 0);
}

#[test]
fn rtl_init_ansi_string_recusa_destino_nulo_em_vez_de_escrever_no_endereco_zero() {
    let (mut bus, pc, regs) = chamada(ORDINAL_RTL_INIT_ANSI_STRING, &[0, 0x0001_2100]);
    let mut d = Dispatcher::new();
    let erro = d.atender(&mut bus, pc, &regs).unwrap_err();
    assert!(
        matches!(erro, Erro::ArgumentoInvalido { .. }),
        "veio {erro:?}"
    );
    assert!(erro.to_string().contains("DestinationString nulo"));
}

#[test]
fn rtl_init_ansi_string_recusa_origem_ilegivel() {
    // Origem em página não mapeada: erro de argumento, não pânico nem leitura do host.
    let (mut bus, pc, regs) = chamada(ORDINAL_RTL_INIT_ANSI_STRING, &[0x0001_2000, 0x5000_0000]);
    let mut d = Dispatcher::new();
    let erro = d.atender(&mut bus, pc, &regs).unwrap_err();
    assert!(
        erro.to_string().contains("ilegível") || matches!(erro, Erro::Fronteira(_)),
        "veio {erro:?}"
    );
}

#[test]
fn endereco_que_nao_e_trampolim_e_recusado_antes_de_qualquer_efeito() {
    let (mut bus, _pc, regs) = chamada(ORDINAL_RTL_INIT_ANSI_STRING, &[0x0001_2000, 0]);
    let mut d = Dispatcher::new();
    // Endereço no meio da imagem, não na página de trampolins.
    let erro = d.atender(&mut bus, 0x0001_1000, &regs).unwrap_err();
    assert!(matches!(erro, Erro::Fronteira(_)), "veio {erro:?}");
    assert_eq!(d.atendidas(), 0);
    assert_eq!(d.recusadas(), 0, "nem atendida nem recusada: não é chamada");
}

#[test]
fn a_lista_de_implementados_casa_exatamente_com_o_que_o_dispatcher_atende() {
    // Protege a fonte única: um serviço novo tem de entrar na lista, e a lista não pode
    // anunciar serviço que o dispatcher recusa.
    for &ordinal in IMPLEMENTADOS {
        assert!(
            imports::servico_implementado(ordinal),
            "ordinal {ordinal} está em IMPLEMENTADOS e o registro diz que não"
        );
        let export = imports::lookup(ordinal).expect("ordinal precisa estar no catálogo");
        assert!(
            matches!(export.kind, Kind::Function(_)),
            "dado não é serviço"
        );
    }

    // Amostra de funções importadas pelo alvo que ainda não têm serviço.
    for ordinal in [8u32, 103, 165, 184, 255, 277] {
        if IMPLEMENTADOS.contains(&ordinal) {
            continue;
        }
        assert!(!imports::servico_implementado(ordinal));
        let (mut bus, pc, regs) = chamada(ordinal, &[]);
        let erro = bus
            .ler_u32(GuestVirt(regs[4]))
            .map(|_| Dispatcher::new().atender(&mut bus, pc, &regs).unwrap_err())
            .unwrap();
        assert!(
            matches!(erro, Erro::NaoImplementado { .. }),
            "ordinal {ordinal} deveria recusar: {erro:?}"
        );
    }
}
