//! Operações e tamanhos explícitos da tradução inteira x86.

/// Tamanho de um operando inteiro, independente do tamanho do endereço.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Width {
    Byte,
    Word,
    Dword,
}

impl Width {
    pub const fn bits(self) -> u32 {
        match self {
            Self::Byte => 8,
            Self::Word => 16,
            Self::Dword => 32,
        }
    }

    pub const fn bytes(self) -> u32 {
        self.bits() / 8
    }

    pub const fn mask(self) -> u32 {
        match self {
            Self::Byte => 0xFF,
            Self::Word => 0xFFFF,
            Self::Dword => u32::MAX,
        }
    }
}

/// Registradores completos, fatias de registrador, imediatos e memória tipada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operand {
    Reg(usize),
    Part {
        index: usize,
        shift: u32,
        width: Width,
    },
    Imm(u32),
    Mem {
        base: Option<usize>,
        index: Option<usize>,
        scale: u32,
        displacement: u32,
        width: Width,
    },
}

impl Operand {
    pub const fn width(&self) -> Option<Width> {
        match self {
            Self::Reg(_) => Some(Width::Dword),
            Self::Part { width, .. } | Self::Mem { width, .. } => Some(*width),
            Self::Imm(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Binary {
    Mov,
    Add,
    Adc,
    Sub,
    Sbb,
    Xor,
    And,
    Or,
    Cmp,
    Test,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unary {
    Inc,
    Dec,
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shift {
    Shl,
    Shr,
    Sar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rotate {
    Rol,
    Ror,
    Rcl,
    Rcr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoubleShift {
    Shld,
    Shrd,
}

/// As 16 condições de Jcc, em ordem de encoding Intel (0x70..0x7F).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Condition {
    Overflow,
    NotOverflow,
    Below,
    AboveEqual,
    Equal,
    NotEqual,
    BelowEqual,
    Above,
    Sign,
    NotSign,
    Parity,
    NotParity,
    Less,
    GreaterEqual,
    LessEqual,
    Greater,
}

/// IR explícita; nenhuma instrução desconhecida vira NOP.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operation {
    Binary(Binary, Operand, Operand),
    Unary(Unary, Operand),
    Shift(Shift, Operand, Operand),
    Rotate(Rotate, Operand, Operand),
    DoubleShift(DoubleShift, Operand, Operand, Operand),
    Multiply {
        signed: bool,
        src: Operand,
    },
    Divide {
        signed: bool,
        src: Operand,
    },
    Imul {
        dst: Operand,
        left: Operand,
        right: Operand,
    },
    SignExtend {
        source: Width,
        pair: bool,
    },
    Extend {
        signed: bool,
        dst: Operand,
        src: Operand,
    },
    Lea {
        dst: Operand,
        address: Operand,
    },
    Push(Operand, Width),
    Pop(Operand, Width),
    Leave,
    Jump(u32),
    Branch {
        condition: Condition,
        target: u32,
    },
    Call(u32),
    Ret(u16),
    Nop,
}
