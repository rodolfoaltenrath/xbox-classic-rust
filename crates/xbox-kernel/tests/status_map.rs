//! `RtlNtStatusToDosError` pela ABI stdcall/4 (ADR-0103, P31: tabela provisória).
use xbox_formats::fixtures::xbe_com_imports;
use xbox_kernel::dispatch::{Dispatcher, ORDINAL_RTL_NT_STATUS_TO_DOS_ERROR};
use xbox_kernel::imports;
use xbox_kernel::loader::{Image, STACK_BASE, STACK_SIZE};
use xbox_mem::GuestVirt;

#[test]
fn status_do_proprio_kernel_viram_erros_win32_e_o_resto_e_317() {
    let image = xbe_com_imports(&[0xC3], &[ORDINAL_RTL_NT_STATUS_TO_DOS_ERROR]).unwrap();
    let mut bus = Image::prepare_aot(&image).unwrap().image.bus;
    let mut d = Dispatcher::new();
    let mut regs = [0; 8];
    regs[4] = STACK_BASE + STACK_SIZE - 0x100;
    let mut dos = |status: u32| {
        bus.escrever_u32(GuestVirt(regs[4]), 0x11000).unwrap();
        bus.escrever_u32(GuestVirt(regs[4] + 4), status).unwrap();
        d.atender(
            &mut bus,
            imports::trampoline(ORDINAL_RTL_NT_STATUS_TO_DOS_ERROR),
            &regs,
        )
        .unwrap()
        .eax
    };
    assert_eq!(dos(0), 0);
    assert_eq!(dos(0xC000_0034), 2, "arquivo não encontrado");
    assert_eq!(dos(0xC000_0022), 5, "acesso negado");
    assert_eq!(dos(0xC000_0008), 6, "handle inválido");
    assert_eq!(dos(0xC000_0011), 38, "fim de arquivo");
    assert_eq!(dos(0xC0DE_0000), 317, "sem mapeamento");
}
