use core::marker::PhantomData;

use better_default::Default;
use tap::Conv;
use tracing::{debug, error};

use crate::context::{self, Context, InterruptRegister, Io, Memory, TimerRegisters};

pub mod registers;

#[derive(Debug, Copy, Clone)]
pub enum Operation {
    Nop,
    Halt,
    Load(Target, Target),
    Inc(Target),
    Dec(Target),
    Stop,
    JumpRelative(Condition, i8),
    RotateAccumulator(RotationType, Direction),
    ComplementCarry,
    SetCarry,
    ComplementAccumulator,
    DecimalAdjustAccumulator,
    Compare(Target),
    Or(Target),
    Xor(Target),
    And(Target),
    Sbc(Target),
    Sub(Target),
    Adc(Target),
    Add(Target, Target),
    Return(Condition),
    Push(Registers16),
    Pop(Registers16),
    ReturnInterrupt,
    Jump(Condition, Target),
    AddStack(i8),
    LoadStackOffset(i8),
    DisableInterrupt,
    EnableInterrupt,
    Call(Condition, Target),
    Restart(u16),
    Rotate(RotationType, Direction, Target),
    ShiftArithmetic(Direction, Target),
    Swap(Target),
    ShiftRightLogical(Target),
    TestBit(u8, Target),
    ResetBit(u8, Target),
    SetBit(u8, Target),
}

#[derive(Debug, Clone, Copy)]
pub enum Registers8 {
    A,
    B,
    C,
    D,
    E,
    H,
    L,
}

#[derive(Debug, Clone, Copy)]
pub enum Registers16 {
    BC,
    DE,
    HL,
    SP,
    AF,
}

#[derive(Debug, Clone, Copy)]

pub enum Indirect {
    R16(Registers16),
    Imm16(u16),
    HLI,
    HLD,
    C,
}

#[derive(Debug, Clone, Copy)]

pub enum Target {
    R8(Registers8),
    R16(Registers16),
    Imm8(u8),
    Imm16(u16),
    Ind(Indirect),
}

#[derive(Debug, Clone, Copy)]

pub enum Condition {
    None,
    NZ,
    Z,
    NC,
    C,
}

#[derive(Debug, Clone, Copy)]

pub enum RotationType {
    Circular,
    NonCircular,
}

#[derive(Debug, Clone, Copy)]

pub enum Direction {
    Left,
    Right,
}

#[derive(Debug, Default)]
pub enum State {
    Decode(usize),
    Execute(Operation, usize),
    #[default]
    ExecutionDone,
    HandlingInterrupts(usize),
}

#[derive(Default)]
pub struct CPU<T> {
    pub registers: registers::Registers,
    pub pc: u16,
    pub ir: u8,
    pub ime: bool,
    pub halted: bool,
    pub state: State,
    pub(crate) decode_state: DecodeState,
    pub(crate) execute_state: ExecuteState,
    interrupt_address: u16,
    _phantom: PhantomData<T>,
}

#[derive(Default)]
pub(crate) struct DecodeState {
    lsb: u8,
    msb: u8,
    value: u8,
    offset: i8,
}

#[derive(Default)]
pub(crate) struct ExecuteState {
    value: u8,
    msb: u8,
    address: u16,
    condition_met: bool,
    carry: bool,
    three_quarter_carry: bool,
    lsb: u8,
    z_sign: bool,
}

impl<T: Memory + Default> CPU<T> {
    pub(crate) fn timer_tick(&mut self, ctx: &mut Context<T>) {
        ctx.memory.io_mut().tick_timer();
    }
    pub fn increment_pc(&mut self, ctx: &mut Context<T>) {
        self.pc = self.pc.wrapping_add(1);
    }

    pub fn load_boot_rom(&mut self, rom: &[u8], ctx: &mut Context<T>) {
        ctx.memory.load_boot_rom(rom);
    }

    pub fn load_rom(&mut self, rom: &[u8], ctx: &mut Context<T>) {
        ctx.memory.load_rom(rom);
    }

    pub fn load_debug_initial_state(&mut self, _ctx: &mut Context<T>) {
        self.registers.a = 0x01;
        *self.registers.f = 0xB0;
        self.registers.b = 0x00;
        self.registers.c = 0x13;
        self.registers.d = 0x00;
        self.registers.e = 0xD8;
        self.registers.h = 0x01;
        self.registers.l = 0x4D;
        self.registers.sp = 0xFFFE;
        self.pc = 0x0100;
    }

    pub fn dump_state(&mut self, ctx: &mut Context<T>) -> String {
        format!(
            "A:{:02X} F:{:02X} B:{:02X} C:{:02X} D:{:02X} E:{:02X} H:{:02X} L:{:02X} SP:{:04X} PC:{:04X} PCMEM:{:02X},{:02X},{:02X},{:02X}",
            self.registers.a,
            *self.registers.f,
            self.registers.b,
            self.registers.c,
            self.registers.d,
            self.registers.e,
            self.registers.h,
            self.registers.l,
            self.registers.sp,
            self.pc,
            ctx.memory.read_u8(self.pc),
            ctx.memory.read_u8(self.pc.wrapping_add(1)),
            ctx.memory.read_u8(self.pc.wrapping_add(2)),
            ctx.memory.read_u8(self.pc.wrapping_add(3)),
        )
    }

    pub fn tick(&mut self, ctx: &mut Context<T>) {
        if self.halted && (ctx.memory.io().interrupt_flag().read() & ctx.memory.ie()) != 0 {
            self.halted = false;
            self.pc = self.pc.wrapping_add(1);
        }

        if let State::Decode(0) = self.state
            && (ctx.memory.io().interrupt_flag().read() & ctx.memory.ie()) != 0
            && self.ime
        {
            self.state = State::HandlingInterrupts(0);
        }
        if !self.halted {
            if let State::HandlingInterrupts(_) = self.state {
                self.handle_interrupts(ctx);
            }
            if let State::Decode(_) = self.state {
                self.decode(ctx);
            }
            if let State::Execute(operation, _) = self.state {
                self.execute_operation(operation, ctx);
            }
            if let State::ExecutionDone = self.state {
                if !self.halted {
                    debug!(
                        "A:{:02X} F:{:02X} B:{:02X} C:{:02X} D:{:02X} E:{:02X} H:{:02X} L:{:02X} SP:{:04X} PC:{:04X} PCMEM:{:02X},{:02X},{:02X},{:02X}",
                        self.registers.a,
                        *self.registers.f,
                        self.registers.b,
                        self.registers.c,
                        self.registers.d,
                        self.registers.e,
                        self.registers.h,
                        self.registers.l,
                        self.registers.sp,
                        self.pc,
                        ctx.memory.read_u8(self.pc),
                        ctx.memory.read_u8(self.pc + 1),
                        ctx.memory.read_u8(self.pc + 2),
                        ctx.memory.read_u8(self.pc + 3),
                    );
                    self.ir = ctx.memory.read_u8(self.pc);
                    self.pc = self.pc.wrapping_add(1);
                } else {
                    self.ir = ctx.memory.read_u8(self.pc);
                }
                self.state = State::Decode(0);
            }
        }
        self.timer_tick(ctx);
    }

    pub(crate) fn handle_interrupts(&mut self, ctx: &mut Context<T>) {
        let State::HandlingInterrupts(step) = self.state else {
            unreachable!()
        };
        match step {
            0 => {
                let masked = ctx.memory.io().interrupt_flag().read() & ctx.memory.ie();
                let next_interrupt = if masked & 0b1 != 0 {
                    context::InterruptType::VBlank
                } else if masked & 0b10 != 0 {
                    context::InterruptType::LCD
                } else if masked & 0b100 != 0 {
                    context::InterruptType::Timer
                } else if masked & 0b1000 != 0 {
                    context::InterruptType::Serial
                } else if masked & 0b10000 != 0 {
                    context::InterruptType::Joypad
                } else {
                    panic!()
                };
                let address = match next_interrupt {
                    context::InterruptType::Joypad => 0x0060,
                    context::InterruptType::Serial => 0x0058,
                    context::InterruptType::Timer => 0x0050,
                    context::InterruptType::LCD => 0x0048,
                    context::InterruptType::VBlank => 0x0040,
                };
                ctx.memory
                    .io_mut()
                    .interrupt_flag_mut()
                    .clear_interrupt(next_interrupt);
                self.interrupt_address = address;
                self.ime = false;
                self.pc = self.pc.wrapping_sub(1);
            }
            1 => {}
            2 => {
                self.state = State::Execute(
                    Operation::Call(Condition::None, Target::Imm16(self.interrupt_address)),
                    0,
                );
            }
            _ => unreachable!(),
        }
        if let State::HandlingInterrupts(step) = &mut self.state {
            *step += 1;
        }
    }

    #[inline(never)]
    pub(crate) fn decode(&mut self, ctx: &mut Context<T>) {
        use Registers8::*;
        use Registers16::*;
        use Target::*;
        let State::Decode(step) = self.state else {
            unreachable!()
        };

        match decompose_octal_triplet(self.ir) {
            // https://gbdev.io/gb-opcodes/optables/octal
            (0o0, 0o0, 0o0) => self.state = State::Execute(Operation::Nop, 0),
            (0o0, 0o1, 0o0) => match step {
                0 => {
                    self.decode_state.lsb = ctx.memory.read_u8(self.pc);
                    self.increment_pc(ctx);
                }
                1 => {
                    self.decode_state.msb = ctx.memory.read_u8(self.pc);
                    self.increment_pc(ctx);
                }
                2 => {
                    self.state = State::Execute(
                        Operation::Load(
                            Ind(Indirect::Imm16(u16::from_le_bytes([
                                self.decode_state.lsb,
                                self.decode_state.msb,
                            ]))),
                            R16(SP),
                        ),
                        0,
                    );
                }
                _ => unreachable!(),
            },

            (0o0, 0o2, 0o0) => self.state = State::Execute(Operation::Stop, 0),
            (0o0, condition, 0o0) => {
                use Condition::*;
                let condition = match condition {
                    0o3 => None,
                    0o4 => NZ,
                    0o5 => Z,
                    0o6 => NC,
                    0o7 => C,
                    _ => unreachable!(),
                };

                self.decode_state.offset = ctx.memory.read_u8(self.pc) as i8;
                self.increment_pc(ctx);
                self.state = State::Execute(
                    Operation::JumpRelative(condition, self.decode_state.offset),
                    0,
                );
            }
            (0o0, op, 0o1) => {
                let target = match op >> 1 {
                    0 => BC,
                    1 => DE,
                    2 => HL,
                    3 => SP,
                    _ => unreachable!(),
                };
                if op & 0b1 == 0 {
                    match step {
                        0 => {
                            self.decode_state.lsb = ctx.memory.read_u8(self.pc);
                            self.increment_pc(ctx);
                        }
                        1 => {
                            self.decode_state.msb = ctx.memory.read_u8(self.pc);
                            self.increment_pc(ctx);
                        }
                        2 => {
                            self.state = State::Execute(
                                Operation::Load(
                                    R16(target),
                                    Imm16(u16::from_le_bytes([
                                        self.decode_state.lsb,
                                        self.decode_state.msb,
                                    ])),
                                ),
                                0,
                            );
                        }
                        _ => unreachable!(),
                    }
                } else {
                    self.state = State::Execute(Operation::Add(R16(HL), R16(target)), 0);
                }
            }
            (0o0, op, 0o2) => {
                let other = match op >> 1 {
                    0 => Indirect::R16(BC),
                    1 => Indirect::R16(DE),
                    2 => Indirect::HLI,
                    3 => Indirect::HLD,
                    _ => unreachable!(),
                };
                self.state = State::Execute(
                    if op & 0b1 == 0 {
                        Operation::Load(Ind(other), R8(A))
                    } else {
                        Operation::Load(R8(A), Ind(other))
                    },
                    0,
                );
            }
            (0o0, op, 0o3) => {
                let target = match op >> 1 {
                    0 => BC,
                    1 => DE,
                    2 => HL,
                    3 => SP,
                    _ => unreachable!(),
                };
                self.state = State::Execute(
                    if op & 0b1 == 0 {
                        Operation::Inc(R16(target))
                    } else {
                        Operation::Dec(R16(target))
                    },
                    0,
                );
            }
            (0o0, target, 0o4) => {
                let target = match target {
                    0o0 => R8(B),
                    0o1 => R8(C),
                    0o2 => R8(D),
                    0o3 => R8(E),
                    0o4 => R8(H),
                    0o5 => R8(L),
                    0o6 => Ind(Indirect::R16(HL)),
                    0o7 => R8(A),
                    _ => unreachable!(),
                };
                self.state = State::Execute(Operation::Inc(target), 0);
            }
            (0o0, target, 0o5) => {
                let target = match target {
                    0o0 => R8(B),
                    0o1 => R8(C),
                    0o2 => R8(D),
                    0o3 => R8(E),
                    0o4 => R8(H),
                    0o5 => R8(L),
                    0o6 => Ind(Indirect::R16(HL)),
                    0o7 => R8(A),
                    _ => unreachable!(),
                };
                self.state = State::Execute(Operation::Dec(target), 0);
            }
            (0o0, destination, 0o6) => {
                let destination = match destination {
                    0o0 => R8(B),
                    0o1 => R8(C),
                    0o2 => R8(D),
                    0o3 => R8(E),
                    0o4 => R8(H),
                    0o5 => R8(L),
                    0o6 => Ind(Indirect::R16(HL)),
                    0o7 => R8(A),
                    _ => unreachable!(),
                };
                match step {
                    0 => {
                        self.decode_state.value = ctx.memory.read_u8(self.pc);
                        self.increment_pc(ctx);
                    }
                    1 => {
                        self.state = State::Execute(
                            Operation::Load(destination, Imm8(self.decode_state.value)),
                            0,
                        );
                    }
                    _ => unreachable!(),
                }
            }
            (0o0, op @ 0o0..=0o3, 0o7) => {
                let kind = match op >> 1 {
                    0o0 => RotationType::Circular,
                    0o1 => RotationType::NonCircular,
                    _ => unreachable!(),
                };
                let direction = if op & 0b1 == 0 {
                    Direction::Left
                } else {
                    Direction::Right
                };
                self.state = State::Execute(Operation::RotateAccumulator(kind, direction), 0);
            }
            (0o0, 0o4, 0o7) => {
                self.state = State::Execute(Operation::DecimalAdjustAccumulator, 0);
            }
            (0o0, 0o5, 0o7) => {
                self.state = State::Execute(Operation::ComplementAccumulator, 0);
            }
            (0o0, 0o6, 0o7) => {
                self.state = State::Execute(Operation::SetCarry, 0);
            }
            (0o0, 0o7, 0o7) => {
                self.state = State::Execute(Operation::ComplementCarry, 0);
            }
            (0o1, 0o6, 0o6) => {
                self.state = State::Execute(Operation::Halt, 0);
            }
            (0o1, destination @ 0o0..=0o7, source) => {
                let destination = match destination {
                    0o0 => R8(B),
                    0o1 => R8(C),
                    0o2 => R8(D),
                    0o3 => R8(E),
                    0o4 => R8(H),
                    0o5 => R8(L),
                    0o6 => Ind(Indirect::R16(HL)),
                    0o7 => R8(A),
                    _ => unreachable!(),
                };
                let source = match source {
                    0o0 => R8(B),
                    0o1 => R8(C),
                    0o2 => R8(D),
                    0o3 => R8(E),
                    0o4 => R8(H),
                    0o5 => R8(L),
                    0o6 => Ind(Indirect::R16(HL)),
                    0o7 => R8(A),
                    _ => unreachable!(),
                };

                self.state = State::Execute(Operation::Load(destination, source), 0);
            }
            (0o2, op, target) => {
                let target = match target {
                    0 => R8(B),
                    1 => R8(C),
                    2 => R8(D),
                    3 => R8(E),
                    4 => R8(H),
                    5 => R8(L),
                    6 => Ind(Indirect::R16(HL)),
                    7 => R8(A),
                    _ => unreachable!(),
                };
                self.state = State::Execute(
                    match op {
                        0 => Operation::Add(R8(A), target),
                        1 => Operation::Adc(target),
                        2 => Operation::Sub(target),
                        3 => Operation::Sbc(target),
                        4 => Operation::And(target),
                        5 => Operation::Xor(target),
                        6 => Operation::Or(target),
                        7 => Operation::Compare(target),
                        _ => unreachable!(),
                    },
                    0,
                );
            }
            (0o3, condition @ 0o0..=0o3, 0o0) => {
                use Condition::*;
                let condition = match condition {
                    0o0 => NZ,
                    0o1 => Z,
                    0o2 => NC,
                    0o3 => C,
                    _ => unreachable!(),
                };
                self.state = State::Execute(Operation::Return(condition), 0);
            }
            (0o3, kind @ (0o4 | 0o6), 0o0) => match step {
                0 => {
                    self.decode_state.value = ctx.memory.read_u8(self.pc);
                    self.increment_pc(ctx);
                }
                1 => {
                    let address = u16::from_le_bytes([self.decode_state.value, 0xFF]);
                    self.state = State::Execute(
                        match kind {
                            0o4 => Operation::Load(Ind(Indirect::Imm16(address)), R8(A)),
                            0o6 => Operation::Load(R8(A), Ind(Indirect::Imm16(address))),
                            _ => unreachable!(),
                        },
                        0,
                    );
                }
                _ => unreachable!(),
            },
            (0o3, 0o5, 0o0) => match step {
                0 => {
                    self.decode_state.offset = ctx.memory.read_u8(self.pc) as i8;
                    self.increment_pc(ctx);
                }
                1 => {
                    self.state = State::Execute(Operation::AddStack(self.decode_state.offset), 0);
                }
                _ => unreachable!(),
            },
            (0o3, 0o7, 0o0) => match step {
                0 => {
                    self.decode_state.offset = ctx.memory.read_u8(self.pc) as i8;
                    self.increment_pc(ctx);
                }
                1 => {
                    self.state =
                        State::Execute(Operation::LoadStackOffset(self.decode_state.offset), 0);
                }
                _ => unreachable!(),
            },
            (0o3, 0o7, 0o1) => {
                self.state = State::Execute(Operation::Load(R16(SP), R16(HL)), 0);
            }
            (0o3, target @ (0 | 2 | 4 | 6), kind @ (0o1 | 0o5)) => {
                let target = match target {
                    0 => BC,
                    2 => DE,
                    4 => HL,
                    6 => AF,
                    _ => unreachable!(),
                };
                self.state = State::Execute(
                    match kind {
                        0o1 => Operation::Pop(target),
                        0o5 => Operation::Push(target),
                        _ => unreachable!(),
                    },
                    0,
                );
            }
            (0o3, 0o1, 0o1) => {
                self.state = State::Execute(Operation::Return(Condition::None), 0);
            }
            (0o3, 0o3, 0o1) => {
                self.state = State::Execute(Operation::ReturnInterrupt, 0);
            }
            (0o3, 0o5, 0o1) => {
                self.state = State::Execute(Operation::Jump(Condition::None, R16(HL)), 0);
            }
            (0o3, condition @ 0o0..=0o3, 0o2) => {
                use Condition::*;
                let condition = match condition {
                    0o0 => NZ,
                    0o1 => Z,
                    0o2 => NC,
                    0o3 => C,
                    _ => unreachable!(),
                };
                match step {
                    0 => {
                        self.decode_state.lsb = ctx.memory.read_u8(self.pc);
                        self.increment_pc(ctx);
                    }
                    1 => {
                        self.decode_state.msb = ctx.memory.read_u8(self.pc);
                        self.increment_pc(ctx);
                    }
                    2 => {
                        let address =
                            u16::from_le_bytes([self.decode_state.lsb, self.decode_state.msb]);
                        self.state = State::Execute(Operation::Jump(condition, Imm16(address)), 0);
                    }
                    _ => unreachable!(),
                }
            }
            (0o3, op @ 0o4..=0o7, 0o2) => {
                let source = R8(A);

                if op & 1 == 0 {
                    let dest = Ind(Indirect::C);
                    self.state = State::Execute(
                        match op {
                            0o4..=0o5 => Operation::Load(dest, source),
                            0o6..=0o7 => Operation::Load(source, dest),
                            _ => unreachable!(),
                        },
                        0,
                    );
                    return;
                } else {
                    match step {
                        0 => {
                            self.decode_state.lsb = ctx.memory.read_u8(self.pc);
                            self.increment_pc(ctx);
                        }
                        1 => {
                            self.decode_state.msb = ctx.memory.read_u8(self.pc);
                            self.increment_pc(ctx);
                        }
                        2 => {
                            let address =
                                u16::from_le_bytes([self.decode_state.lsb, self.decode_state.msb]);
                            let dest = Ind(Indirect::Imm16(address));
                            self.state = State::Execute(
                                match op {
                                    0o4..=0o5 => Operation::Load(dest, source),
                                    0o6..=0o7 => Operation::Load(source, dest),
                                    _ => unreachable!(),
                                },
                                0,
                            );
                        }
                        _ => unreachable!(),
                    }
                };
            }
            (0o3, 0o0, 0o3) => match step {
                0 => {
                    self.decode_state.lsb = ctx.memory.read_u8(self.pc);
                    self.increment_pc(ctx);
                }
                1 => {
                    self.decode_state.msb = ctx.memory.read_u8(self.pc);
                    self.increment_pc(ctx);
                }
                2 => {
                    let address =
                        u16::from_le_bytes([self.decode_state.lsb, self.decode_state.msb]);
                    self.state =
                        State::Execute(Operation::Jump(Condition::None, Imm16(address)), 0);
                }
                _ => unreachable!(),
            },
            (0o3, 0o1, 0o3) => self.fetch_cb_operation(ctx),
            (0o3, 0o2..=0o5, 0o3) => {
                error!("Invalid opcode");
                todo!("Decide what to do on invalid opcode")
            }
            (0o3, 0o6, 0o3) => {
                self.state = State::Execute(Operation::DisableInterrupt, 0);
            }
            (0o3, 0o7, 0o3) => self.state = State::Execute(Operation::EnableInterrupt, 0),
            (0o3, condition @ 0o0..=0o3, 0o4) => {
                use Condition::*;
                let condition = match condition {
                    0o0 => NZ,
                    0o1 => Z,
                    0o2 => NC,
                    0o3 => C,
                    _ => unreachable!(),
                };
                match step {
                    0 => {
                        self.decode_state.lsb = ctx.memory.read_u8(self.pc);
                        self.increment_pc(ctx);
                    }
                    1 => {
                        self.decode_state.msb = ctx.memory.read_u8(self.pc);
                        self.increment_pc(ctx);
                    }
                    2 => {
                        let address =
                            u16::from_le_bytes([self.decode_state.lsb, self.decode_state.msb]);
                        self.state = State::Execute(Operation::Call(condition, Imm16(address)), 0);
                    }
                    _ => unreachable!(),
                }
            }
            (0o3, 0o4..=0o7, 0o4) => {
                error!("Invalid opcode");
                todo!("Decide what to do on invalid opcode")
            }
            (0o3, 0o1, 0o5) => match step {
                0 => {
                    self.decode_state.lsb = ctx.memory.read_u8(self.pc);
                    self.increment_pc(ctx);
                }
                1 => {
                    self.decode_state.msb = ctx.memory.read_u8(self.pc);
                    self.increment_pc(ctx);
                }
                2 => {
                    let address =
                        u16::from_le_bytes([self.decode_state.lsb, self.decode_state.msb]);
                    self.state =
                        State::Execute(Operation::Call(Condition::None, Imm16(address)), 0);
                }
                _ => unreachable!(),
            },
            (0o3, 0o3 | 0o5 | 0o7, 0o5) => {
                error!("Invalid opcode");
                todo!("Decide what to do on invalid opcode")
            }
            (0o3, kind @ 0o0..=0o7, 0o6) => match step {
                0 => {
                    self.decode_state.value = ctx.memory.read_u8(self.pc);
                    self.increment_pc(ctx);
                }
                1 => {
                    let operation = match kind {
                        0o0 => |value| Operation::Add(R8(A), value),
                        0o1 => Operation::Adc,
                        0o2 => Operation::Sub,
                        0o3 => Operation::Sbc,
                        0o4 => Operation::And,
                        0o5 => Operation::Xor,
                        0o6 => Operation::Or,
                        0o7 => Operation::Compare,
                        _ => unreachable!(),
                    };
                    self.state =
                        State::Execute(operation(Target::Imm8(self.decode_state.value)), 0);
                }
                _ => unreachable!(),
            },
            (0o3, variant @ 0o0..=0o7, 0o7) => {
                let lsb = match variant {
                    0o0 => 0x00,
                    0o1 => 0x08,
                    0o2 => 0x10,
                    0o3 => 0x18,
                    0o4 => 0x20,
                    0o5 => 0x28,
                    0o6 => 0x30,
                    0o7 => 0x38,
                    _ => unreachable!(),
                };
                let address = u16::from_le_bytes([lsb, 0x00]);
                self.state = State::Execute(Operation::Restart(address), 0);
            }
            (0o4.., _, _) | (_, 0o10.., _) | (_, _, 0o10..) => unreachable!(),
        }
        if let State::Decode(step) = &mut self.state {
            *step += 1;
        }
    }

    pub(crate) fn fetch_cb_operation(&mut self, ctx: &mut Context<T>) {
        use Registers8::*;
        use Registers16::*;
        use Target::*;

        let State::Decode(step) = self.state else {
            unreachable!()
        };

        match step {
            0 => {
                self.decode_state.value = ctx.memory.read_u8(self.pc);
                self.increment_pc(ctx);
            }
            1 => {
                let (operation, target) = decompose_octal_cb(self.decode_state.value);

                let target = match target {
                    0o0 => R8(B),
                    0o1 => R8(C),
                    0o2 => R8(D),
                    0o3 => R8(E),
                    0o4 => R8(H),
                    0o5 => R8(L),
                    0o6 => Ind(Indirect::R16(HL)),
                    0o7 => R8(A),
                    _ => unreachable!(),
                };

                self.state = State::Execute(
                    match operation {
                        0o0 => Operation::Rotate(RotationType::Circular, Direction::Left, target),
                        0o1 => Operation::Rotate(RotationType::Circular, Direction::Right, target),
                        0o2 => {
                            Operation::Rotate(RotationType::NonCircular, Direction::Left, target)
                        }
                        0o3 => {
                            Operation::Rotate(RotationType::NonCircular, Direction::Right, target)
                        }
                        0o4 => Operation::ShiftArithmetic(Direction::Left, target),
                        0o5 => Operation::ShiftArithmetic(Direction::Right, target),
                        0o6 => Operation::Swap(target),
                        0o7 => Operation::ShiftRightLogical(target),
                        number @ 0o10..=0o17 => Operation::TestBit(number - 0o10, target),
                        number @ 0o20..=0o27 => Operation::ResetBit(number - 0o20, target),
                        number @ 0o30..=0o37 => Operation::SetBit(number - 0o30, target),
                        0o40.. => unreachable!(),
                    },
                    0,
                );
            }
            _ => unreachable!(),
        }
    }

    pub(crate) fn execute_operation(&mut self, operation: Operation, ctx: &mut Context<T>) {
        let State::Execute(_, step) = self.state else {
            unreachable!()
        };
        match operation {
            Operation::Nop => self.state = State::ExecutionDone,
            Operation::Halt => self.halt(ctx),
            Operation::Load(destination, source) => self.load(destination, source, ctx),
            Operation::Inc(target) => self.increment(target, ctx),
            Operation::Dec(target) => self.decrement(target, ctx),
            Operation::Stop => self.stop(ctx),
            Operation::Jump(condition, target) => self.jump(condition, target, ctx),
            Operation::JumpRelative(condition, offset) => {
                self.jump_relative(condition, offset, ctx)
            }
            Operation::RotateAccumulator(rotation_type, direction) => {
                self.rotate_accumulator(rotation_type, direction, ctx)
            }
            Operation::ComplementCarry => {
                let new_carry = !self.registers.f.carry();
                self.registers
                    .f
                    .set_subtract(false)
                    .set_half_carry(false)
                    .set_carry(new_carry);
                self.state = State::ExecutionDone;
            }
            Operation::SetCarry => {
                self.registers
                    .f
                    .set_subtract(false)
                    .set_half_carry(false)
                    .set_carry(true);
                self.state = State::ExecutionDone;
            }
            Operation::ComplementAccumulator => {
                self.registers.a = !self.registers.a;
                self.registers.f.set_subtract(true).set_half_carry(true);
                self.state = State::ExecutionDone;
            }
            Operation::DecimalAdjustAccumulator => {
                let mut adjustment = 0u8;
                if self.registers.f.subtract() {
                    if self.registers.f.half_carry() {
                        adjustment = adjustment.wrapping_add(0x6);
                    }
                    if self.registers.f.carry() {
                        adjustment = adjustment.wrapping_add(0x60);
                    }
                    self.registers.a = self.registers.a.wrapping_sub(adjustment);
                } else {
                    if self.registers.f.half_carry() || self.registers.a & 0xF > 0x9 {
                        adjustment = adjustment.wrapping_add(0x6);
                    }
                    if self.registers.f.carry() || self.registers.a > 0x99 {
                        adjustment = adjustment.wrapping_add(0x60);
                        self.registers.f.set_carry(true);
                    }
                    self.registers.a = self.registers.a.wrapping_add(adjustment);
                }
                self.registers
                    .f
                    .set_zero(self.registers.a == 0)
                    .set_half_carry(false);
                self.state = State::ExecutionDone;
            }
            Operation::Compare(target) => 'block: {
                let value = match target {
                    Target::R8(register) => self.read_register8(register),
                    Target::Imm8(value) => value,
                    Target::Ind(Indirect::R16(register)) => match step {
                        0 => {
                            self.execute_state.value =
                                ctx.memory.read_u8(self.read_register16(register));
                            break 'block;
                        }
                        1 => self.execute_state.value,
                        _ => unreachable!(),
                    },
                    _ => unimplemented!("Invalid targets for operation"),
                };
                let (result, carry) = self.registers.a.overflowing_sub(value);
                self.registers
                    .f
                    .set_zero(result == 0)
                    .set_subtract(true)
                    .set_half_carry(
                        (self.registers.a & 0xF).wrapping_sub(value & 0xF) & 0x10 == 0x10,
                    )
                    .set_carry(carry);
                self.state = State::ExecutionDone;
            }
            Operation::Or(target) => 'block: {
                let value = match target {
                    Target::R8(register) => self.read_register8(register),
                    Target::Imm8(value) => value,
                    Target::Ind(Indirect::R16(register)) => match step {
                        0 => {
                            self.execute_state.value =
                                ctx.memory.read_u8(self.read_register16(register));
                            break 'block;
                        }
                        1 => self.execute_state.value,
                        _ => unreachable!(),
                    },
                    _ => unimplemented!("Invalid targets for operation"),
                };
                let result = self.registers.a | value;
                self.registers
                    .f
                    .set_zero(result == 0)
                    .set_subtract(false)
                    .set_half_carry(false)
                    .set_carry(false);
                self.registers.a = result;
                self.state = State::ExecutionDone;
            }
            Operation::Xor(target) => 'block: {
                let value = match target {
                    Target::R8(register) => self.read_register8(register),
                    Target::Imm8(value) => value,
                    Target::Ind(Indirect::R16(register)) => match step {
                        0 => {
                            self.execute_state.value =
                                ctx.memory.read_u8(self.read_register16(register));
                            break 'block;
                        }
                        1 => self.execute_state.value,
                        _ => unreachable!(),
                    },
                    _ => unimplemented!("Invalid targets for operation"),
                };
                let result = self.registers.a ^ value;
                self.registers
                    .f
                    .set_zero(result == 0)
                    .set_subtract(false)
                    .set_half_carry(false)
                    .set_carry(false);
                self.registers.a = result;
                self.state = State::ExecutionDone;
            }
            Operation::And(target) => 'block: {
                let value = match target {
                    Target::R8(register) => self.read_register8(register),
                    Target::Imm8(value) => value,
                    Target::Ind(Indirect::R16(register)) => match step {
                        0 => {
                            self.execute_state.value =
                                ctx.memory.read_u8(self.read_register16(register));
                            break 'block;
                        }
                        1 => self.execute_state.value,
                        _ => unreachable!(),
                    },
                    _ => unimplemented!("Invalid targets for operation"),
                };
                let result = self.registers.a & value;
                self.registers
                    .f
                    .set_zero(result == 0)
                    .set_subtract(false)
                    .set_half_carry(true)
                    .set_carry(false);
                self.registers.a = result;
                self.state = State::ExecutionDone;
            }
            Operation::Sbc(target) => 'block: {
                let value = match target {
                    Target::R8(register) => self.read_register8(register),
                    Target::Imm8(value) => value,
                    Target::Ind(Indirect::R16(register)) => match step {
                        0 => {
                            self.execute_state.value =
                                ctx.memory.read_u8(self.read_register16(register));
                            break 'block;
                        }
                        1 => self.execute_state.value,
                        _ => unreachable!(),
                    },
                    _ => unimplemented!("Invalid targets for operation"),
                };
                let incoming_carry = self.registers.f.carry() as u8;
                let (result, carry1) = self.registers.a.overflowing_sub(value);
                let (result, carry2) = result.overflowing_sub(incoming_carry);

                self.registers
                    .f
                    .set_zero(result == 0)
                    .set_subtract(true)
                    .set_half_carry(
                        (self.registers.a & 0xF)
                            .wrapping_sub(value & 0xF)
                            .wrapping_sub(incoming_carry & 0xF)
                            & 0x10
                            == 0x10,
                    )
                    .set_carry(carry1 || carry2);
                self.registers.a = result;
                self.state = State::ExecutionDone;
            }
            Operation::Sub(target) => 'block: {
                let value = match target {
                    Target::R8(register) => self.read_register8(register),
                    Target::Imm8(value) => value,
                    Target::Ind(Indirect::R16(register)) => match step {
                        0 => {
                            self.execute_state.value =
                                ctx.memory.read_u8(self.read_register16(register));
                            break 'block;
                        }
                        1 => self.execute_state.value,
                        _ => unreachable!(),
                    },
                    _ => unimplemented!("Invalid targets for operation"),
                };
                let (result, carry) = self.registers.a.overflowing_sub(value);
                self.registers
                    .f
                    .set_zero(result == 0)
                    .set_subtract(true)
                    .set_half_carry(
                        (self.registers.a & 0xF).wrapping_sub(value & 0xF) & 0x10 == 0x10,
                    )
                    .set_carry(carry);
                self.registers.a = result;
                self.state = State::ExecutionDone;
            }
            Operation::Adc(target) => 'block: {
                let value = match target {
                    Target::R8(register) => self.read_register8(register),
                    Target::Imm8(value) => value,
                    Target::Ind(Indirect::R16(register)) => match step {
                        0 => {
                            self.execute_state.value =
                                ctx.memory.read_u8(self.read_register16(register));
                            break 'block;
                        }
                        1 => self.execute_state.value,
                        _ => unreachable!(),
                    },
                    _ => unimplemented!("Invalid targets for operation"),
                };
                let incoming_carry = self.registers.f.carry() as u8;
                let (result, carry1) = self.registers.a.overflowing_add(value);
                let (result, carry2) = result.overflowing_add(incoming_carry);

                self.registers
                    .f
                    .set_zero(result == 0)
                    .set_subtract(false)
                    .set_half_carry(
                        (self.registers.a & 0xF)
                            .wrapping_add(value & 0xF)
                            .wrapping_add(incoming_carry & 0xF)
                            & 0x10
                            == 0x10,
                    )
                    .set_carry(carry1 || carry2);
                self.registers.a = result;
                self.state = State::ExecutionDone;
            }
            Operation::Add(destination, source) => match destination {
                Target::R8(Registers8::A) => 'block: {
                    let value = match source {
                        Target::R8(register) => self.read_register8(register),
                        Target::Imm8(value) => value,
                        Target::Ind(Indirect::R16(register)) => match step {
                            0 => {
                                self.execute_state.value =
                                    ctx.memory.read_u8(self.read_register16(register));
                                break 'block;
                            }
                            1 => self.execute_state.value,
                            _ => unreachable!(),
                        },
                        _ => unimplemented!("Invalid targets for operation"),
                    };

                    let (result, carry) = self.registers.a.overflowing_add(value);

                    self.registers
                        .f
                        .set_zero(result == 0)
                        .set_subtract(false)
                        .set_half_carry(
                            (self.registers.a & 0xF).wrapping_add(value & 0xF) & 0x10 == 0x10,
                        )
                        .set_carry(carry);
                    self.registers.a = result;
                    self.state = State::ExecutionDone;
                }
                Target::R16(Registers16::HL) => match source {
                    Target::R16(register) => match step {
                        0 => {
                            let (l, r) = (self.registers.hl(), self.read_register16(register));
                            let (result, carry) = l.overflowing_add(r);
                            let quarter_carry = ((l & 0xF) + (r & 0xF)) & 0x10 == 0x10;
                            let half_carry = ((l & 0xFF) + (r & 0xFF)) & 0x100 == 0x100;
                            self.execute_state.three_quarter_carry =
                                ((l & 0xFFF) + (r & 0xFFF)) & 0x1000 == 0x1000;
                            let [lsb, msb] = result.to_le_bytes();
                            self.execute_state.msb = msb;
                            self.execute_state.carry = carry;

                            self.registers.l = lsb;
                            self.registers
                                .f
                                .set_subtract(false)
                                .set_half_carry(quarter_carry)
                                .set_carry(half_carry);
                        }
                        1 => {
                            self.registers.h = self.execute_state.msb;
                            self.registers
                                .f
                                .set_subtract(false)
                                .set_half_carry(self.execute_state.three_quarter_carry)
                                .set_carry(self.execute_state.carry);
                            self.state = State::ExecutionDone;
                        }
                        _ => unreachable!(),
                    },
                    _ => unimplemented!("Invalid operation"),
                },
                _ => unimplemented!("Invalid operation"),
            },
            Operation::Return(condition) => {
                let condition_met = match condition {
                    Condition::None => true,
                    Condition::NZ => !self.registers.f.zero(),
                    Condition::Z => self.registers.f.zero(),
                    Condition::NC => !self.registers.f.carry(),
                    Condition::C => self.registers.f.carry(),
                };
                let step = if matches!(condition, Condition::None) {
                    step + 1
                } else {
                    step
                };
                match step {
                    0 => {
                        // Stall on conditional return for a check, see https://gist.github.com/SonoSooS/c0055300670d678b5ae8433e20bea595#ret-and-reti
                    }
                    1 => 'block: {
                        if !condition_met {
                            self.state = State::ExecutionDone;
                            break 'block;
                        }
                        self.execute_state.lsb = ctx.memory.read_u8(self.registers.sp);
                        self.registers.sp = self.registers.sp.wrapping_add(1);
                    }
                    2 => {
                        self.execute_state.msb = ctx.memory.read_u8(self.registers.sp);
                        self.registers.sp = self.registers.sp.wrapping_add(1);
                    }
                    3 => {
                        self.pc =
                            u16::from_le_bytes([self.execute_state.lsb, self.execute_state.msb]);
                    }
                    4 => {
                        self.state = State::ExecutionDone;
                    }
                    _ => unreachable!(),
                }
            }
            Operation::Push(register) => match step {
                0 => {
                    let [lsb, msb] = self.read_register16(register).to_le_bytes();
                    self.execute_state.lsb = lsb;
                    self.execute_state.msb = msb;
                    self.registers.sp = self.registers.sp.wrapping_sub(1);
                }
                1 => {
                    ctx.memory
                        .write_u8(self.registers.sp, self.execute_state.msb);
                    self.registers.sp = self.registers.sp.wrapping_sub(1);
                }
                2 => {
                    ctx.memory
                        .write_u8(self.registers.sp, self.execute_state.lsb);
                }
                3 => {
                    self.state = State::ExecutionDone;
                }
                _ => unreachable!(),
            },
            Operation::Pop(register) => match step {
                0 => {
                    self.execute_state.lsb = ctx.memory.read_u8(self.registers.sp);
                    self.registers.sp = self.registers.sp.wrapping_add(1);
                }
                1 => {
                    self.execute_state.msb = ctx.memory.read_u8(self.registers.sp);
                    self.registers.sp = self.registers.sp.wrapping_add(1);
                }
                2 => {
                    self.set_register16(
                        register,
                        u16::from_le_bytes([self.execute_state.lsb, self.execute_state.msb]),
                    );
                    self.state = State::ExecutionDone;
                }
                _ => unreachable!(),
            },
            Operation::ReturnInterrupt => match step {
                0 => {
                    self.execute_state.lsb = ctx.memory.read_u8(self.registers.sp);
                    self.registers.sp = self.registers.sp.wrapping_add(1);
                }
                1 => {
                    self.execute_state.msb = ctx.memory.read_u8(self.registers.sp);
                    self.registers.sp = self.registers.sp.wrapping_add(1);
                }
                2 => {
                    self.pc = u16::from_le_bytes([self.execute_state.lsb, self.execute_state.msb]);
                    self.ime = true;
                }
                3 => self.state = State::ExecutionDone,
                _ => unreachable!(),
            },
            Operation::AddStack(offset) => match step {
                0 => {
                    let [lsb, msb] = self.registers.sp.to_le_bytes();
                    let (result_lsb, carry) = lsb.overflowing_add(offset as u8);
                    self.registers
                        .f
                        .set_zero(false)
                        .set_subtract(false)
                        .set_half_carry((lsb & 0xF).wrapping_add(offset as u8 & 0xF) & 0x10 == 0x10)
                        .set_carry(carry);
                    self.execute_state.carry = carry;
                    self.execute_state.lsb = result_lsb;
                    self.execute_state.msb = msb;
                    self.execute_state.z_sign = (offset >> 7) & 0b1 == 0b1;
                }
                1 => {
                    let adjustment = if self.execute_state.z_sign {
                        0xFF
                    } else {
                        0x00
                    };
                    self.execute_state.msb = self
                        .execute_state
                        .msb
                        .wrapping_add(adjustment)
                        .wrapping_add(self.execute_state.carry.into());
                }
                2 => {
                    self.registers.sp =
                        u16::from_le_bytes([self.execute_state.lsb, self.execute_state.msb]);
                    self.state = State::ExecutionDone;
                }
                _ => unreachable!(),
            },
            Operation::LoadStackOffset(offset) => match step {
                0 => {
                    let [lsb, msb] = self.registers.sp.to_le_bytes();
                    let (result_lsb, carry) = lsb.overflowing_add(offset as u8);
                    self.registers.l = result_lsb;
                    self.registers
                        .f
                        .set_zero(false)
                        .set_subtract(false)
                        .set_half_carry((lsb & 0xF).wrapping_add(offset as u8 & 0xF) & 0x10 == 0x10)
                        .set_carry(carry);
                    self.execute_state.z_sign = (offset >> 7) & 0b1 == 0b1;
                    self.execute_state.msb = msb;
                }
                1 => {
                    let adjustment = if self.execute_state.z_sign {
                        0xFF
                    } else {
                        0x00
                    };
                    let result_msb = self
                        .execute_state
                        .msb
                        .wrapping_add(adjustment)
                        .wrapping_add(self.registers.f.carry().into());
                    self.registers.h = result_msb;
                    self.state = State::ExecutionDone;
                }
                _ => unreachable!(),
            },
            Operation::DisableInterrupt => {
                self.ime = false;
                self.state = State::ExecutionDone;
            }
            Operation::EnableInterrupt => {
                self.ime = true;
                self.state = State::ExecutionDone;
            }
            Operation::Call(condition, target) => {
                let condition_met = match condition {
                    Condition::None => true,
                    Condition::NZ => !self.registers.f.zero(),
                    Condition::Z => self.registers.f.zero(),
                    Condition::NC => !self.registers.f.carry(),
                    Condition::C => self.registers.f.carry(),
                };

                match step {
                    0 => {
                        if !condition_met {
                            self.state = State::ExecutionDone;
                        } else {
                            let [lsb, msb] = self.pc.to_le_bytes();
                            self.execute_state.lsb = lsb;
                            self.execute_state.msb = msb;
                            self.registers.sp = self.registers.sp.wrapping_sub(1);
                        }
                    }
                    1 => {
                        ctx.memory
                            .write_u8(self.registers.sp, self.execute_state.msb);
                        self.registers.sp = self.registers.sp.wrapping_sub(1);
                    }
                    2 => {
                        ctx.memory
                            .write_u8(self.registers.sp, self.execute_state.lsb);
                        let Target::Imm16(address) = target else {
                            unimplemented!("Invalid target for operation");
                        };
                        self.pc = address;
                    }
                    3 => {
                        self.state = State::ExecutionDone;
                    }
                    _ => unreachable!(),
                }
            }
            Operation::Restart(address) => match step {
                0 => {
                    let [lsb, msb] = self.pc.to_le_bytes();
                    self.registers.sp = self.registers.sp.wrapping_sub(1);
                    self.execute_state.lsb = lsb;
                    self.execute_state.msb = msb;
                }
                1 => {
                    ctx.memory
                        .write_u8(self.registers.sp, self.execute_state.msb);
                    self.registers.sp = self.registers.sp.wrapping_sub(1);
                }
                2 => {
                    ctx.memory
                        .write_u8(self.registers.sp, self.execute_state.lsb);
                    self.pc = address;
                }
                3 => {
                    self.state = State::ExecutionDone;
                }
                _ => unreachable!(),
            },
            Operation::Rotate(rotation_type, direction, target) => {
                self.rotate(rotation_type, direction, target, ctx)
            }
            Operation::ShiftArithmetic(direction, target) => match target {
                Target::R8(register) => {
                    let prev_value = self.read_register8(register);
                    let (result, carry) = match direction {
                        Direction::Left => (prev_value << 1, (prev_value >> 7) & 0b1 == 0b1),
                        Direction::Right => (
                            (prev_value >> 1) | (prev_value & 0b1000_0000),
                            prev_value & 0b1 == 0b1,
                        ),
                    };

                    self.registers
                        .f
                        .set_zero(result == 0)
                        .set_subtract(false)
                        .set_half_carry(false)
                        .set_carry(carry);

                    self.set_register8(register, result);
                    self.state = State::ExecutionDone;
                }

                Target::Ind(Indirect::R16(Registers16::HL)) => match step {
                    0 => {
                        self.execute_state.value = ctx.memory.read_u8(self.registers.hl());
                    }
                    1 => {
                        let (result, carry) = match direction {
                            Direction::Left => (
                                self.execute_state.value << 1,
                                (self.execute_state.value >> 7) & 0b1 == 0b1,
                            ),
                            Direction::Right => (
                                (self.execute_state.value >> 1)
                                    | (self.execute_state.value & 0b1000_0000),
                                self.execute_state.value & 0b1 == 0b1,
                            ),
                        };

                        self.registers
                            .f
                            .set_zero(result == 0)
                            .set_subtract(false)
                            .set_half_carry(false)
                            .set_carry(carry);
                        ctx.memory.write_u8(self.registers.hl(), result);
                    }
                    2 => {
                        self.state = State::ExecutionDone;
                    }
                    _ => unreachable!(),
                },
                _ => unimplemented!("Invalid target for operation"),
            },
            Operation::Swap(target) => match target {
                Target::R8(register) => {
                    let prev_value = self.read_register8(register);

                    let low = prev_value & 0b1111;
                    let high = (prev_value >> 4) & 0b1111;
                    let result = (low << 4) | high;

                    self.registers
                        .f
                        .set_zero(result == 0)
                        .set_subtract(false)
                        .set_half_carry(false)
                        .set_carry(false);

                    self.set_register8(register, result);
                    self.state = State::ExecutionDone;
                }

                Target::Ind(Indirect::R16(Registers16::HL)) => match step {
                    0 => {
                        self.execute_state.value = ctx.memory.read_u8(self.registers.hl());
                    }
                    1 => {
                        let low = self.execute_state.value & 0b1111;
                        let high = (self.execute_state.value >> 4) & 0b1111;
                        let result = (low << 4) | high;

                        self.registers
                            .f
                            .set_zero(result == 0)
                            .set_subtract(false)
                            .set_half_carry(false)
                            .set_carry(false);
                        ctx.memory.write_u8(self.registers.hl(), result);
                    }
                    2 => {
                        self.state = State::ExecutionDone;
                    }
                    _ => unreachable!(),
                },
                _ => unimplemented!("Invalid target for operation"),
            },
            Operation::ShiftRightLogical(target) => match target {
                Target::R8(register) => {
                    let prev_value = self.read_register8(register);

                    let result = prev_value >> 1;
                    let carry = prev_value & 0b1 == 0b1;

                    self.registers
                        .f
                        .set_zero(result == 0)
                        .set_subtract(false)
                        .set_half_carry(false)
                        .set_carry(carry);

                    self.set_register8(register, result);
                    self.state = State::ExecutionDone;
                }

                Target::Ind(Indirect::R16(Registers16::HL)) => match step {
                    0 => {
                        self.execute_state.value = ctx.memory.read_u8(self.registers.hl());
                    }
                    1 => {
                        let result = self.execute_state.value >> 1;
                        let carry = self.execute_state.value & 0b1 == 0b1;

                        self.registers
                            .f
                            .set_zero(result == 0)
                            .set_subtract(false)
                            .set_half_carry(false)
                            .set_carry(carry);
                        ctx.memory.write_u8(self.registers.hl(), result);
                    }
                    2 => {
                        self.state = State::ExecutionDone;
                    }
                    _ => unreachable!(),
                },
                _ => unimplemented!("Invalid target for operation"),
            },

            Operation::TestBit(bit, target) => match target {
                Target::R8(register) => {
                    let prev_value = self.read_register8(register);

                    self.registers
                        .f
                        .set_zero((prev_value >> bit) & 0b1 == 0)
                        .set_subtract(false)
                        .set_half_carry(true);
                    self.state = State::ExecutionDone;
                }

                Target::Ind(Indirect::R16(Registers16::HL)) => match step {
                    0 => {
                        self.execute_state.value = ctx.memory.read_u8(self.registers.hl());
                    }
                    1 => {
                        self.registers
                            .f
                            .set_zero((self.execute_state.value >> bit) & 0b1 == 0)
                            .set_subtract(false)
                            .set_half_carry(true);
                        self.state = State::ExecutionDone;
                    }
                    _ => unreachable!(),
                },
                _ => unimplemented!("Invalid target for operation"),
            },
            Operation::ResetBit(bit, target) => match target {
                Target::R8(register) => {
                    let prev_value = self.read_register8(register);
                    let result = prev_value & !(0b1 << bit);
                    self.set_register8(register, result);
                    self.state = State::ExecutionDone;
                }

                Target::Ind(Indirect::R16(Registers16::HL)) => match step {
                    0 => {
                        self.execute_state.value = ctx.memory.read_u8(self.registers.hl());
                    }
                    1 => {
                        let result = self.execute_state.value & !(0b1 << bit);
                        ctx.memory.write_u8(self.registers.hl(), result);
                    }
                    2 => {
                        self.state = State::ExecutionDone;
                    }
                    _ => unreachable!(),
                },
                _ => unimplemented!("Invalid target for operation"),
            },
            Operation::SetBit(bit, target) => match target {
                Target::R8(register) => {
                    let prev_value = self.read_register8(register);
                    let result = prev_value | (0b1 << bit);
                    self.set_register8(register, result);
                    self.state = State::ExecutionDone;
                }

                Target::Ind(Indirect::R16(Registers16::HL)) => match step {
                    0 => {
                        self.execute_state.value = ctx.memory.read_u8(self.registers.hl());
                    }
                    1 => {
                        let result = self.execute_state.value | (0b1 << bit);
                        ctx.memory.write_u8(self.registers.hl(), result);
                    }
                    2 => {
                        self.state = State::ExecutionDone;
                    }
                    _ => unreachable!(),
                },
                _ => unimplemented!("Invalid target for operation"),
            },
        }
        if let State::Execute(_, step) = &mut self.state {
            *step += 1;
        }
    }

    pub(crate) fn halt(&mut self, _ctx: &mut Context<T>) {
        self.halted = true;
        self.state = State::ExecutionDone;
    }

    pub(crate) fn load(&mut self, destination: Target, source: Target, ctx: &mut Context<T>) {
        let State::Execute(_, step) = self.state else {
            unreachable!()
        };
        // debug!("Load from {source:?} to {destination:?}");
        match source {
            Target::R8(register) => self.load8(destination, self.read_register8(register), ctx, 0),
            Target::Imm8(value) => self.load8(destination, value, ctx, 0),
            Target::R16(register) => {
                self.load16(destination, self.read_register16(register), ctx, 0)
            }
            Target::Imm16(value) => self.load16(destination, value, ctx, 0),
            Target::Ind(indirect) => match step {
                0 => match indirect {
                    Indirect::R16(register) => {
                        self.execute_state.value =
                            ctx.memory.read_u8(self.read_register16(register));
                    }
                    Indirect::Imm16(value) => {
                        self.execute_state.value = ctx.memory.read_u8(value);
                    }
                    Indirect::HLI => {
                        self.execute_state.value = ctx.memory.read_u8(self.registers.hl());
                        self.registers.set_hl(self.registers.hl().wrapping_add(1));
                    }
                    Indirect::HLD => {
                        self.execute_state.value = ctx.memory.read_u8(self.registers.hl());
                        self.registers.set_hl(self.registers.hl().wrapping_sub(1));
                    }
                    Indirect::C => {
                        self.execute_state.value = ctx
                            .memory
                            .read_u8(u16::from_le_bytes([self.registers.c, 0xFF]));
                    }
                },
                1.. => self.load8(destination, self.execute_state.value, ctx, 1),
                // _ => unreachable!(),
            },
        };
    }
    pub(crate) fn load8(
        &mut self,
        destination: Target,
        value: u8,
        ctx: &mut Context<T>,
        starting_step: usize,
    ) {
        let State::Execute(_, step) = self.state else {
            unreachable!()
        };
        // debug!("Load of value {value} to {destination:?}");
        match destination {
            Target::R8(register) => {
                self.set_register8(register, value);
                self.state = State::ExecutionDone;
            }
            Target::R16(_) => unimplemented!("Invalid 8-bit load to 16-bit register"),
            Target::Imm8(_) => unimplemented!("Invalid 8-bit load to immediate value"),
            Target::Imm16(_) => unimplemented!("Invalid 8-bit load to immediate value"),
            Target::Ind(indirect) => match step {
                val if val == starting_step => match indirect {
                    Indirect::R16(register) => {
                        ctx.memory.write_u8(self.read_register16(register), value);
                    }
                    Indirect::Imm16(address) => {
                        ctx.memory.write_u8(address, value);
                    }
                    Indirect::HLI => {
                        ctx.memory.write_u8(self.registers.hl(), value);
                        self.registers.set_hl(self.registers.hl().wrapping_add(1));
                    }
                    Indirect::HLD => {
                        ctx.memory.write_u8(self.registers.hl(), value);
                        self.registers.set_hl(self.registers.hl().wrapping_sub(1));
                    }
                    Indirect::C => {
                        let addr = u16::from_le_bytes([self.registers.c, 0xFF]);
                        ctx.memory.write_u8(addr, value);
                    }
                },
                val if val == starting_step + 1 => {
                    self.state = State::ExecutionDone;
                }
                _ => unreachable!(),
            },
        }
    }

    pub(crate) fn load16(
        &mut self,
        destination: Target,
        value: u16,
        ctx: &mut Context<T>,
        starting_step: usize,
    ) {
        let State::Execute(_, step) = self.state else {
            unreachable!()
        };
        // debug!("Load of value {value:04X} to {destination:?}");
        match destination {
            Target::R8(_) => unimplemented!("Invalid 16-bit write to 8-bit register"),
            Target::R16(register) => match step {
                x if x == starting_step => {
                    self.set_register16(register, value);
                }
                x if x == starting_step + 1 => {
                    self.state = State::ExecutionDone;
                }
                _ => unreachable!(),
            },
            Target::Imm8(_) => unimplemented!("Invalid write to immediate"),
            Target::Imm16(_) => unimplemented!("Invalid write to immediate"),
            Target::Ind(indirect) => match indirect {
                Indirect::R16(_) => unimplemented!("No indirect 16-bit writes to registers"),
                Indirect::Imm16(address) => match step {
                    x if x == starting_step => {
                        let [lsb, msb] = value.to_le_bytes();
                        // debug!("{lsb:02X}, {msb:02X}");
                        self.execute_state.msb = msb;
                        ctx.memory.write_u8(address, lsb);
                    }
                    x if x == starting_step + 1 => {
                        ctx.memory
                            .write_u8(address.wrapping_add(1), self.execute_state.msb);
                    }
                    x if x == starting_step + 2 => {
                        self.state = State::ExecutionDone;
                    }
                    _ => unreachable!(),
                },
                _ => unimplemented!("Invalid 16-bit write requested"),
            },
        }
    }

    pub(crate) fn read_register8(&self, register: Registers8) -> u8 {
        match register {
            Registers8::A => self.registers.a,
            Registers8::B => self.registers.b,
            Registers8::C => self.registers.c,
            Registers8::D => self.registers.d,
            Registers8::E => self.registers.e,
            Registers8::H => self.registers.h,
            Registers8::L => self.registers.l,
        }
    }

    pub(crate) fn read_register16(&self, register: Registers16) -> u16 {
        match register {
            Registers16::BC => self.registers.bc(),
            Registers16::DE => self.registers.de(),
            Registers16::HL => self.registers.hl(),
            Registers16::SP => self.registers.sp,
            Registers16::AF => self.registers.af(),
        }
    }
    pub(crate) fn set_register16(&mut self, register: Registers16, value: u16) {
        match register {
            Registers16::BC => self.registers.set_bc(value),
            Registers16::DE => self.registers.set_de(value),
            Registers16::HL => self.registers.set_hl(value),
            Registers16::SP => self.registers.sp = value,
            Registers16::AF => self.registers.set_af(value),
        }
    }

    pub(crate) fn set_register8(&mut self, register: Registers8, value: u8) {
        match register {
            Registers8::A => self.registers.a = value,
            Registers8::B => self.registers.b = value,
            Registers8::C => self.registers.c = value,
            Registers8::D => self.registers.d = value,
            Registers8::E => self.registers.e = value,
            Registers8::H => self.registers.h = value,
            Registers8::L => self.registers.l = value,
        };
    }

    pub(crate) fn increment(&mut self, target: Target, ctx: &mut Context<T>) {
        let State::Execute(_, step) = self.state else {
            unreachable!()
        };
        match target {
            Target::R8(register) => {
                let current = self.read_register8(register);
                let (result, _carry) = current.overflowing_add(1);
                let half_carry = current & 0x0F == 0x0F;
                self.set_register8(register, result);
                self.registers
                    .f
                    .set_zero(result == 0)
                    .set_subtract(false)
                    .set_half_carry(half_carry);
                self.state = State::ExecutionDone;
            }
            Target::R16(register) => match step {
                0 => {
                    self.set_register16(register, self.read_register16(register).wrapping_add(1));
                }
                1 => {
                    self.state = State::ExecutionDone;
                }
                _ => unreachable!(),
            },
            Target::Ind(indirect) => match indirect {
                Indirect::R16(register) => match step {
                    0 => {
                        self.execute_state.address = self.read_register16(register);
                        self.execute_state.value = ctx.memory.read_u8(self.execute_state.address);
                    }
                    1 => {
                        let (result, _carry) = self.execute_state.value.overflowing_add(1);
                        let half_carry = self.execute_state.value & 0x0F == 0x0F;
                        ctx.memory.write_u8(self.execute_state.address, result);
                        self.registers
                            .f
                            .set_zero(result == 0)
                            .set_subtract(false)
                            .set_half_carry(half_carry);
                    }
                    2 => self.state = State::ExecutionDone,
                    _ => unreachable!(),
                },
                _ => unimplemented!("Invalid target for operation"),
            },
            _ => unimplemented!("Invalid target for operation"),
        }
    }

    pub(crate) fn decrement(&mut self, target: Target, ctx: &mut Context<T>) {
        let State::Execute(_, step) = self.state else {
            unreachable!()
        };
        match target {
            Target::R8(register) => {
                let current = self.read_register8(register);
                let result = current.wrapping_sub(1);
                let half_carry = current & 0x0F == 0;
                self.set_register8(register, result);
                self.registers
                    .f
                    .set_zero(result == 0)
                    .set_subtract(true)
                    .set_half_carry(half_carry);
                self.state = State::ExecutionDone;
            }
            Target::R16(register) => match step {
                0 => {
                    self.set_register16(register, self.read_register16(register).wrapping_sub(1));
                }
                1 => {
                    self.state = State::ExecutionDone;
                }
                _ => unreachable!(),
            },
            Target::Ind(indirect) => match indirect {
                Indirect::R16(register) => match step {
                    0 => {
                        self.execute_state.address = self.read_register16(register);
                        self.execute_state.value = ctx.memory.read_u8(self.execute_state.address);
                        // debug!("Value: {}", self.execute_state.value);
                    }
                    1 => {
                        let result = self.execute_state.value.wrapping_sub(1);
                        let half_carry = self.execute_state.value & 0x0F == 0;
                        ctx.memory.write_u8(self.execute_state.address, result);
                        // debug!("Result: {}", result);
                        // debug!(
                        // "Written Memory: {}",
                        // ctx.memory.read_u8(self.execute_state.address)
                        // );

                        self.registers
                            .f
                            .set_zero(result == 0)
                            .set_subtract(true)
                            .set_half_carry(half_carry);
                    }
                    2 => self.state = State::ExecutionDone,
                    _ => unreachable!(),
                },
                _ => unimplemented!("Invalid target for operation"),
            },
            _ => unimplemented!("Invalid target for operation"),
        }
    }

    pub(crate) fn stop(&self, _ctx: &mut Context<T>) {
        todo!()
    }

    pub(crate) fn jump(&mut self, condition: Condition, target: Target, ctx: &mut Context<T>) {
        // debug!("Jump! {target:?}");
        let State::Execute(_, step) = self.state else {
            unreachable!()
        };
        match target {
            Target::R16(register) => {
                self.pc = self.read_register16(register);
                self.state = State::ExecutionDone;
            }
            Target::Imm16(address) => match step {
                0 => {
                    self.execute_state.condition_met = match condition {
                        Condition::None => true,
                        Condition::NZ => !self.registers.f.zero(),
                        Condition::Z => self.registers.f.zero(),
                        Condition::NC => !self.registers.f.carry(),
                        Condition::C => self.registers.f.carry(),
                    };
                    if self.execute_state.condition_met {
                        self.pc = address;
                    } else {
                        self.state = State::ExecutionDone;
                    }
                }
                1 => self.state = State::ExecutionDone,
                _ => unreachable!(),
            },

            _ => unimplemented!("Invalid target for operation"),
        }
    }

    pub(crate) fn jump_relative(&mut self, condition: Condition, offset: i8, ctx: &mut Context<T>) {
        let State::Execute(_, step) = self.state else {
            unreachable!()
        };
        match step {
            0 => {
                self.execute_state.condition_met = match condition {
                    Condition::None => true,
                    Condition::NZ => !self.registers.f.zero(),
                    Condition::Z => self.registers.f.zero(),
                    Condition::NC => !self.registers.f.carry(),
                    Condition::C => self.registers.f.carry(),
                };
            }
            1 => {
                if self.execute_state.condition_met {
                    self.execute_state.address = self.pc.wrapping_add_signed(offset as i16);
                } else {
                    self.state = State::ExecutionDone;
                }
            }
            2 => {
                self.pc = self.execute_state.address;
                self.state = State::ExecutionDone;
            }
            _ => unreachable!(),
        }
    }

    pub(crate) fn rotate_accumulator(
        &mut self,
        rotation_type: RotationType,
        direction: Direction,
        _ctx: &mut Context<T>,
    ) {
        let (result, carry) = match (rotation_type, direction) {
            (RotationType::Circular, Direction::Left) => {
                let b7 = (self.registers.a >> 7) & 0b1 == 1;
                (self.registers.a.rotate_left(1), b7)
            }
            (RotationType::Circular, Direction::Right) => {
                let b0 = self.registers.a & 0b1 == 1;
                (self.registers.a.rotate_right(1), b0)
            }
            (RotationType::NonCircular, Direction::Left) => {
                let b7 = (self.registers.a >> 7) & 0b1 == 1;
                (
                    (self.registers.a << 1) | self.registers.f.carry().conv::<u8>(),
                    b7,
                )
            }
            (RotationType::NonCircular, Direction::Right) => {
                let b0 = self.registers.a & 0b1 == 1;
                (
                    (self.registers.a >> 1) | (self.registers.f.carry().conv::<u8>() << 7),
                    b0,
                )
            }
        };
        self.registers.a = result;
        self.registers
            .f
            .set_zero(false)
            .set_subtract(false)
            .set_half_carry(false)
            .set_carry(carry);
        self.state = State::ExecutionDone;
    }

    pub(crate) fn rotate(
        &mut self,
        rotation_type: RotationType,
        direction: Direction,
        target: Target,
        ctx: &mut Context<T>,
    ) {
        let State::Execute(_, step) = self.state else {
            unreachable!()
        };
        let perform_rotation = |prev_value: u8| match (rotation_type, direction) {
            (RotationType::Circular, Direction::Left) => {
                let b7 = (prev_value >> 7) & 0b1 == 1;
                (prev_value.rotate_left(1), b7)
            }
            (RotationType::Circular, Direction::Right) => {
                let b0 = prev_value & 0b1 == 1;
                (prev_value.rotate_right(1), b0)
            }
            (RotationType::NonCircular, Direction::Left) => {
                let b7 = (prev_value >> 7) & 0b1 == 1;
                (
                    (prev_value << 1) | self.registers.f.carry().conv::<u8>(),
                    b7,
                )
            }
            (RotationType::NonCircular, Direction::Right) => {
                let b0 = prev_value & 0b1 == 1;
                (
                    (prev_value >> 1) | (self.registers.f.carry().conv::<u8>() << 7),
                    b0,
                )
            }
        };
        match target {
            Target::R8(register) => {
                let prev_value = self.read_register8(register);
                let (result, carry) = perform_rotation(prev_value);
                self.registers
                    .f
                    .set_zero(result == 0)
                    .set_subtract(false)
                    .set_half_carry(false)
                    .set_carry(carry);
                self.set_register8(register, result);
                self.state = State::ExecutionDone;
            }

            Target::Ind(Indirect::R16(Registers16::HL)) => match step {
                0 => {
                    self.execute_state.value = ctx.memory.read_u8(self.registers.hl());
                }
                1 => {
                    let (result, carry) = perform_rotation(self.execute_state.value);
                    self.registers
                        .f
                        .set_zero(result == 0)
                        .set_subtract(false)
                        .set_half_carry(false)
                        .set_carry(carry);
                    ctx.memory.write_u8(self.registers.hl(), result);
                }
                2 => {
                    self.state = State::ExecutionDone;
                }
                _ => unreachable!(),
            },
            _ => unimplemented!("Invalid target for operation"),
        }
    }
}

pub(crate) fn decompose_octal_triplet(value: u8) -> (u8, u8, u8) {
    ((value >> 6) & 0o7, (value >> 3) & 0o7, value & 0o7)
}

pub(crate) fn decompose_octal_cb(value: u8) -> (u8, u8) {
    ((value >> 3) & 0o77, value & 0o7)
}
