
#[cfg(test)] mod parts;
#[cfg(test)] mod high_byte;
#[cfg(test)] mod frame;
#[cfg(test)] mod push_esp;
#[cfg(test)] mod pop_esp;
#[cfg(test)] mod pop_mem_esp;
#[cfg(test)] mod word_stack;
#[cfg(test)] mod byte_inc;

#[cfg(test)]
mod integer_verification {
    use super::*;
    use xbox_mem::GuestVirt;
    use xbox_recomp::runtime::{Exit, Session};

    #[test]
    fn registradores_parciais_extensoes_e_lea_sem_acesso_a_memoria() {
        let mut s = Session::load(include_bytes!("parts.xbe")).unwrap();
        s.state.eflags = 0xAD7;
        assert_eq!(parts::run(&mut s.state, &mut s.bus, 10).unwrap(), Exit::Returned);
        assert_eq!(s.state.regs[0], 0x1234EF01);
        assert_eq!(s.state.regs[1], 0x1234EF09);
        assert_eq!(s.state.regs[2], 0xEF);
        assert_eq!(s.state.regs[3], 0xFFFFFFEF);
        assert_eq!(s.state.regs[6], 0xEF01);
        assert_eq!(s.state.regs[7], 0xFFFFEF01);
        assert_eq!(s.bus.ler_u8(GuestVirt(0x12020)).unwrap(), 0xEF);
        assert_eq!(s.bus.ler_u16(GuestVirt(0x12022)).unwrap(), 0xEF01);
        assert_eq!(s.state.eflags, 0xAD7);
    }

    #[test]
    fn alu_em_ah_preserva_al_e_bits_superiores() {
        let mut s = Session::load(include_bytes!("high_byte.xbe")).unwrap();
        high_byte::run(&mut s.state, &mut s.bus, 10).unwrap();
        assert_eq!(s.state.regs[0], 0x12348056);
        assert_eq!(s.state.eflags & 0x8D5, 0x890);
    }

    #[test]
    fn frame_push_esp_pop_esp_e_pop_memoria_usam_a_pilha_correta() {
        let mut s = Session::load(include_bytes!("frame.xbe")).unwrap();
        let esp = s.state.regs[4];
        s.state.regs[5] = 0x12345678;
        frame::run(&mut s.state, &mut s.bus, 10).unwrap();
        assert_eq!(s.state.regs[0], 42);
        assert_eq!(s.state.regs[5], 0x12345678);
        assert_eq!(s.state.regs[4], esp + 4);
        let mut s = Session::load(include_bytes!("push_esp.xbe")).unwrap();
        let esp = s.state.regs[4];
        push_esp::run(&mut s.state, &mut s.bus, 10).unwrap();
        assert_eq!(s.state.regs[0], esp);
        assert_eq!(s.state.regs[2], esp);
        let mut s = Session::load(include_bytes!("pop_esp.xbe")).unwrap();
        let esp = s.state.regs[4];
        pop_esp::run(&mut s.state, &mut s.bus, 10).unwrap();
        assert_eq!(s.state.regs[4], esp + 4);
        let mut s = Session::load(include_bytes!("pop_mem_esp.xbe")).unwrap();
        pop_mem_esp::run(&mut s.state, &mut s.bus, 10).unwrap();
        assert_eq!(s.state.regs[0], 5);
    }

    #[test]
    fn pilha_de_16_bits_estende_imediato_e_preserva_registrador() {
        let mut s = Session::load(include_bytes!("word_stack.xbe")).unwrap();
        let esp = s.state.regs[4];
        s.state.regs[0] = 0x12340000;
        s.state.eflags = 0xAD7;
        word_stack::run(&mut s.state, &mut s.bus, 10).unwrap();
        assert_eq!(s.state.regs[0], 0x1234FF80);
        assert_eq!(s.state.regs[1], 0xFFFFFF80);
        assert_eq!(s.state.regs[4], esp + 4);
        assert_eq!(s.state.eflags, 0xAD7);
    }

    #[test]
    fn incremento_de_byte_na_memoria_preserva_carry() {
        let mut s = Session::load(include_bytes!("byte_inc.xbe")).unwrap();
        s.state.eflags = 3;
        byte_inc::run(&mut s.state, &mut s.bus, 10).unwrap();
        assert_eq!(s.state.regs[0], 0);
        assert_eq!(s.bus.ler_u8(GuestVirt(0x12020)).unwrap(), 0);
        assert_eq!(s.state.eflags & 0x8D5, 0x55);
    }
}
