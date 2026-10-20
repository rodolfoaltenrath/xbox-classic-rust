//! Vetores independentes, memória guest e contexto opaco copiável do perfil HLE.
use std::fmt::Write;
use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
use xbox_formats::fixtures::xbe_com_imports;
use xbox_kernel::dispatch::{Dispatcher, Erro, SHA_CONTEXT_SIZE};
use xbox_kernel::imports;
use xbox_kernel::loader::{Image, STACK_BASE, STACK_SIZE};
use xbox_mem::{Acesso, Bus, GuestPhys, GuestVirt, Perm};

const CONTEXT: u32 = STACK_BASE + 0x1000;
const COPY: u32 = CONTEXT + 0x100;
const DIGEST: u32 = CONTEXT + 0x200;
const INPUT: u32 = STACK_BASE + 0x2000;

fn setup() -> (Bus, Dispatcher) {
    let image = xbe_com_imports(&[0xC3], &[335, 336, 337]).unwrap();
    (
        Image::prepare_aot(&image).unwrap().image.bus,
        Dispatcher::new(),
    )
}

fn call(bus: &mut Bus, d: &mut Dispatcher, ordinal: u32, args: &[u32]) -> Result<u32, Erro> {
    let esp = STACK_BASE + STACK_SIZE - 0x100;
    bus.escrever_u32(GuestVirt(esp), 0x11000).unwrap();
    for (i, word) in args.iter().enumerate() {
        bus.escrever_u32(GuestVirt(esp + 4 + i as u32 * 4), *word)
            .unwrap();
    }
    let mut regs = [0; 8];
    regs[4] = esp;
    d.atender(bus, imports::trampoline(ordinal), &regs)
        .map(|r| r.eax)
}

fn bytes(bus: &Bus, at: u32, len: usize) -> Vec<u8> {
    let mut bytes = vec![0; len];
    bus.ler_bytes(GuestVirt(at), &mut bytes).unwrap();
    bytes
}

fn digest(bus: &mut Bus, d: &mut Dispatcher, context: u32) -> String {
    call(bus, d, 337, &[context, DIGEST]).unwrap();
    bytes(bus, DIGEST, 20)
        .into_iter()
        .fold(String::new(), |mut s, b| {
            write!(s, "{b:02x}").unwrap();
            s
        })
}

#[test]
fn init_escreve_exatamente_116_bytes_reinicializa_e_preserva_guardas() {
    let (mut bus, mut d) = setup();
    bus.escrever_bytes(GuestVirt(CONTEXT - 1), &[0xAA; SHA_CONTEXT_SIZE + 2])
        .unwrap();
    assert_eq!(call(&mut bus, &mut d, 335, &[CONTEXT]).unwrap(), 0);
    assert_eq!(bus.ler_u8(GuestVirt(CONTEXT - 1)).unwrap(), 0xAA);
    assert_eq!(bus.ler_u8(GuestVirt(CONTEXT + 116)).unwrap(), 0xAA);
    assert_eq!(
        digest(&mut bus, &mut d, CONTEXT),
        "da39a3ee5e6b4b0d3255bfef95601890afd80709"
    );
    bus.escrever_bytes(GuestVirt(INPUT), b"abc").unwrap();
    call(&mut bus, &mut d, 336, &[CONTEXT, INPUT, 3]).unwrap();
    call(&mut bus, &mut d, 335, &[CONTEXT]).unwrap();
    assert_eq!(
        digest(&mut bus, &mut d, CONTEXT),
        "da39a3ee5e6b4b0d3255bfef95601890afd80709"
    );
}

#[test]
fn vetores_rfc_3174_abc_multibloco_e_um_milhao_de_as() {
    let (mut bus, mut d) = setup();
    for (message, expected) in [
        (&b"abc"[..], "a9993e364706816aba3e25717850c26c9cd0d89d"),
        (
            &b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"[..],
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1",
        ),
    ] {
        call(&mut bus, &mut d, 335, &[CONTEXT]).unwrap();
        bus.escrever_bytes(GuestVirt(INPUT), message).unwrap();
        call(
            &mut bus,
            &mut d,
            336,
            &[CONTEXT, INPUT, message.len() as u32],
        )
        .unwrap();
        assert_eq!(digest(&mut bus, &mut d, CONTEXT), expected);
    }
    call(&mut bus, &mut d, 335, &[CONTEXT]).unwrap();
    bus.escrever_bytes(GuestVirt(INPUT), &[b'a'; 1000]).unwrap();
    for _ in 0..1000 {
        call(&mut bus, &mut d, 336, &[CONTEXT, INPUT, 1000]).unwrap();
    }
    assert_eq!(
        digest(&mut bus, &mut d, CONTEXT),
        "34aa973cd4c4daa4f61eeb2bdbad27316534016f"
    );
}

#[test]
fn bordas_de_padding_blocos_e_paginas_conferem_com_oraculo_dotnet() {
    // SHA1.ComputeHash do .NET, calculado separadamente para bytes i % 251.
    let cases = [
        (0, "da39a3ee5e6b4b0d3255bfef95601890afd80709"),
        (1, "5ba93c9db0cff93f52b521d7420e43f6eda2784f"),
        (55, "8ae2d46729cfe68ff927af5eec9c7d1b66d65ac2"),
        (56, "636e2ec698dac903498e648bd2f3af641d3c88cb"),
        (57, "7cb1330f35244b57437539253304ea78a6b7c443"),
        (63, "6d942da0c4392b123528f2905c713a3ce28364bd"),
        (64, "c6138d514ffa2135bfce0ed0b8fac65669917ec7"),
        (65, "69bd728ad6e13cd76ff19751fde427b00e395746"),
        (119, "41c89d06001bab4ab78736b44efe7ce18ce6ae08"),
        (120, "d3dbd653bd8597b7475321b60a36891278e6a04a"),
        (127, "89d7312a903f65cd2b3e34a975e55dbea9033353"),
        (128, "e6434bc401f98603d7eda504790c98c67385d535"),
        (4095, "2981e06893e00f71bb3aecb4337cf741b91bfc6e"),
        (4096, "92ca8f2b4163e64a1b53e0fde263ad56cbdb75fc"),
        (4097, "199b39f39620b41326652dea09d98b57b0381329"),
    ];
    let (mut bus, mut d) = setup();
    for (size, expected) in cases {
        let message: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
        bus.escrever_bytes(GuestVirt(INPUT), &message).unwrap();
        for chunk in [1, 7, 55, 64, 65, 4096, 8192] {
            call(&mut bus, &mut d, 335, &[CONTEXT]).unwrap();
            for offset in (0..size).step_by(chunk) {
                call(
                    &mut bus,
                    &mut d,
                    336,
                    &[
                        CONTEXT,
                        INPUT + offset as u32,
                        (size - offset).min(chunk) as u32,
                    ],
                )
                .unwrap();
            }
            assert_eq!(
                digest(&mut bus, &mut d, CONTEXT),
                expected,
                "size={size}, chunk={chunk}"
            );
        }
    }
}

#[test]
fn final_preserva_contexto_no_perfil_e_update_pode_continuar() {
    let (mut bus, mut d) = setup();
    call(&mut bus, &mut d, 335, &[CONTEXT]).unwrap();
    bus.escrever_bytes(GuestVirt(INPUT), b"abc").unwrap();
    call(&mut bus, &mut d, 336, &[CONTEXT, INPUT, 1]).unwrap();
    let before = bytes(&bus, CONTEXT, 116);
    assert_eq!(
        digest(&mut bus, &mut d, CONTEXT),
        "86f7e437faa5a7fce15d1ddcb9eaeaea377667b8"
    );
    assert_eq!(bytes(&bus, CONTEXT, 116), before);
    call(&mut bus, &mut d, 336, &[CONTEXT, INPUT + 1, 2]).unwrap();
    assert_eq!(
        digest(&mut bus, &mut d, CONTEXT),
        "a9993e364706816aba3e25717850c26c9cd0d89d"
    );
}

#[test]
fn copiar_contexto_em_ram_cria_fluxos_independentes_sem_mapa_host() {
    let (mut bus, mut d) = setup();
    call(&mut bus, &mut d, 335, &[CONTEXT]).unwrap();
    bus.escrever_bytes(GuestVirt(INPUT), b"abc").unwrap();
    call(&mut bus, &mut d, 336, &[CONTEXT, INPUT, 1]).unwrap();
    let copy = bytes(&bus, CONTEXT, 116);
    bus.escrever_bytes(GuestVirt(COPY), &copy).unwrap();
    call(&mut bus, &mut d, 336, &[COPY, INPUT + 1, 2]).unwrap();
    assert_eq!(
        digest(&mut bus, &mut d, COPY),
        "a9993e364706816aba3e25717850c26c9cd0d89d"
    );
    assert_eq!(
        digest(&mut bus, &mut d, CONTEXT),
        "86f7e437faa5a7fce15d1ddcb9eaeaea377667b8"
    );
}

#[test]
fn entrada_sobreposta_ao_contexto_e_consumida_antes_do_store() {
    let (mut bus, mut d) = setup();
    call(&mut bus, &mut d, 335, &[CONTEXT]).unwrap();
    let original = bytes(&bus, CONTEXT, 116);
    bus.escrever_bytes(GuestVirt(INPUT), &original).unwrap();
    call(&mut bus, &mut d, 335, &[COPY]).unwrap();
    call(&mut bus, &mut d, 336, &[COPY, INPUT, 116]).unwrap();
    call(&mut bus, &mut d, 336, &[CONTEXT, CONTEXT, 116]).unwrap();
    assert_eq!(
        digest(&mut bus, &mut d, CONTEXT),
        digest(&mut bus, &mut d, COPY)
    );
}

#[test]
fn digest_sobreposto_ao_contexto_escreve_so_vinte_bytes() {
    let (mut bus, mut d) = setup();
    call(&mut bus, &mut d, 335, &[CONTEXT]).unwrap();
    bus.escrever_bytes(GuestVirt(INPUT), b"abc").unwrap();
    call(&mut bus, &mut d, 336, &[CONTEXT, INPUT, 3]).unwrap();
    let expected = digest(&mut bus, &mut d, CONTEXT);
    let before = bytes(&bus, CONTEXT, 116);
    let digest_bytes = bytes(&bus, DIGEST, 20);
    call(&mut bus, &mut d, 337, &[CONTEXT, CONTEXT + 9]).unwrap();
    let after = bytes(&bus, CONTEXT, 116);
    assert_eq!(&after[..9], &before[..9]);
    assert_eq!(&after[9..29], &digest_bytes);
    assert_eq!(&after[29..], &before[29..]);
    assert_eq!(expected, "a9993e364706816aba3e25717850c26c9cd0d89d");
}

#[test]
fn update_vazio_aceita_ponteiro_nulo_sem_acesso_ou_mudanca_de_contexto() {
    let (mut bus, mut d) = setup();
    call(&mut bus, &mut d, 335, &[CONTEXT]).unwrap();
    let before = bytes(&bus, CONTEXT, 116);
    for pointer in [0, u32::MAX, 0x50000000] {
        call(&mut bus, &mut d, 336, &[CONTEXT, pointer, 0]).unwrap();
        assert_eq!(bytes(&bus, CONTEXT, 116), before);
    }
}

#[test]
fn contexto_invalido_ou_nao_inicializado_recusa_sem_publicar_saida() {
    let (mut bus, mut d) = setup();
    bus.escrever_bytes(GuestVirt(DIGEST), &[0xAA; 20]).unwrap();
    assert!(call(&mut bus, &mut d, 337, &[CONTEXT, DIGEST]).is_err());
    call(&mut bus, &mut d, 335, &[CONTEXT]).unwrap();
    let valid = bytes(&bus, CONTEXT, 116);
    for offset in [0, 4, 35, 36, 100] {
        let mut bad = valid.clone();
        bad[offset] ^= 0xFF;
        bus.escrever_bytes(GuestVirt(CONTEXT), &bad).unwrap();
        for (ordinal, args) in [(336, vec![CONTEXT, INPUT, 1]), (337, vec![CONTEXT, DIGEST])] {
            assert!(matches!(
                call(&mut bus, &mut d, ordinal, &args),
                Err(Erro::ArgumentoInvalido { .. })
            ));
            assert_eq!(bytes(&bus, CONTEXT, 116), bad);
            assert_eq!(bytes(&bus, DIGEST, 20), [0xAA; 20]);
        }
    }
}

#[test]
fn init_com_falta_tardia_nao_deixa_contexto_parcial() {
    let (mut bus, mut d) = setup();
    let at = STACK_BASE + STACK_SIZE - 60;
    bus.escrever_bytes(GuestVirt(at), &[0xAA; 60]).unwrap();
    for pointer in [0, u32::MAX - 50, 0x50000000, 0x11000, at] {
        assert!(call(&mut bus, &mut d, 335, &[pointer]).is_err());
        assert_eq!(bytes(&bus, at, 60), [0xAA; 60]);
    }
}

#[test]
fn entrada_com_falta_tardia_ou_wrap_preserva_contexto() {
    let (mut bus, mut d) = setup();
    call(&mut bus, &mut d, 335, &[CONTEXT]).unwrap();
    let before = bytes(&bus, CONTEXT, 116);
    for (input, size) in [(0, 1), (u32::MAX - 1, 4), (STACK_BASE + STACK_SIZE - 2, 4)] {
        assert!(call(&mut bus, &mut d, 336, &[CONTEXT, input, size]).is_err());
        assert_eq!(bytes(&bus, CONTEXT, 116), before);
    }
}

#[test]
fn final_com_falta_tardia_nao_deixa_digest_parcial() {
    let (mut bus, mut d) = setup();
    call(&mut bus, &mut d, 335, &[CONTEXT]).unwrap();
    let context = bytes(&bus, CONTEXT, 116);
    let at = STACK_BASE + STACK_SIZE - 10;
    bus.escrever_bytes(GuestVirt(at), &[0xAA; 10]).unwrap();
    for pointer in [0, u32::MAX - 9, 0x50000000, 0x11000, at] {
        assert!(call(&mut bus, &mut d, 337, &[CONTEXT, pointer]).is_err());
        assert_eq!(bytes(&bus, CONTEXT, 116), context);
        assert_eq!(bytes(&bus, at, 10), [0xAA; 10]);
    }
}

#[test]
fn contexto_ro_permite_final_mas_recusa_init_e_update() {
    let (mut bus, mut d) = setup();
    call(&mut bus, &mut d, 335, &[CONTEXT]).unwrap();
    let frame = bus.traduzir(GuestVirt(CONTEXT), Acesso::Leitura).unwrap();
    bus.mapear(GuestVirt(CONTEXT), frame, 1, Perm::RO).unwrap();
    // Digest em outra página, fora da proteção RO.
    call(&mut bus, &mut d, 337, &[CONTEXT, INPUT]).unwrap();
    let before = bytes(&bus, CONTEXT, 116);
    assert!(call(&mut bus, &mut d, 335, &[CONTEXT]).is_err());
    assert!(call(&mut bus, &mut d, 336, &[CONTEXT, 0, 0]).is_err());
    assert_eq!(bytes(&bus, CONTEXT, 116), before);
}

#[test]
fn mmio_nao_pode_ser_contexto_entrada_ou_digest() {
    let (mut bus, mut d) = setup();
    bus.mapear(GuestVirt(0x50000000), GuestPhys(0xFD000000), 1, Perm::RW)
        .unwrap();
    call(&mut bus, &mut d, 335, &[CONTEXT]).unwrap();
    let before = bytes(&bus, CONTEXT, 116);
    for (ordinal, args) in [
        (335, vec![0x50000000]),
        (336, vec![CONTEXT, 0x50000000, 1]),
        (337, vec![CONTEXT, 0x50000000]),
        (337, vec![0x50000000, DIGEST]),
    ] {
        assert!(matches!(
            call(&mut bus, &mut d, ordinal, &args),
            Err(Erro::ArgumentoInvalido { .. })
        ));
    }
    assert_eq!(bytes(&bus, CONTEXT, 116), before);
}

#[test]
fn contexto_desalinhado_cruzando_paginas_funciona() {
    let (mut bus, mut d) = setup();
    let at = CONTEXT + 4096 - 53;
    call(&mut bus, &mut d, 335, &[at]).unwrap();
    bus.escrever_bytes(GuestVirt(INPUT + 500), b"abc").unwrap();
    call(&mut bus, &mut d, 336, &[at, INPUT + 500, 3]).unwrap();
    assert_eq!(
        digest(&mut bus, &mut d, at),
        "a9993e364706816aba3e25717850c26c9cd0d89d"
    );
}

#[test]
fn contador_sha_recusa_overflow_sem_wrap_ou_store_parcial() {
    let (mut bus, mut d) = setup();
    call(&mut bus, &mut d, 335, &[CONTEXT]).unwrap();
    bus.escrever_u64(GuestVirt(CONTEXT + 28), u64::MAX / 8)
        .unwrap();
    let before = bytes(&bus, CONTEXT, 116);
    assert!(call(&mut bus, &mut d, 336, &[CONTEXT, INPUT, 1]).is_err());
    assert_eq!(bytes(&bus, CONTEXT, 116), before);
}

#[test]
fn snapshot_da_memoria_retoma_hash_parcial_sem_estado_sha_no_host() {
    let (mut bus, mut d) = setup();
    call(&mut bus, &mut d, 335, &[CONTEXT]).unwrap();
    bus.escrever_bytes(GuestVirt(INPUT), b"abc").unwrap();
    call(&mut bus, &mut d, 336, &[CONTEXT, INPUT, 1]).unwrap();
    let mut out = SnapshotWriter::new();
    bus.save(&mut out);
    d.save(&mut out);
    let saved = out.finish();
    call(&mut bus, &mut d, 336, &[CONTEXT, INPUT + 1, 2]).unwrap();
    let expected = digest(&mut bus, &mut d, CONTEXT);
    let mut inp = SnapshotReader::new(&saved).unwrap();
    bus.load(&mut inp).unwrap();
    d.load(&mut inp).unwrap();
    assert_eq!(inp.remaining(), 0);
    call(&mut bus, &mut d, 336, &[CONTEXT, INPUT + 1, 2]).unwrap();
    assert_eq!(digest(&mut bus, &mut d, CONTEXT), expected);
}
