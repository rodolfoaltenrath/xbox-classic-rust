use xbox_formats::fixtures::xbe_com_imports;
use xbox_kernel::dispatch::{Dispatcher, Erro};
use xbox_kernel::imports;
use xbox_kernel::loader::{Image, STACK_BASE, STACK_SIZE};
use xbox_mem::{Acesso, Bus, GuestPhys, GuestVirt, Perm};

const TARGET: u32 = 0x12100;

fn setup() -> (Bus, Dispatcher, [u32; 8]) {
    let image = xbe_com_imports(&[0xC3], &[54]).unwrap();
    let mut bus = Image::prepare_aot(&image).unwrap().image.bus;
    let esp = STACK_BASE + STACK_SIZE - 0x100;
    bus.escrever_u32(GuestVirt(esp), 0x11000).unwrap();
    let mut regs = [0; 8];
    regs[4] = esp;
    (bus, Dispatcher::new(), regs)
}

fn call(
    bus: &mut Bus,
    d: &mut Dispatcher,
    regs: &[u32; 8],
    target: u32,
    value: u32,
) -> Result<u32, Erro> {
    let mut args = *regs;
    args[1] = target;
    args[2] = value;
    d.atender(bus, imports::trampoline(54), &args)
        .map(|r| r.eax)
}

#[test]
fn exchange_devolve_bits_anteriores_e_grava_novos_preservando_guardas() {
    let (mut bus, mut d, regs) = setup();
    for offset in 0..4 {
        let target = TARGET + offset;
        for old in [0, 1, u32::MAX, 0x8000_0000, 0x1234_5678] {
            for value in [0, u32::MAX, 0x89AB_CDEF] {
                bus.escrever_bytes(GuestVirt(target - 1), &[0xAA; 6])
                    .unwrap();
                bus.escrever_u32(GuestVirt(target), old).unwrap();
                assert_eq!(call(&mut bus, &mut d, &regs, target, value).unwrap(), old);
                assert_eq!(bus.ler_u32(GuestVirt(target)).unwrap(), value);
                assert_eq!(bus.ler_u8(GuestVirt(target - 1)).unwrap(), 0xAA);
                assert_eq!(bus.ler_u8(GuestVirt(target + 4)).unwrap(), 0xAA);
            }
        }
    }
    assert_eq!((d.atendidas(), d.recusadas()), (60, 0));
}

#[test]
fn argumentos_sao_ecx_edx_e_palavras_na_pilha_nao_substituem_fastcall() {
    let (mut bus, mut d, mut regs) = setup();
    regs[1] = TARGET;
    regs[2] = 0xAABBCCDD;
    bus.escrever_u32(GuestVirt(TARGET), 0x8000_0000).unwrap();
    bus.escrever_u32(GuestVirt(regs[4] + 4), 0).unwrap();
    bus.escrever_u32(GuestVirt(regs[4] + 8), 0xDEADBEEF)
        .unwrap();
    let before = regs;
    assert_eq!(
        d.atender(&mut bus, imports::trampoline(54), &regs)
            .unwrap()
            .eax,
        0x8000_0000
    );
    assert_eq!(regs, before);
    assert_eq!(bus.ler_u32(GuestVirt(TARGET)).unwrap(), 0xAABBCCDD);
    assert_eq!(bus.ler_u32(GuestVirt(regs[4] + 4)).unwrap(), 0);
    assert_eq!(bus.ler_u32(GuestVirt(regs[4] + 8)).unwrap(), 0xDEADBEEF);
}

#[test]
fn ponteiros_invalidos_mmio_ou_sem_permissao_recusam_antes_da_leitura() {
    let (mut bus, mut d, regs) = setup();
    bus.mapear(GuestVirt(0x5000_0000), GuestPhys(0xFD00_0000), 1, Perm::RW)
        .unwrap();
    bus.escrever_u16(GuestVirt(0x12FFE), 0xBEEF).unwrap();
    for target in [0, u32::MAX - 1, 0x6000_0000, 0x11000, 0x12FFE, 0x5000_0000] {
        assert!(matches!(
            call(&mut bus, &mut d, &regs, target, 1),
            Err(Erro::ArgumentoInvalido { .. })
        ));
        assert_eq!(bus.ler_u16(GuestVirt(0x12FFE)).unwrap(), 0xBEEF);
    }
    assert_eq!(
        bus.telemetria().total_de_acessos(),
        0,
        "MMIO não deve ser lido nem escrito"
    );
    let page = bus.traduzir(GuestVirt(0x12000), Acesso::Leitura).unwrap();
    bus.escrever_u32(GuestVirt(TARGET), 0xABCD).unwrap();
    for perm in [Perm::RO, Perm(Perm::ESCREVER)] {
        // R somente, depois W sem R.
        bus.mapear(GuestVirt(0x12000), page, 1, perm).unwrap();
        assert!(matches!(
            call(&mut bus, &mut d, &regs, TARGET, 0xABCD),
            Err(Erro::ArgumentoInvalido { .. })
        ));
    }
    bus.mapear(GuestVirt(0x12000), page, 1, Perm::RW).unwrap();
    assert_eq!(bus.ler_u32(GuestVirt(TARGET)).unwrap(), 0xABCD);
    assert_eq!(d.atendidas(), 0);
}

#[test]
fn exchange_cruza_paginas_somente_quando_toda_a_faixa_e_ram_rw() {
    let (mut bus, mut d, regs) = setup();
    bus.mapear(GuestVirt(0x13000), GuestPhys(0x13000), 1, Perm::RW)
        .unwrap();
    bus.escrever_u32(GuestVirt(0x12FFE), 0x11223344).unwrap();
    assert_eq!(
        call(&mut bus, &mut d, &regs, 0x12FFE, 0xAABBCCDD).unwrap(),
        0x11223344
    );
    assert_eq!(bus.ler_u32(GuestVirt(0x12FFE)).unwrap(), 0xAABBCCDD);
    bus.mapear(GuestVirt(0x13000), GuestPhys(0x13000), 1, Perm::RO)
        .unwrap();
    assert!(call(&mut bus, &mut d, &regs, 0x12FFE, 1).is_err());
    assert_eq!(bus.ler_u32(GuestVirt(0x12FFE)).unwrap(), 0xAABBCCDD);
}

#[test]
fn troca_por_alias_e_visivel_na_visao_original_fisica_e_dma() {
    let (mut bus, mut d, regs) = setup();
    let frame = bus.traduzir(GuestVirt(0x12000), Acesso::Leitura).unwrap();
    bus.mapear(GuestVirt(0x6000_0000), frame, 1, Perm::RW)
        .unwrap();
    bus.escrever_u32(GuestVirt(TARGET), 0x12345678).unwrap();
    assert_eq!(
        call(&mut bus, &mut d, &regs, 0x6000_0100, 0xAABBCCDD).unwrap(),
        0x12345678
    );
    assert_eq!(bus.ler_u32(GuestVirt(TARGET)).unwrap(), 0xAABBCCDD);
    let mut physical = [0; 4];
    bus.ler_fisico(GuestPhys(frame.0 + 0x100), &mut physical)
        .unwrap();
    assert_eq!(physical, 0xAABBCCDDu32.to_le_bytes());
    assert_eq!(&bus.dma(frame, 0x104).unwrap()[0x100..], &physical);
    assert_eq!(
        call(&mut bus, &mut d, &regs, TARGET, 1).unwrap(),
        0xAABBCCDD
    );
    assert_eq!(bus.ler_u32(GuestVirt(0x6000_0100)).unwrap(), 1);
}
