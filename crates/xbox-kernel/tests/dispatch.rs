//! Dispatcher de serviços de kernel: ABI, diagnóstico e efeito verificável.
//!
//! A regra que estes testes protegem é a do guia do alpha: ordinal sem serviço devolve
//! **erro identificável**, nunca sucesso genérico. E o serviço implementado tem de ter
//! efeito observável na memória guest, não só um `EAX`.

use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_formats::fixtures::xbe_com_imports;
use xbox_kernel::dispatch::{Dispatcher, Erro, IMPLEMENTADOS, ORDINAL_RTL_INIT_ANSI_STRING};
use xbox_kernel::dispatch::{
    INITIAL_THREAD, MAX_MUTANTS, ORDINAL_NT_CREATE_MUTANT, STATUS_ACCESS_VIOLATION,
    STATUS_INSUFFICIENT_RESOURCES, STATUS_SUCCESS,
};
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

fn argumento(bus: &mut Bus, regs: &[u32; 8], index: u32, value: u32) {
    bus.escrever_u32(GuestVirt(regs[4] + 4 + index * 4), value)
        .unwrap();
}

fn salvo(dispatcher: &Dispatcher) -> Vec<u8> {
    let mut out = SnapshotWriter::new();
    dispatcher.save(&mut out);
    out.finish()
}

#[test]
fn nt_create_mutant_cria_handles_distintos_e_registra_proprietario_guest() {
    let (mut bus, pc, regs) = chamada(ORDINAL_NT_CREATE_MUTANT, &[0x12010, 0, 0]);
    let mut d = Dispatcher::new();
    assert_eq!(d.atender(&mut bus, pc, &regs).unwrap().eax, STATUS_SUCCESS);
    let first = bus.ler_u32(GuestVirt(0x12010)).unwrap();
    assert_eq!(first, 4);
    assert_eq!(d.mutant(first).unwrap().owner, None);
    assert_eq!(d.mutant(first).unwrap().recursion, 0);

    argumento(&mut bus, &regs, 2, 0x101);
    d.atender(&mut bus, pc, &regs).unwrap();
    let second = bus.ler_u32(GuestVirt(0x12010)).unwrap();
    assert_eq!(second, 8);
    assert_eq!(d.mutant(second).unwrap().owner, Some(INITIAL_THREAD));
    assert_eq!(d.mutant(second).unwrap().recursion, 1);
    // BOOLEAN ocupa uma palavra na pilha, mas apenas os oito bits baixos valem.
    argumento(&mut bus, &regs, 2, 0x100);
    d.atender(&mut bus, pc, &regs).unwrap();
    assert_eq!(d.mutant(12).unwrap().owner, None);
    assert!(d.mutant(0).is_none());
    assert_eq!(d.atendidas(), 3);
}

#[test]
fn atributos_vazios_usam_layout_xbox_de_doze_bytes() {
    let (mut bus, pc, regs) = chamada(192, &[0x12010, 0x12020, 0]);
    bus.escrever_bytes(GuestVirt(0x12020), &[0; 12]).unwrap();
    let mut d = Dispatcher::new();
    assert_eq!(d.atender(&mut bus, pc, &regs).unwrap().eax, STATUS_SUCCESS);
    assert!(d.mutant(bus.ler_u32(GuestVirt(0x12010)).unwrap()).is_some());
}

#[test]
fn atributos_nao_modelados_recusam_sem_criar_objeto_ou_escrever_saida() {
    let (mut bus, pc, regs) = chamada(192, &[0x12010, 0x12020, 0]);
    let mut d = Dispatcher::new();
    for field in [0, 4, 8] {
        bus.escrever_bytes(GuestVirt(0x12020), &[0; 12]).unwrap();
        bus.escrever_u32(GuestVirt(0x12020 + field), 1).unwrap();
        bus.escrever_u32(GuestVirt(0x12010), 0xDEAD_BEEF).unwrap();
        let error = d.atender(&mut bus, pc, &regs).unwrap_err();
        assert!(matches!(
            error,
            Erro::VarianteNaoImplementada {
                ordinal: 192,
                chamador: RETORNO,
                ..
            }
        ));
        assert!(error.to_string().contains("OBJECT_ATTRIBUTES"));
        assert_eq!(bus.ler_u32(GuestVirt(0x12010)).unwrap(), 0xDEAD_BEEF);
        assert!(d.mutant(4).is_none());
    }
    assert_eq!(d.recusadas(), 3);
    argumento(&mut bus, &regs, 1, 0);
    d.atender(&mut bus, pc, &regs).unwrap();
    assert_eq!(bus.ler_u32(GuestVirt(0x12010)).unwrap(), 4);
}

#[test]
fn ponteiro_de_saida_invalido_nao_deixa_store_parcial_nem_consume_handle() {
    let (mut bus, pc, regs) = chamada(192, &[0x12010, 0, 0]);
    let mut d = Dispatcher::new();
    bus.escrever_u16(GuestVirt(0x12FFE), 0xBEEF).unwrap();
    // Nulo, wrap, página ausente, página RX e store atravessando página ausente.
    for pointer in [0, u32::MAX - 1, 0x5000_0000, 0x11000, 0x12FFE] {
        argumento(&mut bus, &regs, 0, pointer);
        assert_eq!(
            d.atender(&mut bus, pc, &regs).unwrap().eax,
            STATUS_ACCESS_VIOLATION
        );
        assert!(d.mutant(4).is_none());
        assert_eq!(bus.ler_u16(GuestVirt(0x12FFE)).unwrap(), 0xBEEF);
    }
    argumento(&mut bus, &regs, 0, 0x12011); // Desalinhamento em RAM é permitido.
    d.atender(&mut bus, pc, &regs).unwrap();
    assert_eq!(bus.ler_u32(GuestVirt(0x12011)).unwrap(), 4);
}

#[test]
fn ponteiro_mmio_nao_recebe_handle_descartado_pelo_bus() {
    use xbox_mem::{GuestPhys, Perm};
    let (mut bus, pc, regs) = chamada(192, &[0x5000_0000, 0, 0]);
    bus.mapear(GuestVirt(0x5000_0000), GuestPhys(0xFD00_0000), 1, Perm::RW)
        .unwrap();
    let mut d = Dispatcher::new();
    assert_eq!(
        d.atender(&mut bus, pc, &regs).unwrap().eax,
        STATUS_ACCESS_VIOLATION
    );
    assert!(d.mutant(4).is_none());
}

#[test]
fn atributos_ilegiveis_devolvem_status_sem_modificar_saida() {
    let (mut bus, pc, regs) = chamada(192, &[0x12010, 0x5000_0000, 0]);
    bus.escrever_u32(GuestVirt(0x12010), 0xDEAD_BEEF).unwrap();
    let mut d = Dispatcher::new();
    assert_eq!(
        d.atender(&mut bus, pc, &regs).unwrap().eax,
        STATUS_ACCESS_VIOLATION
    );
    assert_eq!(bus.ler_u32(GuestVirt(0x12010)).unwrap(), 0xDEAD_BEEF);
    assert!(d.mutant(4).is_none());
}

#[test]
fn limite_de_objetos_devolve_ntstatus_e_preserva_saida() {
    let (mut bus, pc, regs) = chamada(192, &[0x12010, 0, 0]);
    let mut d = Dispatcher::new();
    for i in 0..MAX_MUTANTS {
        assert_eq!(d.atender(&mut bus, pc, &regs).unwrap().eax, STATUS_SUCCESS);
        assert_eq!(bus.ler_u32(GuestVirt(0x12010)).unwrap(), (i as u32 + 1) * 4);
    }
    bus.escrever_u32(GuestVirt(0x12010), 0xDEAD_BEEF).unwrap();
    assert_eq!(
        d.atender(&mut bus, pc, &regs).unwrap().eax,
        STATUS_INSUFFICIENT_RESOURCES
    );
    assert_eq!(bus.ler_u32(GuestVirt(0x12010)).unwrap(), 0xDEAD_BEEF);
}

#[test]
fn snapshot_de_kernel_preserva_objetos_propriedade_proximo_handle_e_contadores() {
    let (mut bus, pc, regs) = chamada(192, &[0x12010, 0, 1]);
    let mut d = Dispatcher::new();
    d.atender(&mut bus, pc, &regs).unwrap();
    argumento(&mut bus, &regs, 1, 0x12020);
    bus.escrever_u32(GuestVirt(0x12020), 1).unwrap();
    assert!(matches!(
        d.atender(&mut bus, pc, &regs),
        Err(Erro::VarianteNaoImplementada { .. })
    ));
    argumento(&mut bus, &regs, 1, 0);
    let saved = salvo(&d);
    assert_eq!(saved, salvo(&d));
    d.atender(&mut bus, pc, &regs).unwrap();
    let complete = salvo(&d);
    let mut reader = SnapshotReader::new(&saved).unwrap();
    d.load(&mut reader).unwrap();
    assert_eq!(reader.remaining(), 0);
    assert_eq!(d.atendidas(), 1);
    assert_eq!(d.recusadas(), 1);
    assert_eq!(d.mutant(4).unwrap().owner, Some(INITIAL_THREAD));
    assert!(d.mutant(8).is_none());
    d.atender(&mut bus, pc, &regs).unwrap();
    assert_eq!(bus.ler_u32(GuestVirt(0x12010)).unwrap(), 8);
    assert_eq!(salvo(&d), complete);
}

#[test]
fn snapshot_malformado_nao_injeta_handles_ou_proprietarios_invalidos() {
    use xbox_core::error::SnapshotError;
    for (next, count, rows) in [
        (3, 0, vec![]),
        (4, MAX_MUTANTS as u32 + 1, vec![]),
        (8, 1, vec![(0, 0, 0)]),
        (8, 1, vec![(8, 0, 0)]),
        (8, 1, vec![(5, 0, 0)]),
        (12, 2, vec![(4, 0, 0), (4, 0, 0)]),
        (12, 2, vec![(8, 0, 0), (4, 0, 0)]),
        (8, 1, vec![(4, 2, 1)]),
        (8, 1, vec![(4, 0, 1)]),
        (8, 1, vec![(4, 1, 0)]),
    ] {
        let mut out = SnapshotWriter::new();
        out.section("KRNL", 1);
        out.u64(0);
        out.u64(0);
        out.section("KMUT", 1);
        out.u32(next);
        out.u32(count);
        for (handle, owner, recursion) in rows {
            out.u32(handle);
            out.u32(owner);
            out.u32(recursion);
        }
        let bytes = out.finish();
        let mut d = Dispatcher::new();
        assert!(matches!(
            d.load(&mut SnapshotReader::new(&bytes).unwrap()),
            Err(SnapshotError::BadValue { .. })
        ));
        assert!(d.mutant(4).is_none());
    }
    let valid = salvo(&Dispatcher::new());
    assert!(
        Dispatcher::new()
            .load(&mut SnapshotReader::new(&valid[..valid.len() - 1]).unwrap())
            .is_err()
    );
}
