use xbox_formats::{
    fixtures::{xbe_com_codigo, xbe_com_imports},
    xbe::SectionFlags,
};
use xbox_kernel::{
    imports::{self, Abi, Call, Kind},
    loader::{Image, RETURN_SENTINEL, STACK_BASE, STACK_SIZE, TIB_BASE},
};
use xbox_mem::{Acesso, GuestVirt};

fn put(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn tls_image() -> Vec<u8> {
    let mut bytes = xbe_com_codigo(&[0xC3]).unwrap();
    put(&mut bytes, 0x12C, 0x103C0);
    for (i, value) in [0x12020u32, 0x12024, 0x12010, 0, 13, 0x00300000]
        .into_iter()
        .enumerate()
    {
        put(&mut bytes, 0x3C0 + i * 4, value);
    }
    put(&mut bytes, 0x2020, 0x44332211);
    bytes
}

#[test]
fn tls_nxdk_tem_indice_negativo_slot_auto_referente_alinhamento_e_zerofill() {
    let startup = Image::prepare_aot(&tls_image()).unwrap();
    let bus = &startup.image.bus;
    let top = STACK_BASE + STACK_SIZE;
    let slot = top - 36; // align16(4 + 13) + 4
    let data = slot + 4;
    assert_eq!(startup.fs_base, TIB_BASE);
    assert_eq!(bus.ler_u32(GuestVirt(TIB_BASE + 4)).unwrap(), top);
    assert_eq!(bus.ler_u32(GuestVirt(TIB_BASE + 8)).unwrap(), STACK_BASE);
    assert_eq!(bus.ler_u32(GuestVirt(TIB_BASE)).unwrap(), u32::MAX);
    let index = bus.ler_u32(GuestVirt(0x12010)).unwrap();
    assert_eq!(top.wrapping_add(index.wrapping_mul(4)), slot);
    assert_eq!(bus.ler_u32(GuestVirt(slot)).unwrap(), data);
    assert_eq!(data % 16, 0);
    assert_eq!(bus.ler_u32(GuestVirt(data)).unwrap(), 0x44332211);
    let mut zeros = [0xFF; 28];
    bus.ler_bytes(GuestVirt(data + 4), &mut zeros).unwrap();
    assert_eq!(zeros, [0; 28]);
    assert_eq!(startup.esp, slot - 4);
    assert_eq!(
        bus.ler_u32(GuestVirt(startup.esp)).unwrap(),
        RETURN_SENTINEL
    );
}

#[test]
fn importa_funcoes_em_tabela_ro_sem_deixar_codigo_gravavel_ou_instalar_outros_slots() {
    let mut bytes = xbe_com_imports(&[0xC3], &[8, 165, 21, 8]).unwrap();
    put(&mut bytes, 0x338, SectionFlags::PRELOAD);
    let startup = Image::prepare_aot(&bytes).unwrap();
    let bus = &startup.image.bus;
    for (i, ordinal) in [8u32, 165, 21, 8].into_iter().enumerate() {
        let target = bus.ler_u32(GuestVirt(0x12000 + i as u32 * 4)).unwrap();
        assert_eq!(target, imports::trampoline(ordinal));
        assert_eq!(imports::entry(bus, target).unwrap().ordinal, ordinal);
        assert!(bus.traduzir(GuestVirt(target), Acesso::Escrita).is_err());
    }
    assert_eq!(bus.ler_u32(GuestVirt(0x12010)).unwrap(), 0);
    assert!(bus.traduzir(GuestVirt(0x12000), Acesso::Escrita).is_err());
    assert!(imports::entry(bus, imports::trampoline(1)).is_err());
    assert!(imports::entry(bus, imports::trampoline(8) + 1).is_err());
    assert!(imports::entry(bus, u32::MAX).is_err());
}

#[test]
fn fronteira_abi_distingue_pilha_registradores_e_limites_sem_mudar_cpu() {
    let mut startup =
        Image::prepare_aot(&xbe_com_imports(&[0xC3], &[8, 165, 21, 250]).unwrap()).unwrap();
    let bus = &mut startup.image.bus;
    let mut regs = [0; 8];
    regs[1] = 0x1111;
    regs[2] = 0x2222;
    regs[4] = startup.esp - 16;
    bus.escrever_u32(GuestVirt(regs[4]), 0x11000).unwrap();
    bus.escrever_u32(GuestVirt(regs[4] + 4), 0x3333).unwrap();
    for (ordinal, cleanup, expected) in [
        (8, 0, 0x3333),
        (165, 4, 0x3333),
        (21, 4, 0x1111),
        (250, 0, 0x1111),
    ] {
        let call = Call::read(bus, imports::trampoline(ordinal), &regs).unwrap();
        assert_eq!(call.return_pc, 0x11000);
        assert_eq!(call.return_esp, regs[4] + 4 + cleanup);
        assert_eq!(call.argument(bus, 0).unwrap(), expected);
        if ordinal == 21 {
            assert_eq!(call.argument(bus, 1).unwrap(), 0x2222);
            assert_eq!(call.argument(bus, 2).unwrap(), 0x3333);
            assert!(call.argument(bus, 3).is_err());
        }
        assert!(call.argument(bus, 64).is_err());
    }
    regs[4] = u32::MAX - 2;
    assert!(Call::read(bus, imports::trampoline(8), &regs).is_err());
    regs[4] = 0;
    assert!(Call::read(bus, imports::trampoline(8), &regs).is_err());
}

#[test]
fn rejeita_dados_desconhecidos_patches_de_codigo_callbacks_e_tls_malformado() {
    for ordinal in [164, 259, 326, 500] {
        assert!(Image::prepare_aot(&xbe_com_imports(&[0xC3], &[ordinal]).unwrap()).is_err());
    }
    for (offset, value) in [
        (0x3C4, 0x12000),
        (0x3C4, u32::MAX),
        (0x3D0, u32::MAX),
        (0x3CC, 0x11000),
        (0x3D4, 0x00600000),
        (0x3D4, 1),
        (0x3C8, 0x11000),
        (0x3C8, 0x7000FFFC),
        (0x3C0, 0x70000000),
    ] {
        let mut bytes = tls_image();
        put(&mut bytes, offset, value);
        assert!(
            Image::prepare_aot(&bytes).is_err(),
            "offset {offset:X}, value {value:X}"
        );
    }
}

#[test]
fn catalogo_tem_371_ordinais_e_abi_independente_para_assinaturas_representativas() {
    assert_eq!((0..512).filter_map(imports::lookup).count(), 371);
    assert_eq!(imports::lookup(8).unwrap().kind, Kind::Function(Abi::Cdecl));
    assert_eq!(
        imports::lookup(165).unwrap().kind,
        Kind::Function(Abi::Stdcall(4))
    );
    assert_eq!(
        imports::lookup(21).unwrap().kind,
        Kind::Function(Abi::Fastcall(12))
    );
    assert_eq!(imports::lookup(326).unwrap().kind, Kind::Data);
    assert!(imports::lookup(0).is_none());
    assert!(imports::lookup(367).is_none());
}

#[test]
fn imagem_nao_pode_ocupar_regioes_reservadas_e_indice_ro_recupera_protecao() {
    for base in [STACK_BASE, TIB_BASE, imports::TRAMPOLINE_BASE] {
        let mut bytes = xbe_com_codigo(&[0xC3]).unwrap();
        put(&mut bytes, 0x10C, base + 4096 - 0x10000);
        put(&mut bytes, 0x33C, base);
        assert!(Image::prepare_aot(&bytes).is_err());
        assert!(
            xbox_kernel::loader::validate_startup_layout(
                &xbox_formats::Xbe::parse(&bytes).unwrap()
            )
            .is_err()
        );
    }
    let mut bytes = tls_image();
    put(&mut bytes, 0x338, SectionFlags::PRELOAD);
    let startup = Image::prepare_aot(&bytes).unwrap();
    assert!(
        startup
            .image
            .bus
            .traduzir(GuestVirt(0x12010), Acesso::Escrita)
            .is_err()
    );
    assert_ne!(startup.image.bus.ler_u32(GuestVirt(0x12010)).unwrap(), 0);
}
