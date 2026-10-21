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
        /// FS contém uma base guest explícita; LEA ignora esta base.
        fs: bool,
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
pub enum BitScan {
    Forward,
    Reverse,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CountBranch {
    Zero,
    Loop,
    LoopEqual,
    LoopNotEqual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StringOp {
    Movs,
    Stos,
    Lods,
    Cmps,
    Scas,
}

/// Operação sobre CF sem operando.
///
/// O fluxo descoberto do homebrew usa `CLC`; `STC` e `CMC` entram junto porque são a
/// mesma família de decodificação e têm semântica mecânica e sem ambiguidade — a mesma
/// razão pela qual as 16 condições de `SETcc` foram feitas de uma vez.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CarryOp {
    /// `CLC`: CF = 0.
    Clear,
    /// `STC`: CF = 1.
    Set,
    /// `CMC`: CF = !CF.
    Complement,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Repeat {
    None,
    Always,
    Equal,
    NotEqual,
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
    CompareExchange {
        dst: Operand,
        src: Operand,
        locked: bool,
    },
    SetCondition {
        condition: Condition,
        dst: Operand,
    },
    ConditionalMove {
        condition: Condition,
        dst: Operand,
        src: Operand,
    },
    BitScan {
        kind: BitScan,
        dst: Operand,
        src: Operand,
    },
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
    CountBranch {
        kind: CountBranch,
        counter: Width,
        target: u32,
    },
    String {
        kind: StringOp,
        width: Width,
        address: Width,
        repeat: Repeat,
    },
    Direction(bool),
    /// `CLC`/`STC`/`CMC`: manipulação direta de CF, sem operando.
    Carry(CarryOp),
    Call(u32),
    Indirect {
        call: bool,
        target: Operand,
    },
    Ret(u16),
    ReadTimestamp,
    /// RDMSR: índice em ECX, resultado EDX:EAX, somente MSRs do perfil guest.
    ReadMsr,
    /// Perfil Xbox privilegiado: limpa IF guest, sem executar CLI no host.
    ClearInterrupts,
    /// Suspende a CPU guest e entrega o controle ao host.
    Halt,
    /// INT3 (0xCC): fronteira de exceção #BP, sem entrega via IDT neste perfil.
    Breakpoint,
    /// SFENCE: ordenação de stores pelo contrato do bus, sem fence de GPU.
    StoreFence,
    Nop,
}
