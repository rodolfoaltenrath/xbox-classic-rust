/* MODULES */

#[cfg(test)]
mod indirect_verification {
    use super::*;
    use xbox_core::snapshot::{Snapshot, SnapshotReader, SnapshotWriter};
    use xbox_mem::{Acesso, GuestVirt, Perm};
    use xbox_recomp::{Error, runtime::{State, Session, Exit, RETURN_SENTINEL}};
    type Run = fn(&mut State, &mut xbox_mem::Bus, u64) -> xbox_recomp::Result<Exit>;

    fn save(s: &Session) -> Vec<u8> {
        let mut w = SnapshotWriter::new(); s.state.save(&mut w); s.bus.save(&mut w); w.finish()
    }
    fn unknown(result: xbox_recomp::Result<Exit>, pc: u32, address: u32) {
        assert!(matches!(result, Err(Error::UnknownTarget { pc: actual, target }) if actual == pc && target == address));
    }

    #[test]
    fn registradores_e_tail_calls_executam_rust_gerado() {
        let cases = [ /* REGISTERS */ ];
        for (register, call, run, image) in cases {
            let mut s = Session::load(image).unwrap(); let esp = s.state.regs[4];
            s.state.eflags = 0xED7;
            assert_eq!(run(&mut s.state, &mut s.bus, 32).unwrap(), Exit::Returned);
            assert_eq!(s.state.regs[0], 42, "r={register}, call={call}");
            assert_eq!(s.state.regs[4], esp + 4); assert_eq!(s.state.eflags, 0xED7);
            assert_eq!(s.state.retired, if call || register == 4 { 5 } else { 4 });
        }
    }

    #[test]
    fn callbacks_mutaveis_sib_e_ponteiro_readonly_sao_lidos_no_bus() {
        for (run, image, readonly) in [
            (ind_memory::run as Run, include_bytes!("ind_memory.xbe").as_slice(), false),
            (ind_sib::run as Run, include_bytes!("ind_sib.xbe").as_slice(), false),
            (ind_readonly::run as Run, include_bytes!("ind_readonly.xbe").as_slice(), true),
        ] {
            let mut s = Session::load(image).unwrap();
            s.state.regs[3] = 0x12020; s.state.regs[1] = 1;
            if !readonly { s.bus.escrever_u32(GuestVirt(0x12020), 0x11008).unwrap(); }
            assert_eq!(run(&mut s.state, &mut s.bus, 16).unwrap(), Exit::Returned);
            assert_eq!(s.state.regs[0], 42); assert_eq!(s.state.retired, 4);
        }
        let mut s = Session::load(include_bytes!("ind_memory.xbe")).unwrap();
        s.bus.escrever_u32(GuestVirt(0x12020), 0x1100E).unwrap();
        ind_memory::run(&mut s.state, &mut s.bus, 16).unwrap(); assert_eq!(s.state.regs[0], 99);
    }

    #[test]
    fn tabela_de_saltos_e_jump_reg_preservam_pilha() {
        for index in [0,1] {
            let mut s = Session::load(include_bytes!("ind_table.xbe")).unwrap();
            s.bus.escrever_u32(GuestVirt(0x12020), 0x11008).unwrap();
            s.bus.escrever_u32(GuestVirt(0x12024), 0x1100E).unwrap();
            s.state.regs[1] = index; s.state.eflags = 0xA93; let esp = s.state.regs[4];
            assert_eq!(ind_table::run(&mut s.state, &mut s.bus, 1).unwrap(), Exit::Budget);
            assert_eq!(s.state.regs[4], esp); assert_eq!(s.state.retired, 1); assert_eq!(s.state.eflags, 0xA93);
            ind_table::run(&mut s.state, &mut s.bus, 8).unwrap();
            assert_eq!(s.state.regs[0], if index == 0 { 42 } else { 99 });
        }
        let mut s = Session::load(include_bytes!("ind_jump.xbe")).unwrap();
        s.state.regs[0] = 0x11008; let esp = s.state.regs[4];
        ind_jump::run(&mut s.state, &mut s.bus, 1).unwrap();
        assert_eq!(s.state.regs[4], esp); assert_eq!(s.state.eip, 0x11008);
    }

    #[test]
    fn call_com_base_esp_captura_destino_antes_de_empilhar() {
        for (run, image, alias) in [
            (ind_stack::run as Run, include_bytes!("ind_stack.xbe").as_slice(), false),
            (ind_stack_alias::run as Run, include_bytes!("ind_stack_alias.xbe").as_slice(), true),
        ] {
            let mut s = Session::load(image).unwrap(); if !alias { s.state.regs[4] -= 4; }
            let esp = s.state.regs[4];
            s.bus.escrever_u32(GuestVirt(if alias { esp - 4 } else { esp }), 0x11008).unwrap();
            run(&mut s.state, &mut s.bus, 1).unwrap();
            assert_eq!(s.state.eip, 0x11008); assert_eq!(s.state.regs[4], esp - 4);
            assert_eq!(s.bus.ler_u32(GuestVirt(esp - 4)).unwrap(), if alias { 0x11004 } else { 0x11003 });
            assert_eq!(run(&mut s.state, &mut s.bus, 16).unwrap(), Exit::Returned); assert_eq!(s.state.regs[0], 42);
        }
    }

    #[test]
    fn alvos_desconhecidos_nao_comprometem_cpu_pilha_ou_contador() {
        for (run, image) in [
            (ind_dynamic::run as Run, include_bytes!("ind_dynamic.xbe").as_slice()),
            (ind_jump::run as Run, include_bytes!("ind_jump.xbe").as_slice()),
        ] {
            for target in [0x11009, 0x11007, 0x12020, 0x50000000, RETURN_SENTINEL] {
                let mut s = Session::load(image).unwrap(); s.state.regs[0] = target; s.state.eflags = 0xED7;
                let before = s.state.clone(); let esp = s.state.regs[4];
                s.bus.escrever_u32(GuestVirt(esp-4), 0x12345678).unwrap();
                unknown(run(&mut s.state, &mut s.bus, 1), 0x11000, target); assert_eq!(s.state, before);
                assert_eq!(s.bus.ler_u32(GuestVirt(esp-4)).unwrap(), 0x12345678);
            }
        }
        // Qualquer início decodificado, incluindo o RET no interior de uma rotina.
        let mut s = Session::load(include_bytes!("ind_jump.xbe")).unwrap(); s.state.regs[0] = 0x1100D;
        assert_eq!(ind_jump::run(&mut s.state, &mut s.bus, 4).unwrap(), Exit::Returned);
    }

    #[test]
    fn faltas_de_leitura_execucao_e_pilha_nao_comprometem_estado() {
        let mut s = Session::load(include_bytes!("ind_memory.xbe")).unwrap(); s.state.regs[4] = 0;
        s.bus.mmu_mut().desmapear(GuestVirt(0x12000), 1);
        let before = s.state.clone();
        assert!(matches!(ind_memory::run(&mut s.state, &mut s.bus, 1), Err(Error::Memory(f)) if f.acesso == Acesso::Leitura && f.endereco == GuestVirt(0x12020)));
        assert_eq!(s.state, before);
        let mut s = Session::load(include_bytes!("ind_dynamic.xbe")).unwrap(); s.state.regs[0] = 0x11008;
        let frame = s.bus.traduzir(GuestVirt(0x11000), Acesso::Leitura).unwrap();
        s.bus.mapear(GuestVirt(0x11000), frame, 1, Perm::RO).unwrap();
        let before = s.state.clone();
        assert!(matches!(ind_dynamic::run(&mut s.state, &mut s.bus, 1), Err(Error::Memory(f)) if f.acesso == Acesso::Execucao));
        assert_eq!(s.state, before);
        // Store do retorno atravessa página inválida: nem os bytes iniciais mudam.
        let mut s = Session::load(include_bytes!("ind_dynamic.xbe")).unwrap();
        s.state.regs[0] = 0x11008; s.state.regs[4] = 0x13002;
        s.bus.escrever_u16(GuestVirt(0x12FFE), 0x1234).unwrap(); let before = s.state.clone();
        assert!(matches!(ind_dynamic::run(&mut s.state, &mut s.bus, 1), Err(Error::Memory(f)) if f.acesso == Acesso::Escrita));
        assert_eq!(s.state, before); assert_eq!(s.bus.ler_u16(GuestVirt(0x12FFE)).unwrap(), 0x1234);
    }

    #[test]
    fn ret_valida_destino_antes_de_desempilhar_e_aceita_sentinela() {
        for target in [0x11009, 0x50000000] {
            let mut s = Session::load(include_bytes!("ind_return.xbe")).unwrap();
            s.bus.escrever_u32(GuestVirt(s.state.regs[4]), target).unwrap(); let before = s.state.clone();
            unknown(ind_return::run(&mut s.state, &mut s.bus, 1), 0x11000, target); assert_eq!(s.state, before);
        }
        let mut s = Session::load(include_bytes!("ind_return.xbe")).unwrap();
        s.state.regs[4] -= 4; s.bus.escrever_u32(GuestVirt(s.state.regs[4]), 0x11008).unwrap();
        ind_return::run(&mut s.state, &mut s.bus, 8).unwrap();
        assert!(s.state.returned); assert_eq!(s.state.regs[0], 42); assert_eq!(s.state.retired, 3);
    }

    #[test]
    fn snapshot_preserva_call_indireta_pendente_e_tabela_mutavel() {
        let image = include_bytes!("ind_memory.xbe"); let mut s = Session::load(image).unwrap();
        s.bus.escrever_u32(GuestVirt(0x12020), 0x11008).unwrap();
        ind_memory::run(&mut s.state, &mut s.bus, 1).unwrap();
        assert_eq!(s.state.eip, 0x11008); assert_eq!(s.state.retired, 1); let checkpoint = save(&s);
        ind_memory::run(&mut s.state, &mut s.bus, 16).unwrap(); let expected = save(&s);
        let mut resumed = Session::load(image).unwrap(); let mut reader = SnapshotReader::new(&checkpoint).unwrap();
        resumed.state.load(&mut reader).unwrap(); resumed.bus.load(&mut reader).unwrap();
        while ind_memory::run(&mut resumed.state, &mut resumed.bus, 1).unwrap() == Exit::Budget {}
        assert_eq!(save(&resumed), expected);
    }

    #[test]
    fn loop_indireto_respeita_orcamento_e_zero_nao_altera_estado() {
        let mut s = Session::load(include_bytes!("ind_jump.xbe")).unwrap(); s.state.regs[0] = 0x11000;
        let before = s.state.clone();
        assert_eq!(ind_jump::run(&mut s.state, &mut s.bus, 0).unwrap(), Exit::Budget); assert_eq!(s.state, before);
        assert_eq!(ind_jump::run(&mut s.state, &mut s.bus, 100).unwrap(), Exit::Budget);
        assert_eq!(s.state.eip, 0x11000); assert_eq!(s.state.retired, 100);
        assert_eq!(s.state.regs, before.regs); assert_eq!(s.state.eflags, before.eflags);
        // Recursão indireta também só ocupa a pilha guest e sai por orçamento.
        let mut s = Session::load(include_bytes!("ind_dynamic.xbe")).unwrap(); s.state.regs[0] = 0x11000;
        let esp = s.state.regs[4];
        assert_eq!(ind_dynamic::run(&mut s.state, &mut s.bus, 3).unwrap(), Exit::Budget);
        assert_eq!(s.state.regs[4], esp - 12); assert_eq!(s.state.retired, 3);
        s.state.regs[0] = 0x11008;
        assert_eq!(ind_dynamic::run(&mut s.state, &mut s.bus, 32).unwrap(), Exit::Returned);
        assert_eq!(s.state.regs[0], 42); assert_eq!(s.state.regs[4], esp + 4);
    }
}
